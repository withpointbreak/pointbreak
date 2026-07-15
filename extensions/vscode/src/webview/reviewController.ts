/// <reference lib="dom" />

import type { DiffRenderData } from "../diffDataSource";
import type {
  ReviewPanelFocus,
  SnapshotRangeTarget,
  WebviewToHost,
} from "../webviewProtocol";
import { diffStatusClass } from "./diff/classNames";
import { escapeHtml } from "./diff/escape";
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
  unanchoredReason,
} from "./diff/render";

interface ControllerStateApi {
  getState(): unknown;
  setState(state: unknown): void;
  postMessage?(message: WebviewToHost): void;
}

interface ControllerState {
  readonly version: 1;
  readonly locationKey: string;
  readonly filter: string;
  readonly expanded: readonly boolean[];
  readonly factCursor: number;
  readonly changeCursor: number;
}

/** Browser-only DOM state over the pure annotated-diff renderer. */
export class ReviewWebviewController {
  private context: DiffCtx | undefined;
  private state: ControllerState | undefined;

  constructor(
    private readonly root: HTMLElement,
    private readonly stateApi: ControllerStateApi,
  ) {
    root.addEventListener("click", (event) => this.onClick(event));
    root.addEventListener("input", (event) => this.onInput(event));
    root.ownerDocument.addEventListener("keydown", (event) =>
      this.onKeydown(event),
    );
  }

  render(data: DiffRenderData, focus?: ReviewPanelFocus): void {
    const result = renderDiff(
      data.snapshotId,
      data.artifact as DiffArtifact,
      data.annotations as Annotation[],
    );
    this.context = result.ctx;
    const locationKey =
      this.root.dataset.locationKey ?? `${data.revisionId}\n${data.snapshotId}`;
    const saved = controllerState(this.stateApi.getState(), locationKey);
    this.root.innerHTML = `<div class="diff-layout">
      <aside class="diff-nav" aria-label="diff navigator">
        <input id="diff-file-query" class="diff-file-search" type="search" placeholder="Filter files" aria-label="Filter files" value="${escapeHtml(saved?.filter ?? "")}">
        <div id="diff-nav-list"></div>
      </aside>
      <section id="diff-body" class="diff-body" aria-label="annotated diff">${result.html}</section>
    </div>
    <p id="diff-nav-status" class="empty" aria-live="polite">n/p facts · [/] changes</p>`;
    this.state =
      saved ??
      this.captureState({
        version: 1,
        locationKey,
        filter: "",
        expanded: [],
        factCursor: -1,
        changeCursor: -1,
      });
    if (saved) {
      this.restoreExpanded(saved.expanded);
    }
    this.bindSourceActions();
    this.renderNavigator();
    this.persist();
    this.focus(focus);
  }

  focus(focus?: ReviewPanelFocus): void {
    for (const element of this.root.querySelectorAll(".review-focus")) {
      element.classList.remove("review-focus");
    }
    if (!focus || !this.context) {
      return;
    }
    const candidates = focusCandidateIds(focus.id);
    let target = this.findAnnotation(candidates);
    if (!target) {
      const annotation = this.context.anchored.find((item) =>
        candidates.includes(item.id),
      );
      const path = annotation?.target?.filePath;
      const index = this.context.files.findIndex(
        (file) => file.old_path === path || file.new_path === path,
      );
      if (index >= 0) {
        const section = this.fileSections()[index];
        if (section) this.setExpanded(section, true);
        target = this.findAnnotation(candidates);
      }
    }
    if (target) {
      target.classList.add("review-focus");
      target.scrollIntoView({ block: "center" });
    }
  }

  error(message: string): void {
    this.context = undefined;
    this.state = undefined;
    this.root.innerHTML = `<p class="empty" role="alert">${escapeHtml(message)}</p>`;
  }

