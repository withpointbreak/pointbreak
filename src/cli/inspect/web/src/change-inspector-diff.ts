/**
 * Change-aware routed annotated diff.
 *
 * This deliberately consumes the contextual exact-Revision document rather
 * than the raw resource endpoint: captured bytes, inline facts, and every
 * deep-link target have already been validated against one Change +
 * RevisionRef by `change-inspector-reading`. It reuses only the pure diff
 * renderer; no legacy router, store, controller, or snapshot lifecycle enters
 * the active reader.
 */

import type { ChangeInspectorRenderActions } from "./change-inspector-render";
import type { ChangeInspectorRoute } from "./change-inspector-router";
import type {
  ChangeRevisionDetail,
  FactContent,
  FactTarget,
} from "./change-protocol";
import {
  type Annotation,
  type DiffArtifact,
  type DiffCtx,
  fileFactCount,
  filePathLabel,
  matchDiffFiles,
  renderDiff,
  renderDiffFileBody,
  renderDiffNavSummary,
} from "./diff/render";
import { shortRef } from "./refs";

type DiffRoute = Extract<ChangeInspectorRoute, { kind: "diff" }>;

function capturedDiffArtifact(value: unknown): DiffArtifact | null {
  if (typeof value !== "object" || value === null) return null;
  const snapshot = (value as Record<string, unknown>).snapshot;
  if (typeof snapshot !== "object" || snapshot === null) return null;
  return Array.isArray((snapshot as Record<string, unknown>).files)
    ? (value as DiffArtifact)
    : null;
}

function annotationTarget(target: FactTarget): Annotation["target"] {
  return {
    kind: target.kind,
    filePath: target.filePath,
    startLine: target.startLine,
    endLine: target.endLine,
    side: target.side,
    observationId: target.observationId,
    inputRequestId: target.inputRequestId,
    assessmentId: target.assessmentId,
    eventId: target.eventId,
  };
}

function annotationBody(content: FactContent): string | undefined {
  return content.kind === "observation" || content.kind === "input_request"
    ? content.body
    : content.kind === "assessment" || content.kind === "validation"
      ? content.summary
      : undefined;
}

function annotationForFact(
  detail: ChangeRevisionDetail,
  fact: ChangeRevisionDetail["factPresentations"][number],
): Annotation | null {
  const presentation = detail.factContentPresentations?.[fact.factId];
  const content = presentation?.content;
  if (
    !content ||
    content.kind !== fact.family ||
    fact.originRevision.revisionId !== detail.revision.revisionId ||
    fact.originRevision.objectArtifactContentHash !==
      detail.revision.objectArtifactContentHash ||
    (fact.target !== undefined &&
      fact.target.revisionId !== detail.revision.revisionId)
  ) {
    return null;
  }
  const annotation: Annotation = {
    id: fact.factId,
    kind: content.kind === "input_request" ? "input-request" : content.kind,
    title:
      content.kind === "assessment"
        ? `assessment: ${content.assessment}`
        : content.kind === "validation"
          ? content.checkName
          : content.title,
    track: fact.trackId ?? "untracked",
    body: annotationBody(content),
    bodyContentType: presentation.contentType,
    bodyContentState: presentation.bodyContentState,
    ...(fact.target ? { target: annotationTarget(fact.target) } : {}),
  };
  if (content.kind === "input_request") {
    annotation.status = content.status;
    annotation.responses = content.responses?.map((response) => ({
      id: response.responseId,
      outcome: response.outcome,
      reason: response.reason,
      reasonContentType: response.contentType,
      reasonContentState: response.bodyContentState,
      verificationStatus: response.availability,
    }));
  } else if (content.kind === "assessment") {
    annotation.assessment = content.assessment;
    annotation.status = fact.familyState;
  } else if (content.kind === "validation") {
    annotation.status = content.status;
    annotation.command = content.command;
  }
  return annotation;
}

function annotationsForExactRevision(
  detail: ChangeRevisionDetail,
): Annotation[] {
  return detail.factPresentations.flatMap((fact) => {
    const annotation = annotationForFact(detail, fact);
    return annotation ? [annotation] : [];
  });
}

function exactRoute(route: DiffRoute, focus: DiffRoute["focus"]): DiffRoute {
  return { ...route, ...(focus ? { focus } : {}) };
}

function updateFocus(
  route: DiffRoute,
  patch: Partial<NonNullable<DiffRoute["focus"]>>,
): DiffRoute {
  const next = { ...route.focus, ...patch };
  for (const key of Object.keys(next) as Array<keyof typeof next>) {
    if (!next[key]) delete next[key];
  }
  return exactRoute(route, Object.keys(next).length ? next : undefined);
}

function expandFile(
  section: HTMLElement,
  file: DiffCtx["files"][number],
  ctx: DiffCtx,
): void {
  const body = section.querySelector<HTMLElement>("[data-dfile-body]");
  if (!body) return;
  if (body.dataset.rendered !== "1") {
    body.innerHTML = renderDiffFileBody(file, ctx.anchored);
    body.dataset.rendered = "1";
  }
  section.dataset.expanded = "true";
  section
    .querySelector<HTMLElement>(".dfile-head")
    ?.setAttribute("aria-expanded", "true");
}

function factTarget(root: HTMLElement, factId: string): HTMLElement | null {
  const matching = Array.from(
    root.querySelectorAll<HTMLElement>("[data-anno]"),
  ).filter((element) => element.dataset.anno === factId);
  return (
    matching.find((element) => element.classList.contains("anno")) ??
    matching[0] ??
    null
  );
}

function focusFile(body: HTMLElement, ctx: DiffCtx, filePath: string): void {
  const index = ctx.files.findIndex(
    (file) => file.new_path === filePath || file.old_path === filePath,
  );
  if (index < 0) return;
  const section = body.querySelector<HTMLElement>(`[data-dfile="${index}"]`);
  if (!section) return;
  expandFile(section, ctx.files[index], ctx);
  section.dataset.exactFocus = "true";
  section.scrollIntoView({ block: "start", behavior: "auto" });
  section.focus({ preventScroll: true });
}

function focusFact(body: HTMLElement, ctx: DiffCtx, factId: string): void {
  const fact = [
    ...ctx.anchored,
    ...ctx.decisionContext,
    ...ctx.unanchored,
  ].find((item) => item.id === factId);
  if (fact?.target?.filePath) focusFile(body, ctx, fact.target.filePath);
  const target = factTarget(body, factId);
  if (!target) return;
  target.dataset.exactFocus = "true";
  // Annotation cards are intentionally outside the natural Tab order. Make
  // only the deep-linked card programmatically focusable so assistive
  // technology follows the routed fact selection without adding a tab stop to
  // every inline fact.
  target.tabIndex = -1;
  target.scrollIntoView({ block: "center", behavior: "auto" });
  target.focus({ preventScroll: true });
}

function button(label: string, className = "ghost"): HTMLButtonElement {
  const element = document.createElement("button");
  element.type = "button";
  element.className = className;
  element.textContent = label;
  return element;
}

function renderNavigator(
  nav: HTMLElement,
  route: DiffRoute,
  actions: ChangeInspectorRenderActions,
  ctx: DiffCtx,
): void {
  const query = route.focus?.fileQuery ?? "";
  const match = matchDiffFiles(ctx, query);
  nav.replaceChildren();
  const summary = document.createElement("div");
  summary.innerHTML = renderDiffNavSummary({
    fileCount: ctx.files.length,
    factCount:
      ctx.anchored.length + ctx.decisionContext.length + ctx.unanchored.length,
    decisionContextCount: ctx.decisionContext.length,
    unanchoredCount: ctx.unanchored.length,
  });
  nav.append(summary);
  for (const diagnostic of match.diagnostics) {
    const notice = document.createElement("p");
    notice.className = "diff-file-notice";
    notice.textContent = diagnostic.message;
    nav.append(notice);
  }
  const files = document.createElement("ol");
  files.className = "diff-nav-files";
  for (const file of match.files) {
    const index = ctx.files.indexOf(file);
    const item = document.createElement("li");
    const trigger = button(filePathLabel(file), "diff-nav-file");
    trigger.dataset.navFile = String(index);
    const count = fileFactCount(file, ctx.anchored);
    trigger.setAttribute(
      "aria-label",
      `${filePathLabel(file)}${count ? `, ${count} inline facts` : ""}`,
    );
    trigger.addEventListener("click", () => {
      const filePath = file.new_path ?? file.old_path;
      if (!filePath) return;
      actions.navigate(updateFocus(route, { filePath }));
    });
    item.append(trigger);
    files.append(item);
  }
  nav.append(files);
  const facts = [...ctx.anchored, ...ctx.decisionContext, ...ctx.unanchored];
  if (facts.length > 0) {
    const heading = document.createElement("h3");
    heading.textContent = "Facts";
    nav.append(heading);
    const list = document.createElement("ol");
    for (const fact of facts) {
      const item = document.createElement("li");
      const trigger = button(fact.title, "diff-nav-fact");
      trigger.dataset.anno = fact.id;
      trigger.addEventListener("click", () =>
        actions.navigate(updateFocus(route, { factId: fact.id })),
      );
      item.append(trigger);
      list.append(item);
    }
    nav.append(list);
  }
}