  private onClick(event: Event): void {
    const target = event.target;
    if (!(target instanceof Element)) return;

    const sourceAction = target.closest<HTMLElement>("[data-open-source]");
    if (sourceAction) {
      const sourceTarget = targetFromAction(sourceAction);
      if (sourceTarget) {
        this.stateApi.postMessage?.({
          type: "openSource",
          target: sourceTarget,
        });
      }
      return;
    }

    const renderAll = target.closest("[data-render-diff-file]");
    if (renderAll) {
      const section = renderAll.closest<HTMLElement>(".dfile");
      if (section) this.setExpanded(section, true);
      return;
    }
    const header = target.closest<HTMLElement>(".dfile-head");
    if (header) {
      const section = header.closest<HTMLElement>(".dfile");
      if (section) this.setExpanded(section, !this.isExpanded(section));
      return;
    }
    const file = target.closest<HTMLElement>("[data-nav-file]");
    if (file) {
      const section = this.fileSections()[Number(file.dataset.navFile)];
      if (section) {
        this.setExpanded(section, true);
        section.scrollIntoView({ block: "start" });
      }
      return;
    }
    const fact = target.closest<HTMLElement>("[data-anno]");
    if (fact?.dataset.anno) {
      this.focus({ kind: "attention", id: fact.dataset.anno });
    }
  }

  private onInput(event: Event): void {
    const input = event.target;
    if (
      !(input instanceof HTMLInputElement) ||
      input.id !== "diff-file-query"
    ) {
      return;
    }
    this.updateState({ filter: input.value });
    this.renderNavigator();
  }