function bindDiffBody(
  body: HTMLElement,
  route: DiffRoute,
  actions: ChangeInspectorRenderActions,
  ctx: DiffCtx,
): void {
  ctx.files.forEach((file, index) => {
    const section = body.querySelector<HTMLElement>(`[data-dfile="${index}"]`);
    if (!section) return;
    const path = file.new_path ?? file.old_path;
    if (path) section.dataset.filePath = path;
    if (file.old_path) section.dataset.oldFilePath = file.old_path;
    if (file.new_path) section.dataset.newFilePath = file.new_path;
    section.tabIndex = -1;
    const toggle = () => {
      if (section.dataset.expanded === "true") {
        section.dataset.expanded = "false";
        section
          .querySelector<HTMLElement>(".dfile-head")
          ?.setAttribute("aria-expanded", "false");
      } else {
        expandFile(section, file, ctx);
      }
    };
    const header = section.querySelector<HTMLElement>(".dfile-head");
    header?.addEventListener("click", toggle);
    header?.addEventListener("keydown", (event) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      toggle();
    });
  });
  const activateBodyTarget = (target: Element | null): void => {
    const renderAll = target?.closest<HTMLElement>("[data-render-diff-file]");
    if (renderAll) {
      const section = renderAll.closest<HTMLElement>(".dfile");
      const index = Number(section?.dataset.dfile);
      if (section && Number.isInteger(index))
        expandFile(section, ctx.files[index], ctx);
      return;
    }
    const noted = target?.closest<HTMLElement>(".drow-noted[data-anno]");
    if (noted?.dataset.anno)
      actions.navigate(updateFocus(route, { factId: noted.dataset.anno }));
  };
  // The body node survives polling and route refinements. Property handlers
  // replace the previous route closure instead of accumulating stale delegates.
  body.onclick = (event) =>
    activateBodyTarget(event.target instanceof Element ? event.target : null);
  body.onkeydown = (event) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    const target = event.target instanceof Element ? event.target : null;
    if (!target?.closest(".drow-noted[data-anno]")) return;
    event.preventDefault();
    activateBodyTarget(target);
  };
}

/** Mount a first-class full-frame annotated diff from one exact contextual read. */
export function renderChangeInspectorDiffPage(
  detail: ChangeRevisionDetail,
  route: DiffRoute,
  actions: ChangeInspectorRenderActions,
): boolean {
  const page = document.querySelector<HTMLElement>("#diff-page");
  const toolbar = document.querySelector<HTMLElement>("#toolbar");
  const split = document.querySelector<HTMLElement>(".split");
  const title = document.querySelector<HTMLElement>("#diff-page-title");
  const close = document.querySelector<HTMLButtonElement>("#diff-page-close");
  const input = document.querySelector<HTMLInputElement>("#diff-file-query");
  const nav = document.querySelector<HTMLElement>("#diff-page-nav-list");
  const body = document.querySelector<HTMLElement>("#diff-page-body");
  if (
    !page ||
    !toolbar ||
    !split ||
    !title ||
    !close ||
    !input ||
    !nav ||
    !body
  )
    return false;
  page.classList.remove("hidden");
  toolbar.classList.add("hidden");
  split.classList.add("hidden");
  title.textContent = `Annotated diff · ${shortRef(detail.revision.revisionId)}`;
  title.title = `exact Revision ${detail.revision.revisionId}; artifact ${detail.revision.objectArtifactContentHash}`;
  title.setAttribute(
    "aria-label",
    `Annotated diff for exact Revision ${detail.revision.revisionId}; artifact ${detail.revision.objectArtifactContentHash}`,
  );
  close.onclick = () =>
    actions.navigate({
      kind: "revision",
      changeId: route.changeId,
      revision: route.revision,
      query: route.query,
      ...(route.focus && (route.focus.factId || route.focus.filePath)
        ? {
            focus: {
              ...(route.focus.factId ? { factId: route.focus.factId } : {}),
              ...(route.focus.filePath
                ? { filePath: route.focus.filePath }
                : {}),
            },
          }
        : {}),
    });
  input.value = route.focus?.fileQuery ?? "";
  input.oninput = () =>
    actions.replace?.(updateFocus(route, { fileQuery: input.value }));

  const resource = detail.exactRevisionDocument;
  if (resource.availability !== "available") {
    body.replaceChildren(
      Object.assign(document.createElement("p"), {
        className: "empty",
        textContent:
          "Captured bytes are unavailable. The Inspector will not reconstruct a diff from Git or an associated commit.",
      }),
    );
    nav.replaceChildren();
    return true;
  }
  const artifact = capturedDiffArtifact(resource.capturedDocument);
  if (!artifact) {
    body.replaceChildren(
      Object.assign(document.createElement("p"), {
        className: "empty",
        textContent:
          "This exact resource does not contain a captured snapshot.",
      }),
    );
    nav.replaceChildren();
    return true;
  }
  const rendered = renderDiff(
    resource.resource.objectId,
    artifact,
    annotationsForExactRevision(detail),
  );
  body.innerHTML = rendered.html;
  bindDiffBody(body, route, actions, rendered.ctx);
  renderNavigator(nav, route, actions, rendered.ctx);
  if (route.focus?.filePath)
    focusFile(body, rendered.ctx, route.focus.filePath);
  if (route.focus?.factId) focusFact(body, rendered.ctx, route.focus.factId);
  return true;
}

/** Restore the ordinary Change reader shell after leaving the routed diff. */
export function hideChangeInspectorDiffPage(): void {
  document.querySelector("#diff-page")?.classList.add("hidden");
  document.querySelector("#toolbar")?.classList.remove("hidden");
  document.querySelector(".split")?.classList.remove("hidden");
}