  private onKeydown(event: KeyboardEvent): void {
    if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) {
      return;
    }
    const target = event.target;
    if (target instanceof Element) {
      const header = target.closest<HTMLElement>(".dfile-head");
      if (header && (event.key === "Enter" || event.key === " ")) {
        event.preventDefault();
        const section = header.closest<HTMLElement>(".dfile");
        if (section) this.setExpanded(section, !this.isExpanded(section));
        return;
      }
      const annotation = target.closest<HTMLElement>(".drow-noted[data-anno]");
      if (
        annotation?.dataset.anno &&
        (event.key === "Enter" || event.key === " ")
      ) {
        event.preventDefault();
        this.focus({ kind: "attention", id: annotation.dataset.anno });
        return;
      }
    }
    if (isEditable(target) || !["n", "p", "[", "]"].includes(event.key)) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "n") this.jump("fact", 1);
    if (event.key === "p") this.jump("fact", -1);
    if (event.key === "]") this.jump("change", 1);
    if (event.key === "[") this.jump("change", -1);
  }

  private jump(kind: "fact" | "change", direction: number): void {
    if (!this.state) return;
    const selector = kind === "fact" ? ".anno[data-anno]" : ".dhunk";
    const targets = [...this.root.querySelectorAll<HTMLElement>(selector)];
    if (!targets.length) return;
    for (const element of this.root.querySelectorAll(".review-current")) {
      element.classList.remove("review-current");
    }
    const current =
      kind === "fact" ? this.state.factCursor : this.state.changeCursor;
    const cursor = (current + direction + targets.length) % targets.length;
    const target = targets[cursor];
    const section = target.closest<HTMLElement>(".dfile");
    if (section) this.setExpanded(section, true);
    target.classList.add("review-current");
    target.scrollIntoView({ block: "center" });
    this.updateState(
      kind === "fact" ? { factCursor: cursor } : { changeCursor: cursor },
    );
    const status = this.root.querySelector("#diff-nav-status");
    if (status) {
      status.textContent = `${kind} ${cursor + 1}/${targets.length}`;
    }
  }

  private renderNavigator(): void {
    if (!this.context || !this.state) return;
    const context = this.context;
    const host = this.root.querySelector<HTMLElement>("#diff-nav-list");
    if (!host) return;
    const { files, diagnostics } = matchDiffFiles(context, this.state.filter);
    const matched = new Set(files);
    const fileItems = context.files
      .map((file, index) => ({ file, index }))
      .filter(({ file }) => matched.has(file))
      .map(({ file, index }) => {
        const count = fileFactCount(file, context.anchored);
        const badge = count ? `<span class="dfile-notes">${count}</span>` : "";
        return `<li><button class="diff-nav-file" data-nav-file="${index}" type="button">
          <span class="${diffStatusClass(escapeHtml(file.status ?? ""))}">${escapeHtml(file.status ?? "")}</span>
          <span class="dpath">${escapeHtml(filePathLabel(file))}</span>${badge}
        </button></li>`;
      })
      .join("");
    const summary = renderDiffNavSummary({
      fileCount: context.files.length,
      factCount: context.anchored.length + context.unanchored.length,
      unanchoredCount: context.unanchored.length,
    });
    const notice = diagnostics.length
      ? `<div class="diff-file-notice" role="status">${diagnostics.map(({ message }) => escapeHtml(message)).join(" ")}</div>`
      : "";
    const unanchored = context.unanchored.length
      ? `<section class="diff-unanchored" aria-label="unanchored review facts">
          <h3>${context.unanchored.length} not anchored to a diff line</h3>
          <ol>${context.unanchored
            .map(
              (annotation) =>
                `<li><button class="diff-nav-fact" data-anno="${escapeHtml(annotation.id)}" type="button"><span>${escapeHtml(annotation.title)}</span><span class="diff-nav-reason">${escapeHtml(unanchoredReason(annotation, context.filePaths))}</span></button></li>`,
            )
            .join("")}</ol>
        </section>`
      : "";
    host.innerHTML = `${summary}${notice}<ol class="diff-nav-files">${fileItems}</ol>${unanchored}`;
  }

  private setExpanded(section: HTMLElement, expanded: boolean): void {
    this.applyExpanded(section, expanded);
    if (this.state) {
      this.updateState({
        expanded: this.fileSections().map((file) => this.isExpanded(file)),
      });
    }
  }

  private applyExpanded(section: HTMLElement, expanded: boolean): void {
    if (expanded) this.ensureFileBody(section);
    section.dataset.expanded = String(expanded);
    section
      .querySelector<HTMLElement>(".dfile-head")
      ?.setAttribute("aria-expanded", String(expanded));
  }

  private ensureFileBody(section: HTMLElement): void {
    if (!this.context) return;
    const body = section.querySelector<HTMLElement>("[data-dfile-body]");
    if (!body || body.dataset.rendered) return;
    const index = Number(section.dataset.dfile);
    const file = this.context.files[index];
    if (!file) return;
    body.innerHTML = renderDiffFileBody(file, this.context.anchored);
    body.removeAttribute("data-fact-vicinity");
    body.dataset.rendered = "1";
    this.bindSourceActions(section);
  }

  private bindSourceActions(scope: ParentNode = this.root): void {
    if (!this.context) return;
    const sections =
      scope instanceof HTMLElement && scope.matches(".dfile[data-dfile]")
        ? [scope]
        : [...scope.querySelectorAll<HTMLElement>(".dfile[data-dfile]")];
    for (const section of sections) {
      const file = this.context.files[Number(section.dataset.dfile)];
      if (!file) continue;
      const rows = (file.hunks ?? []).flatMap((hunk) => hunk.rows ?? []);
      const elements = section.querySelectorAll<HTMLElement>(
        ".dfile-body .drow:not(.drow-meta)",
      );
      elements.forEach((element, index) => {
        const row = rows[index];
        if (!row) return;
        const side = row.kind === "removed" ? "old" : "new";
        const line = side === "old" ? row.old_line : row.new_line;
        const filePath = side === "old" ? file.old_path : file.new_path;
        if (line != null && filePath) {
          addSourceAction(element, {
            filePath,
            side,
            startLine: line,
            endLine: line,
          });
        }
      });
    }

    for (const annotation of scope.querySelectorAll<HTMLElement>(
      ".anno[data-anno]",
    )) {
      const id = annotation.dataset.anno;
      const fact = this.context.anchored.find((item) => item.id === id);
      const target = fact?.target;
      if (
        target?.kind === "range" &&
        target.filePath &&
        (target.side === "old" || target.side === "new") &&
        Number.isInteger(target.startLine) &&
        Number.isInteger(target.endLine ?? target.startLine)
      ) {
        addSourceAction(annotation, {
          filePath: target.filePath,
          side: target.side,
          startLine: target.startLine as number,
          endLine: (target.endLine ?? target.startLine) as number,
        });
      }
    }
  }

  private restoreExpanded(expanded: readonly boolean[]): void {
    this.fileSections().forEach((section, index) => {
      if (expanded[index] !== undefined) {
        this.applyExpanded(section, expanded[index]);
      }
    });
  }

  private captureState(state: ControllerState): ControllerState {
    return {
      ...state,
      expanded: this.fileSections().map((section) => this.isExpanded(section)),
    };
  }

  private updateState(patch: Partial<ControllerState>): void {
    if (!this.state) return;
    this.state = { ...this.state, ...patch };
    this.persist();
  }

  private persist(): void {
    if (this.state) this.stateApi.setState(this.state);
  }

  private fileSections(): HTMLElement[] {
    return [...this.root.querySelectorAll<HTMLElement>(".dfile[data-dfile]")];
  }

  private isExpanded(section: HTMLElement): boolean {
    return (
      section.querySelector(".dfile-head")?.getAttribute("aria-expanded") ===
      "true"
    );
  }

  private findAnnotation(ids: readonly string[]): HTMLElement | undefined {
    return [
      ...this.root.querySelectorAll<HTMLElement>(".anno[data-anno]"),
    ].find(
      (element) => !!element.dataset.anno && ids.includes(element.dataset.anno),
    );
  }
}

function addSourceAction(
  container: HTMLElement,
  target: SnapshotRangeTarget,
): void {
  if (container.querySelector(":scope > [data-open-source]")) return;
  const action = document.createElement("button");
  action.type = "button";
  action.className = "source-action";
  action.dataset.openSource = "true";
  action.dataset.sourceFile = target.filePath;
  action.dataset.sourceSide = target.side;
  action.dataset.sourceLine = String(target.startLine);
  action.dataset.sourceEndLine = String(target.endLine);
  action.ariaLabel = "Open captured range in source";
  action.title = "Open in source";
  action.textContent = "↗";
  container.append(action);
}

function targetFromAction(
  action: HTMLElement,
): SnapshotRangeTarget | undefined {
  const filePath = action.dataset.sourceFile;
  const side = action.dataset.sourceSide;
  const startLine = Number(action.dataset.sourceLine);
  const endLine = Number(action.dataset.sourceEndLine);
  if (
    !filePath ||
    (side !== "old" && side !== "new") ||
    !Number.isInteger(startLine) ||
    startLine <= 0 ||
    !Number.isInteger(endLine) ||
    endLine < startLine
  ) {
    return undefined;
  }
  return { filePath, side, startLine, endLine };
}

export function focusCandidateIds(attentionId: string): string[] {
  const separator = attentionId.indexOf(":");
  return separator < 0
    ? [attentionId]
    : [attentionId, attentionId.slice(separator + 1)];
}

function controllerState(
  value: unknown,
  locationKey: string,
): ControllerState | undefined {
  if (
    !isRecord(value) ||
    value.version !== 1 ||
    value.locationKey !== locationKey
  ) {
    return undefined;
  }
  if (
    typeof value.filter !== "string" ||
    !Array.isArray(value.expanded) ||
    !value.expanded.every((item) => typeof item === "boolean") ||
    !Number.isInteger(value.factCursor) ||
    !Number.isInteger(value.changeCursor)
  ) {
    return undefined;
  }
  return value as unknown as ControllerState;
}

function isEditable(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement ||
    (target instanceof HTMLElement && target.isContentEditable)
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
