/** DOM projection for the Change-first shell. It fetches nothing and owns no history. */

import { changeCardPresentation } from "./change-inspector-cards";
import {
  hideChangeInspectorDiffPage,
  renderChangeInspectorDiffPage,
} from "./change-inspector-diff";
import {
  eventSubjectLabel,
  eventTypeColor,
  presentEvent,
} from "./change-inspector-event-presentation";
import {
  renderChangeRevisionGraph,
  renderFactRelationshipGraph,
} from "./change-inspector-graphs";
import type { ChangeInspectorReading } from "./change-inspector-reading";
import type { ChangeInspectorRoute } from "./change-inspector-router";
import {
  eventAnnotatedDiffRoute,
  formatChangeInspectorRoute,
  lensForRoute,
  parseChangeInspectorRoute,
  queryForExactNavigation,
  showChangeInTimelineRoute,
  showRevisionInTimelineRoute,
} from "./change-inspector-router";
import type { ChangeInspectorSnapshot } from "./change-inspector-state";
import { renderChangeInspectorTimeline } from "./change-inspector-timeline";
import type { TimelineMonitorSnapshot } from "./change-inspector-timeline-monitor";
import type {
  ChangeDetail,
  ChangePageQuery,
  EventHistoryDocument,
  EventHistoryEntry,
  EventHistoryQuery,
  FactContent,
  RevisionRef,
  RevisionResource,
} from "./change-protocol";
import { filterChipsFor, removeFilterChipToken } from "./chips";
import {
  type DiffArtifact,
  renderDiff,
  renderDiffFileBody,
} from "./diff/render";
import { renderBodyContent } from "./markdown";
import {
  compactIdentityText,
  exactRevisionAccessibleIdentity,
  shortExactRevision,
  shortRef,
} from "./refs";

export interface ChangeInspectorNavigationActions {
  navigate(route: Exclude<ChangeInspectorRoute, { kind: "invalid" }>): void;
  navigateTimelineBoundary?(
    boundary: "first" | "last",
    route: Extract<ChangeInspectorRoute, { kind: "timeline" }>,
  ): Promise<Extract<ChangeInspectorRoute, { kind: "timeline" }> | null>;
  revealTimelineEvent?(eventId: string): boolean;
  toggleTimelineMonitoring?(): void;
  parkTimelineMonitoring?(): void;
}

export interface ChangeInspectorRenderActions
  extends ChangeInspectorNavigationActions {
  /** Replace local presentation-only route refinements and query edits. */
  replace(route: Exclude<ChangeInspectorRoute, { kind: "invalid" }>): void;
}

export interface ChangeInspectorDetailPresentation {
  reading: ChangeInspectorReading | null;
  refusal: string | null;
  timeline?: TimelineMonitorSnapshot | null;
}

function routeForLens(
  lens: "timeline" | "changes" | "attention",
  current: ChangeInspectorRoute,
): Exclude<ChangeInspectorRoute, { kind: "invalid" }> {
  if (lens === "timeline") {
    return {
      kind: "timeline",
      historyQuery:
        current.kind === "timeline" || current.kind === "event"
          ? { ...current.historyQuery, after: undefined, at: undefined }
          : {},
    };
  }
  return {
    kind: "lens",
    lens,
    query:
      current.kind === "invalid" ||
      current.kind === "timeline" ||
      current.kind === "event"
        ? {}
        : { ...current.query, after: undefined },
  };
}

function syncLensLinks(
  active: "timeline" | "changes" | "attention",
  current: ChangeInspectorRoute,
): void {
  document
    .querySelectorAll<HTMLAnchorElement>("#lens-switcher a[data-lens]")
    .forEach((link) => {
      const lens = link.dataset.lens;
      if (lens !== "timeline" && lens !== "changes" && lens !== "attention")
        return;
      link.href = formatChangeInspectorRoute(routeForLens(lens, current));
      if (lens === active) link.setAttribute("aria-current", "page");
      else link.removeAttribute("aria-current");
    });
}

const FILTER_OPTIONS = [
  [
    "topology",
    [
      "initial",
      "replacement",
      "replacement_divergent",
      "consolidation",
      "parallel_current",
      "mixed",
      "incomplete",
      "cycle_conflicted",
    ],
  ],
  ["lifecycle", ["incomplete", "conflicted", "in_progress", "accepted"]],
  ["attention", ["clear", "in_progress", "incomplete", "conflicted"]],
  ["availability", ["available", "incomplete"]],
] as const;

function setCompactIdentityText(
  element: HTMLElement,
  text: string,
  accessibleName = text,
): void {
  const visible = compactIdentityText(text);
  element.textContent = visible;
  if (visible !== text) {
    element.title = text;
    element.setAttribute("aria-label", accessibleName);
  }
}

function message(text: string): HTMLParagraphElement {
  const element = document.createElement("p");
  element.className = "empty";
  setCompactIdentityText(element, text);
  return element;
}

function selectOption(
  label: string,
  value: string,
  artifactHash?: string,
  revisionId?: string,
  title?: string,
  accessibleName?: string,
): HTMLOptionElement {
  const option = document.createElement("option");
  option.textContent = label;
  option.value = value;
  if (artifactHash) option.dataset.artifactHash = artifactHash;
  if (revisionId) option.dataset.revisionId = revisionId;
  if (title) option.title = title;
  if (accessibleName) option.setAttribute("aria-label", accessibleName);
  return option;
}

function exactRevisionOptionValue(
  revisionId: string,
  objectArtifactContentHash: string,
): string {
  return JSON.stringify([revisionId, objectArtifactContentHash]);
}

function setText(selector: string, value: string): void {
  const element = document.querySelector<HTMLElement>(selector);
  if (element) element.textContent = value;
}

function replaceMasterWith(...children: Node[]): void {
  const master = document.querySelector<HTMLElement>("#master");
  if (!master) return;
  // Render keys are valid only while their keyed DOM is still mounted. A
  // loading or refusal plane replaces that DOM, so retaining either key would
  // make a same-generation recovery repaint a detached list and leave this
  // plane visible forever.
  delete master.dataset.changeListKey;
  delete master.dataset.timelineKey;
  master.replaceChildren(...children);
}

function replaceDetailWith(...children: Node[]): void {
  const detail = document.querySelector<HTMLElement>("#detail-body");
  if (!detail) return;
  // `changeReadingKey` describes the exact reading DOM, not merely the route.
  // Clear it whenever a placeholder/refusal replaces that DOM so reopening the
  // same exact route can render the reading again.
  delete detail.dataset.changeReadingKey;
  detail.replaceChildren(...children);
}

/** Prepare retained model-neutral chrome for Timeline and the Change lenses. */
export function prepareChangeInspectorShell(
  actions: ChangeInspectorNavigationActions,
): void {
  // Display preferences are reader-local presentation state. They do not alter
  // the server-owned Change generation, so keep them available in this reader.
  document.querySelector("#view-controls")?.classList.remove("hidden");
  setText("#view-toggle", "View");
  document.querySelector("#view-order-section")?.classList.add("hidden");
  document.querySelector("#view-sort-section")?.classList.add("hidden");
  document
    .querySelector("#jump-latest")
    ?.closest(".control-section")
    ?.classList.add("hidden");
  document.querySelector("#derived-access-status")?.classList.add("hidden");
  const follow = document.querySelector<HTMLButtonElement>("#follow-toggle");
  if (follow) {
    follow.classList.add("hidden");
    follow.onclick = () => actions.toggleTimelineMonitoring?.();
  }
  const switcher = document.querySelector<HTMLElement>("#lens-switcher");
  if (switcher) {
    switcher.replaceChildren();
    for (const lens of ["timeline", "changes", "attention"] as const) {
      const link = document.createElement("a");
      link.className = "lens-tab";
      link.dataset.lens = lens;
      link.textContent =
        lens === "timeline"
          ? "Timeline"
          : lens === "changes"
            ? "Changes"
            : "Attention";
      const destination = () => {
        const current = parseChangeInspectorRoute(
          location.hash || "#/timeline",
        );
        return routeForLens(lens, current);
      };
      link.href = formatChangeInspectorRoute(destination());
      link.addEventListener("click", (event) => {
        if (
          event.button !== 0 ||
          event.metaKey ||
          event.ctrlKey ||
          event.shiftKey ||
          event.altKey
        )
          return;
        event.preventDefault();
        actions.navigate(destination());
      });
      switcher.append(link);
    }
  }
  const back = document.querySelector<HTMLButtonElement>("#detail-back");
  if (back) {
    back.textContent = "‹ Timeline";
  }
  const search = document.querySelector<HTMLInputElement>("#filter-text");
  if (search)
    search.placeholder = "Search Timeline, Changes, and exact Revisions";
  const filterTypes = document.querySelector<HTMLElement>("#filter-types");
  if (filterTypes) {
    filterTypes.replaceChildren();
    const heading = document.createElement("h2");
    heading.id = "filter-types-label";
    heading.className = "control-heading change-filter";
    heading.textContent = "Change status";
    filterTypes.append(heading);
    const timelineHeading = document.createElement("h2");
    timelineHeading.className = "control-heading timeline-filter hidden";
    timelineHeading.textContent = "Event types";
    const timelineMenu = document.createElement("ul");
    timelineMenu.id = "filter-types-menu";
    timelineMenu.className = "type-facet-menu timeline-filter hidden";
    timelineMenu.setAttribute("aria-label", "event types");
    filterTypes.append(timelineHeading, timelineMenu);
    for (const [name, values] of FILTER_OPTIONS) {
      const label = document.createElement("label");
      label.className = "change-filter";
      label.textContent = name.replaceAll("_", " ");
      const select = document.createElement("select");
      select.id = `change-filter-${name}`;
      for (const [labelText, value] of [
        ["Any", ""],
        ...values.map((value) => [value.replaceAll("_", " "), value]),
      ]) {
        const option = document.createElement("option");
        option.textContent = labelText;
        option.value = value;
        select.append(option);
      }
      select.addEventListener("change", () => {
        const current = parseChangeInspectorRoute(
          location.hash || "#/timeline",
        );
        const base =
          current.kind === "invalid" ||
          current.kind === "timeline" ||
          current.kind === "event"
            ? { kind: "lens" as const, lens: "changes" as const, query: {} }
            : current;
        actions.navigate({
          ...base,
          query: {
            ...base.query,
            after: undefined,
            [name]: select.value || undefined,
          },
        } as Exclude<ChangeInspectorRoute, { kind: "invalid" }>);
      });
      label.append(select);
      filterTypes.append(label);
    }
    const timelineChoice = (
      label: string,
      id: string,
      update: (query: EventHistoryQuery) => void,
    ) => {
      const choiceLabel = document.createElement("label");
      choiceLabel.className = "timeline-filter hidden";
      choiceLabel.textContent = label;
      const select = document.createElement("select");
      select.id = id;
      select.append(selectOption("Any", ""));
      select.addEventListener("change", () => {
        const current = parseChangeInspectorRoute(
          location.hash || "#/timeline",
        );
        if (current.kind !== "timeline" && current.kind !== "event") return;
        const query: EventHistoryQuery = {
          ...current.historyQuery,
          after: undefined,
          at: undefined,
        };
        update(query);
        actions.navigate({ kind: "timeline", historyQuery: query });
      });
      choiceLabel.append(select);
      filterTypes.append(choiceLabel);
    };
    timelineChoice("track", "timeline-filter-track", (query) => {
      query.track =
        document.querySelector<HTMLSelectElement>("#timeline-filter-track")
          ?.value || undefined;
    });
    timelineChoice("Change", "timeline-filter-change", (query) => {
      query.change =
        document.querySelector<HTMLSelectElement>("#timeline-filter-change")
          ?.value || undefined;
    });
    timelineChoice("exact Revision", "timeline-filter-revision", (query) => {
      const select = document.querySelector<HTMLSelectElement>(
        "#timeline-filter-revision",
      );
      const selected = select?.selectedOptions[0];
      query.revision = selected?.dataset.revisionId;
      query.artifactHash = selected?.dataset.artifactHash;
    });
  }
  for (const input of document.querySelectorAll<HTMLInputElement>(
    "input[name='view-order']",
  )) {
    input.addEventListener("change", () => {
      if (!input.checked) return;
      const current = parseChangeInspectorRoute(location.hash || "#/timeline");
      if (current.kind !== "timeline" && current.kind !== "event") return;
      actions.navigate({
        kind: "timeline",
        historyQuery: {
          ...current.historyQuery,
          after: undefined,
          at: undefined,
          order: input.value === "asc" ? "asc" : "desc",
        },
      });
    });
  }
  const clear = document.querySelector<HTMLButtonElement>("#filter-clear");
  if (clear) {
    clear.onclick = () => {
      const current = parseChangeInspectorRoute(location.hash || "#/timeline");
      if (current.kind === "timeline" || current.kind === "event") {
        actions.navigate({
          kind: "timeline",
          historyQuery: {
            limit: current.historyQuery.limit,
            order: current.historyQuery.order,
          },
        });
        return;
      }
      const base =
        current.kind === "invalid"
          ? { kind: "lens" as const, lens: "changes" as const, query: {} }
          : current;
      actions.navigate({
        ...base,
        // Clear only reader filters and the now-invalid continuation. Keep the
        // bounded page shape in the URL so reset does not silently change the
        // caller's explicit limit or stable ordering contract.
        query: {
          limit: base.query.limit,
          order: base.query.order,
        },
      } as Exclude<ChangeInspectorRoute, { kind: "invalid" }>);
    };
  }
}

function copyExact(control: HTMLButtonElement, value: string): void {
  const original = control.textContent ?? "Copy link";
  const settle = (label: "Copied" | "Copy failed") => {
    control.textContent = label;
    window.setTimeout(() => {
      if (control.isConnected && control.textContent === label)
        control.textContent = original;
    }, 1_500);
  };
  try {
    const write = navigator.clipboard?.writeText(value);
    if (write === undefined) {
      settle("Copy failed");
      return;
    }
    void write.then(
      () => settle("Copied"),
      () => settle("Copy failed"),
    );
  } catch {
    settle("Copy failed");
  }
}

function clearError(): void {
  const banner = document.querySelector<HTMLElement>("#error");
  if (!banner) return;
  banner.textContent = "";
  banner.classList.add("hidden");
}

function filterValues(query: ChangePageQuery): Array<[string, string]> {
  const values: Array<[string, string]> = [];
  if (query.q) values.push(["search", query.q]);
  for (const [name] of FILTER_OPTIONS) {
    const value = query[name];
    if (value) values.push([name, value]);
  }
  return values;
}

function replaceTimelineSelectOptions(
  id: string,
  options: Array<{
    value: string;
    label: string;
    artifactHash?: string;
    revisionId?: string;
    title?: string;
    accessibleName?: string;
  }>,
  selected: string | undefined,
): void {
  const select = document.querySelector<HTMLSelectElement>(`#${id}`);
  if (!select) return;
  const key = JSON.stringify(options);
  if (select.dataset.timelineOptions !== key) {
    select.replaceChildren(selectOption("Any", ""));
    for (const option of options) {
      select.append(
        selectOption(
          option.label,
          option.value,
          option.artifactHash,
          option.revisionId,
          option.title,
          option.accessibleName,
        ),
      );
    }
    select.dataset.timelineOptions = key;
  }
  select.value = selected ?? "";
}

function syncTimelineTypeFacets(
  route: Extract<ChangeInspectorRoute, { kind: "timeline" | "event" }>,
  history: EventHistoryDocument,
  actions: ChangeInspectorRenderActions,
): void {
  const menu = document.querySelector<HTMLUListElement>("#filter-types-menu");
  if (!menu) return;
  const selected = new Set(
    route.historyQuery.type?.split(",").filter(Boolean) ?? [],
  );
  const rows = history.completion.eventTypes.map((eventType) => {
    const item = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    button.className = `type-facet-row${selected.has(eventType) ? "" : " type-facet-row-off"}`;
    button.dataset.eventType = eventType;
    button.setAttribute("aria-pressed", String(selected.has(eventType)));
    const dot = document.createElement("span");
    dot.className = "dot";
    dot.style.background = eventTypeColor(eventType);
    dot.setAttribute("aria-hidden", "true");
    const name = document.createElement("span");
    name.textContent = eventType.replaceAll("_", " ");
    const count = document.createElement("span");
    count.className = "type-count";
    count.textContent = String(history.facets[eventType] ?? 0);
    button.append(dot, name, count);
    button.addEventListener("click", () => {
      const nextTypes = new Set(selected);
      if (nextTypes.has(eventType)) nextTypes.delete(eventType);
      else nextTypes.add(eventType);
      actions.navigate({
        kind: "timeline",
        historyQuery: {
          ...route.historyQuery,
          after: undefined,
          at: undefined,
          type: nextTypes.size ? [...nextTypes].sort().join(",") : undefined,
        },
      });
    });
    item.append(button);
    return item;
  });
  if (!rows.length) {
    const empty = document.createElement("li");
    empty.className = "dim";
    empty.textContent = "No event types available";
    rows.push(empty);
  }
  menu.replaceChildren(...rows);
}

function syncFilterChrome(
  route: ChangeInspectorRoute,
  history: EventHistoryDocument | null,
  actions: ChangeInspectorRenderActions,
): void {
  if (route.kind === "invalid") return;
  if (route.kind === "timeline" || route.kind === "event") {
    const input = document.querySelector<HTMLInputElement>("#filter-text");
    if (input) input.value = route.historyQuery.q ?? "";
    const chips = document.querySelector<HTMLElement>("#filter-chips");
    const queryChips = filterChipsFor(
      route.historyQuery.q ?? "",
      "change-timeline",
    );
    const values = [
      ["type", route.historyQuery.type],
      ["track", route.historyQuery.track],
      ["change", route.historyQuery.change],
      ["revision", route.historyQuery.revision],
    ].filter((value): value is [string, string] => Boolean(value[1]));
    chips?.replaceChildren(
      ...queryChips.map((queryChip) => {
        const chip = document.createElement("button");
        chip.type = "button";
        chip.className = "badge";
        const field = `${queryChip.negate ? "-" : ""}${queryChip.field}`;
        chip.textContent = `${field}: ${compactIdentityText(queryChip.value)} ×`;
        chip.title = `${field}:${queryChip.value}`;
        chip.setAttribute(
          "aria-label",
          `Remove ${field} query clause: ${queryChip.value}`,
        );
        chip.addEventListener("click", () => {
          const q = removeFilterChipToken(
            route.historyQuery.q ?? "",
            queryChip.tokenIndex,
          );
          actions.replace({
            kind: "timeline",
            historyQuery: {
              ...route.historyQuery,
              after: undefined,
              at: undefined,
              q: q || undefined,
            },
          });
        });
        return chip;
      }),
      ...values.map(([name, value]) => {
        const chip = document.createElement("button");
        chip.type = "button";
        chip.className = "badge";
        const exactRevision =
          name === "revision" && route.historyQuery.artifactHash
            ? {
                revisionId: value,
                objectArtifactContentHash: route.historyQuery.artifactHash,
              }
            : null;
        const visible = exactRevision
          ? shortExactRevision(exactRevision)
          : compactIdentityText(value);
        const full = exactRevision
          ? `${value} · ${exactRevision.objectArtifactContentHash}`
          : value;
        chip.textContent = `${name}: ${visible} ×`;
        chip.title = full;
        chip.setAttribute(
          "aria-label",
          exactRevision
            ? `Remove revision filter: ${value}; artifact ${exactRevision.objectArtifactContentHash}`
            : `Remove ${name} filter: ${value}`,
        );
        chip.addEventListener("click", () => {
          const next = {
            ...route.historyQuery,
            after: undefined,
            at: undefined,
            [name === "search" ? "q" : name]: undefined,
          };
          if (name === "revision") next.artifactHash = undefined;
          actions.navigate({ kind: "timeline", historyQuery: next });
        });
        return chip;
      }),
    );
    document
      .querySelector<HTMLElement>("#filter-chips-empty")
      ?.classList.toggle("hidden", queryChips.length + values.length > 0);
    const toggle = document.querySelector<HTMLElement>("#filters-toggle");
    const filterCount = queryChips.length + values.length;
    if (toggle)
      toggle.textContent = filterCount ? `Filters · ${filterCount}` : "Filters";
    document
      .querySelectorAll<HTMLElement>(".timeline-filter")
      .forEach((element) => {
        element.classList.remove("hidden");
      });
    document
      .querySelectorAll<HTMLElement>(".change-filter")
      .forEach((element) => {
        element.classList.add("hidden");
      });
    if (history !== null) {
      syncTimelineTypeFacets(route, history, actions);
      replaceTimelineSelectOptions(
        "timeline-filter-track",
        history.completion.trackIds.map((trackId) => ({
          value: trackId,
          label: trackId,
        })),
        route.historyQuery.track,
      );
      replaceTimelineSelectOptions(
        "timeline-filter-change",
        history.completion.changeIds.map((changeId) => ({
          value: changeId,
          label: shortRef(changeId),
          title: changeId,
          accessibleName: `Change ${changeId}`,
        })),
        route.historyQuery.change,
      );
      replaceTimelineSelectOptions(
        "timeline-filter-revision",
        history.completion.revisionRefs.map((revision) => ({
          value: exactRevisionOptionValue(
            revision.revisionId,
            revision.objectArtifactContentHash,
          ),
          label: shortExactRevision(revision),
          artifactHash: revision.objectArtifactContentHash,
          revisionId: revision.revisionId,
          title: exactRevisionAccessibleIdentity(revision),
          accessibleName: exactRevisionAccessibleIdentity(revision),
        })),
        route.historyQuery.revision && route.historyQuery.artifactHash
          ? exactRevisionOptionValue(
              route.historyQuery.revision,
              route.historyQuery.artifactHash,
            )
          : undefined,
      );
    }
    document.querySelector("#view-order-section")?.classList.remove("hidden");
    const newest = document.querySelector<HTMLInputElement>("#order-newest");
    const oldest = document.querySelector<HTMLInputElement>("#order-oldest");
    if (newest) newest.checked = route.historyQuery.order !== "asc";
    if (oldest) oldest.checked = route.historyQuery.order === "asc";
    return;
  }
  document
    .querySelectorAll<HTMLElement>(".timeline-filter")
    .forEach((element) => {
      element.classList.add("hidden");
    });
  document
    .querySelectorAll<HTMLElement>(".change-filter")
    .forEach((element) => {
      element.classList.remove("hidden");
    });
  document.querySelector("#view-order-section")?.classList.add("hidden");
  const input = document.querySelector<HTMLInputElement>("#filter-text");
  if (input) input.value = route.query.q ?? "";
  for (const [name] of FILTER_OPTIONS) {
    const select = document.querySelector<HTMLSelectElement>(
      `#change-filter-${name}`,
    );
    if (select) select.value = route.query[name] ?? "";
  }
  const values = filterValues(route.query);
  const chips = document.querySelector<HTMLElement>("#filter-chips");
  if (chips) {
    chips.replaceChildren(
      ...values.map(([name, value]) => {
        const chip = document.createElement("span");
        chip.className = "badge";
        chip.textContent = `${name}: ${value.replaceAll("_", " ")}`;
        return chip;
      }),
    );
  }
  document
    .querySelector<HTMLElement>("#filter-chips-empty")
    ?.classList.toggle("hidden", values.length > 0);
  const toggle = document.querySelector<HTMLElement>("#filters-toggle");
  if (toggle)
    toggle.textContent = values.length
      ? `Filters · ${values.length}`
      : "Filters";
}

function appendDefinition(
  list: HTMLDListElement,
  label: string,
  value: string,
  accessibleName = value,
): HTMLElement {
  const term = document.createElement("dt");
  term.textContent = label;
  const definition = document.createElement("dd");
  setCompactIdentityText(definition, value, accessibleName);
  list.append(term, definition);
  return definition;
}

function renderEventDetail(
  event: EventHistoryEntry,
  actions: ChangeInspectorRenderActions,
): Node[] {
  const presentation = presentEvent(event);
  const heading = detailHeading("Event");
  const identity = detailLine(event.eventId, "mono");
  identity.title = event.eventId;
  identity.setAttribute("aria-label", `event ${event.eventId}`);
  identity.dataset.eventId = event.eventId;
  const summary = document.createElement("section");
  summary.className = "event-detail-summary";
  summary.append(detailHeading(presentation.title, 3));
  if (presentation.body) summary.append(detailLine(presentation.body));
  const summaryFacts = document.createElement("dl");
  summaryFacts.className = "kv";
  for (const item of presentation.fields) {
    appendDefinition(summaryFacts, item.label, item.value);
  }
  if (presentation.fields.length) summary.append(summaryFacts);

  const attribution = document.createElement("section");
  attribution.className = "event-detail-attribution";
  attribution.append(detailHeading("Subject and attribution", 3));
  const attributionFacts = document.createElement("dl");
  attributionFacts.className = "kv";
  const attributionAdd = (name: string, value: string) =>
    appendDefinition(attributionFacts, name, value);
  attributionAdd("subject", eventSubjectLabel(event.subject));
  attributionAdd("writer", event.writer.actorId);
  attributionAdd(
    "producer",
    `${event.writer.producer.name} ${event.writer.producer.version}`,
  );
  attributionAdd("assertion", event.assertionMode.replaceAll("_", " "));
  if (event.signer) attributionAdd("signer", event.signer);
  if (event.sourceRef) {
    attributionAdd(
      "source",
      `${event.sourceRef.sourceSystem} · ${event.sourceRef.sourceId}`,
    );
  }
  if (event.ingest) {
    attributionAdd(
      "ingest",
      `${event.ingest.via} · ${event.ingest.receivedAt}`,
    );
  }
  attribution.append(attributionFacts);

  const record = document.createElement("section");
  record.className = "event-detail-record";
  record.append(detailHeading("Event record", 3));
  const facts = document.createElement("dl");
  facts.className = "kv";
  const add = (name: string, value: string, accessibleName = value) =>
    appendDefinition(facts, name, value, accessibleName);
  add("type", event.eventType.replaceAll("_", " "));
  add("occurred", event.occurredAt);
  add("verification", event.verificationStatus.replaceAll("_", " "));
  const payload = add(
    "event payload",
    event.payloadHash,
    `artifact ${event.payloadHash}`,
  );
  payload.dataset.artifactHash = event.payloadHash;
  const journal = add("journal", event.journalId, `journal ${event.journalId}`);
  journal.dataset.journalId = event.journalId;
  if (event.trackId) add("track", event.trackId);
  if (event.signer) add("signer", event.signer);
  add("Changes", event.changeIds.join("; ") || "none");
  add(
    "exact Revisions",
    event.revisionRefs
      .map(
        (revision) =>
          `${revision.revisionId} · ${revision.objectArtifactContentHash}`,
      )
      .join("; ") || "none",
  );
  if (event.unresolvedRevisionIds.length)
    add("unresolved Revisions", event.unresolvedRevisionIds.join("; "));
  record.append(facts);

  const context = document.createElement("section");
  context.className = "actions event-detail-actions";
  const onlyChange = event.changeIds.length === 1 ? event.changeIds[0] : null;
  const annotatedDiff = eventAnnotatedDiffRoute(event);
  if (annotatedDiff) {
    const activation = document.createElement("button");
    activation.type = "button";
    activation.className = "ghost detail-action-primary";
    activation.dataset.exactDiffActivation = "true";
    activation.textContent = "Open annotated diff";
    activation.addEventListener("click", () => actions.navigate(annotatedDiff));
    context.append(activation);
  }
  for (const changeId of event.changeIds) {
    const change = document.createElement("button");
    change.type = "button";
    change.className = "ghost";
    change.dataset.eventChangeChoice = changeId;
    change.dataset.changeId = changeId;
    change.textContent =
      event.changeIds.length === 1
        ? "Open Change"
        : `Open Change ${shortRef(changeId)}`;
    change.title = `Change ${changeId}`;
    change.setAttribute("aria-label", `Open Change ${changeId}`);
    change.addEventListener("click", () =>
      actions.navigate({ kind: "change", changeId, query: {} }),
    );
    context.append(change);
  }
  if (onlyChange) {
    for (const exactRevision of event.revisionRefs) {
      const revision = document.createElement("button");
      revision.type = "button";
      revision.className = "ghost";
      revision.dataset.eventRevisionChoice = formatChangeInspectorRoute({
        kind: "revision",
        changeId: onlyChange,
        revision: exactRevision,
        query: {},
      });
      revision.textContent =
        event.revisionRefs.length === 1
          ? "Open exact Revision"
          : `Open exact Revision ${shortExact(exactRevision)}`;
      revision.title = exactRevisionAccessibleIdentity(exactRevision);
      revision.setAttribute(
        "aria-label",
        `Open ${exactRevisionAccessibleIdentity(exactRevision)} for Change ${onlyChange}`,
      );
      revision.dataset.changeId = onlyChange;
      revision.dataset.revisionId = exactRevision.revisionId;
      revision.dataset.artifactHash = exactRevision.objectArtifactContentHash;
      revision.addEventListener("click", () =>
        actions.navigate({
          kind: "revision",
          changeId: onlyChange,
          revision: exactRevision,
          query: {},
        }),
      );
      context.append(revision);
    }
  }
  if (!annotatedDiff) {
    const refusal = detailLine(
      "Opening an annotated diff requires exactly one Change and one exact Revision, with no unresolved Revisions. Choose an explicit context where available.",
      "dim",
    );
    refusal.dataset.eventDiffRefusal = "true";
    refusal.setAttribute("role", "status");
    refusal.tabIndex = -1;
    context.append(refusal);
  }
  const copyLink = document.createElement("button");
  copyLink.type = "button";
  copyLink.className = "ghost";
  copyLink.textContent = "Copy link";
  copyLink.addEventListener("click", () => copyExact(copyLink, location.href));
  context.append(copyLink);

  const structured = document.createElement("details");
  structured.className = "event-structured";
  const structuredLabel = document.createElement("summary");
  structuredLabel.textContent = "Structured event data";
  const raw = document.createElement("pre");
  raw.className = "anno-body mono";
  raw.textContent = JSON.stringify(
    {
      summary: event.summary,
      subject: event.subject,
      writer: event.writer,
      sourceRef: event.sourceRef,
      ingest: event.ingest,
    },
    null,
    2,
  );
  structured.append(structuredLabel, raw);
  return [heading, identity, summary, attribution, record, context, structured];
}

function detailHeading(text: string, level = 2): HTMLHeadingElement {
  const heading = document.createElement(`h${level}`) as HTMLHeadingElement;
  setCompactIdentityText(heading, text);
  return heading;
}

function detailLine(text: string, className?: string): HTMLParagraphElement {
  const line = document.createElement("p");
  if (className) line.className = className;
  setCompactIdentityText(line, text);
  return line;
}

function detailState(
  entries: Array<[label: string, value: string]>,
): HTMLDListElement {
  const list = document.createElement("dl");
  list.className = "detail-state";
  for (const [label, value] of entries) {
    const term = document.createElement("dt");
    term.textContent = label;
    const description = document.createElement("dd");
    setCompactIdentityText(description, value);
    list.append(term, description);
  }
  return list;
}

function detailIdentity(revision: RevisionRef): HTMLParagraphElement {
  const line = document.createElement("p");
  line.className = "detail-identity";
  line.append(exactRevisionIdentity(revision));
  return line;
}

function detailActions(...controls: HTMLElement[]): HTMLElement {
  const actions = document.createElement("div");
  actions.className = "detail-actions";
  actions.append(...controls);
  return actions;
}

function showChangeInTimeline(changeId: string): HTMLAnchorElement {
  const link = document.createElement("a");
  link.className = "ghost";
  link.textContent = "Show in Timeline";
  link.href = formatChangeInspectorRoute(showChangeInTimelineRoute(changeId));
  link.setAttribute("aria-label", `Show Change ${changeId} in Timeline`);
  return link;
}

function showRevisionInTimeline(
  changeId: string,
  revision: RevisionRef,
): HTMLAnchorElement {
  const link = document.createElement("a");
  link.className = "ghost";
  link.textContent = "Show in Timeline";
  link.href = formatChangeInspectorRoute(
    showRevisionInTimelineRoute(changeId, revision),
  );
  link.setAttribute(
    "aria-label",
    `Show exact Revision ${revision.revisionId} with artifact ${revision.objectArtifactContentHash} for Change ${changeId} in Timeline`,
  );
  return link;
}

function shortExact(revision: {
  revisionId: string;
  objectArtifactContentHash: string;
}): string {
  return shortExactRevision(revision);
}

function exactRevisionText(revision: {
  revisionId: string;
  objectArtifactContentHash: string;
}): string {
  return `${revision.revisionId} · ${revision.objectArtifactContentHash}`;
}

function exactRevisionIdentity(
  revision: RevisionRef,
  className = "mono",
): HTMLElement {
  const identity = document.createElement("code");
  identity.className = className;
  identity.textContent = shortExact(revision);
  identity.title = exactRevisionAccessibleIdentity(revision);
  identity.setAttribute(
    "aria-label",
    exactRevisionAccessibleIdentity(revision),
  );
  identity.dataset.revisionId = revision.revisionId;
  identity.dataset.artifactHash = revision.objectArtifactContentHash;
  return identity;
}

function renderedFactBody(
  content: FactContent,
  contentType: "text/plain" | "text/markdown",
): HTMLElement {
  const body = document.createElement("div");
  body.className = "anno-body";
  const text =
    content.kind === "observation" || content.kind === "input_request"
      ? content.body
      : content.kind === "assessment" || content.kind === "validation"
        ? content.summary
        : undefined;
  // Fact prose is supplied by the server after exact contextual validation.
  // This uses the retained pure Markdown renderer, not the retired Inspector
  // composition, so exact contextual selection remains the only reader state.
  if (text) body.innerHTML = renderBodyContent(text, contentType);
  return body;
}

function renderFacts(
  reading:
    | Extract<ChangeInspectorReading, { kind: "revision" }>
    | Extract<ChangeInspectorReading, { kind: "association" }>,
  route: Extract<ChangeInspectorRoute, { kind: "revision" | "association" }>,
  actions: ChangeInspectorRenderActions,
): HTMLElement {
  const facts = document.createElement("section");
  facts.className = "detail-facts";
  facts.append(detailHeading("Facts", 3));
  const groups = new Map<string, typeof reading.document.factPresentations>();
  for (const fact of reading.document.factPresentations) {
    const family = groups.get(fact.family) ?? [];
    family.push(fact);
    groups.set(fact.family, family);
  }
  for (const [family, items] of groups) {
    const group = document.createElement("section");
    group.append(detailHeading(family.replaceAll("_", " "), 4));
    for (const fact of items) {
      const card = document.createElement("article");
      card.className = "unit-card";
      card.dataset.factId = fact.factId;
      card.tabIndex = -1;
      const content = reading.document.factContentPresentations?.[fact.factId];
      if (content) {
        const heading =
          content.content.kind === "assessment"
            ? `Assessment: ${content.content.assessment}`
            : content.content.kind === "validation"
              ? content.content.checkName
              : content.content.title;
        card.append(detailHeading(heading, 5));
      }
      const factIdentity = document.createElement("p");
      factIdentity.className = "detail-fact-identity";
      const factCode = document.createElement("code");
      factCode.textContent = shortRef(fact.factId);
      factCode.title = fact.factId;
      factCode.setAttribute("aria-label", `${fact.family} ${fact.factId}`);
      factIdentity.append(factCode);
      card.append(
        factIdentity,
        detailLine(
          `origin: ${exactRevisionText(fact.originRevision)} · context: ${fact.contextChangeId ?? "unavailable"} · currency: ${fact.revisionCurrency.replaceAll("_", " ")}`,
        ),
        detailLine(
          `family: ${fact.familyState.replaceAll("_", " ")} · availability: ${fact.availability.replaceAll("_", " ")} · actor: ${fact.actorId}${fact.trackId ? ` · track: ${fact.trackId}` : ""}`,
        ),
      );
      const presentedInRevision = fact.presentedInRevision;
      if (presentedInRevision) {
        const applicablePort = reading.document.factPorts.find(
          (port) =>
            port.applicability === "applicable" &&
            port.originRevision.revisionId === fact.originRevision.revisionId &&
            port.originRevision.objectArtifactContentHash ===
              fact.originRevision.objectArtifactContentHash &&
            port.targetRevision.revisionId === presentedInRevision.revisionId &&
            port.targetRevision.objectArtifactContentHash ===
              presentedInRevision.objectArtifactContentHash &&
            factRefLabel(port.originFact) === factRefLabelFromFactId(fact),
        );
        card.append(
          detailLine(
            `presented in: ${exactRevisionText(presentedInRevision)} · port: ${fact.portRelation?.replaceAll("_", " ") ?? (applicablePort ? `${applicablePort.relation.replaceAll("_", " ")} (${applicablePort.portId})` : "see Fact ports")}`,
          ),
        );
      }
      if (content) {
        card.append(
          detailLine(
            `body: ${content.bodyContentState.replaceAll("_", " ")} · ${content.contentType}`,
          ),
          renderedFactBody(content.content, content.contentType),
        );
      }
      const focus = document.createElement("button");
      focus.type = "button";
      focus.className = "ghost";
      focus.textContent = "Focus fact";
      focus.addEventListener("click", () =>
        actions.navigate({
          kind: route.kind,
          changeId: route.changeId,
          revision: route.revision,
          query: queryForExactNavigation(route),
          focus: { factId: fact.factId },
        }),
      );
      card.append(focus);
      group.append(card);
    }
    facts.append(group);
  }
  if (groups.size === 0) facts.append(message("No facts."));
  return facts;
}

function factRefLabel(fact: {
  kind: string;
  observationId?: string;
  inputRequestId?: string;
}): string {
  return fact.kind === "observation"
    ? `observation: ${fact.observationId}`
    : `input request: ${fact.inputRequestId}`;
}

function factRefLabelFromFactId(fact: {
  family: string;
  factId: string;
}): string {
  return fact.family === "observation"
    ? `observation: ${fact.factId}`
    : fact.family === "input_request"
      ? `input request: ${fact.factId}`
      : "";
}

function renderFactPorts(
  reading:
    | Extract<ChangeInspectorReading, { kind: "revision" }>
    | Extract<ChangeInspectorReading, { kind: "association" }>,
): HTMLElement {
  const ports = document.createElement("section");
  ports.append(detailHeading("Fact ports", 3));
  for (const port of reading.document.factPorts) {
    const item = document.createElement("article");
    item.className = "unit-card";
    item.append(
      detailLine(port.portId, "mono"),
      detailLine(
        `origin: ${factRefLabel(port.originFact)} · ${exactRevisionText(port.originRevision)}`,
      ),
      detailLine(
        `target: ${exactRevisionText(port.targetRevision)} · ${port.relation.replaceAll("_", " ")}`,
      ),
      detailLine(
        `target fact: ${port.targetFact ? factRefLabel(port.targetFact) : "none"} · applicability: ${port.applicability.replaceAll("_", " ")}`,
      ),
      detailLine(
        `actor: ${port.actorId}${port.trackId ? ` · track: ${port.trackId}` : ""}${port.contextChangeId ? ` · context: ${port.contextChangeId}` : ""}`,
      ),
    );
    if (port.rationaleContentHash)
      item.append(
        detailLine(`rationale: ${port.rationaleContentHash}`, "mono"),
      );
    for (const diagnostic of port.diagnostics)
      item.append(detailLine(diagnostic));
    ports.append(item);
  }
  if (reading.document.factPorts.length === 0)
    ports.append(message("No fact ports."));
  return ports;
}

function openCapturedResource(
  route: Extract<ChangeInspectorRoute, { kind: "revision" | "association" }>,
  actions: ChangeInspectorRenderActions,
): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "ghost";
  button.textContent = "Open authoritative captured diff";
  button.addEventListener("click", () =>
    actions.navigate({
      kind: "resource",
      changeId: route.changeId,
      revision: route.revision,
      query: route.query,
      ...(route.focus ? { focus: route.focus } : {}),
    }),
  );
  return button;
}

function openAnnotatedDiff(
  route: Extract<ChangeInspectorRoute, { kind: "revision" | "association" }>,
  actions: ChangeInspectorRenderActions,
): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "ghost detail-action-primary";
  button.dataset.exactDiffActivation = "true";
  button.textContent = "Open annotated diff";
  button.addEventListener("click", () =>
    actions.navigate({
      kind: "diff",
      changeId: route.changeId,
      revision: route.revision,
      query: route.query,
      ...(route.focus ? { focus: route.focus } : {}),
    }),
  );
  return button;
}

function renderAssociations(
  reading: Extract<
    ChangeInspectorReading,
    { kind: "revision" | "association" }
  >,
  route: Extract<ChangeInspectorRoute, { kind: "revision" | "association" }>,
  actions: ChangeInspectorRenderActions,
): HTMLElement {
  const section = document.createElement("section");
  section.append(detailHeading("Association comparisons", 3));
  for (const association of reading.document.associations) {
    const item = document.createElement("article");
    item.className = "unit-card";
    item.append(
      detailLine(association.comparison.associationId, "mono"),
      detailLine(
        `commit: ${association.comparison.commitOid} · base: ${association.comparison.comparisonBase}`,
      ),
      detailLine(
        `view: ${association.comparison.viewKind} · state: ${association.state} · proof: ${association.proofAvailability}`,
      ),
    );
    if (association.comparison.proofRef)
      item.append(
        detailLine(`proof: ${association.comparison.proofRef}`, "mono"),
      );
    for (const diagnostic of association.diagnostics)
      item.append(detailLine(diagnostic));
    item.append(openCapturedResource(route, actions));
    section.append(item);
  }
  if (reading.document.associations.length === 0)
    section.append(message("No association comparisons."));
  return section;
}

function capturedDiffArtifact(documentValue: unknown): DiffArtifact | null {
  if (typeof documentValue !== "object" || documentValue === null) return null;
  const documentRecord = documentValue as Record<string, unknown>;
  const snapshot = documentRecord.snapshot;
  if (typeof snapshot !== "object" || snapshot === null) return null;
  const files = (snapshot as Record<string, unknown>).files;
  return Array.isArray(files) ? (documentValue as DiffArtifact) : null;
}

function bindCapturedDiffInteractions(
  diff: HTMLElement,
  rendered: ReturnType<typeof renderDiff>,
): void {
  rendered.ctx.files.forEach((file, index) => {
    const section = diff.querySelector<HTMLElement>(`[data-dfile="${index}"]`);
    const header = section?.querySelector<HTMLElement>(".dfile-head");
    const body = section?.querySelector<HTMLElement>(
      `[data-dfile-body="${index}"]`,
    );
    const renderBody = () => {
      if (!section || !body) return;
      body.innerHTML = renderDiffFileBody(file, rendered.ctx.anchored);
      body.dataset.rendered = "1";
      section.dataset.expanded = "true";
    };
    const toggle = () => {
      if (!section || !body) return;
      if (section.dataset.expanded === "true") {
        section.dataset.expanded = "false";
        return;
      }
      renderBody();
    };
    header?.addEventListener("click", toggle);
    header?.addEventListener("keydown", (event) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      toggle();
    });
    body?.addEventListener("click", (event) => {
      const trigger = (event.target as Element | null)?.closest<HTMLElement>(
        "[data-render-diff-file]",
      );
      if (!trigger) return;
      event.preventDefault();
      renderBody();
    });
  });
}

function renderCapturedDiff(resource: RevisionResource): HTMLElement {
  const artifact = capturedDiffArtifact(resource.capturedDocument);
  if (artifact === null) {
    const refusal = document.createElement("section");
    refusal.className = "detail-facts";
    refusal.append(
      message(
        "This exact resource does not contain a captured snapshot. The Inspector will not reconstruct a diff from Git or an associated commit.",
      ),
    );
    return refusal;
  }
  const diff = document.createElement("section");
  diff.className = "captured-diff";
  const rendered = renderDiff(resource.resource.objectId, artifact, []);
  diff.innerHTML = rendered.html;
  rendered.ctx.files.forEach((file, index) => {
    const section = diff.querySelector<HTMLElement>(`[data-dfile="${index}"]`);
    const path = file.new_path ?? file.old_path;
    if (section && path) {
      section.dataset.filePath = path;
      if (file.old_path) section.dataset.oldFilePath = file.old_path;
      if (file.new_path) section.dataset.newFilePath = file.new_path;
      section.tabIndex = -1;
    }
  });
  bindCapturedDiffInteractions(diff, rendered);
  return diff;
}

function renderCapturedResource(
  resource: RevisionResource,
  route: Extract<ChangeInspectorRoute, { kind: "resource" }>,
  actions: ChangeInspectorRenderActions,
): Node[] {
  const nodes: Node[] = [
    detailHeading("Authoritative captured diff"),
    detailLine(exactRevisionText(resource.resource.revision), "mono"),
    detailLine(`availability: ${resource.availability.replaceAll("_", " ")}`),
  ];
  if (resource.availability !== "available") {
    nodes.push(
      message(
        "Captured bytes are unavailable. No live or associated-commit bytes were substituted.",
      ),
    );
  } else {
    nodes.push(
      detailLine(`captured document: ${resource.capturedDocumentHash}`, "mono"),
    );
    nodes.push(renderCapturedDiff(resource));
  }
  for (const diagnostic of resource.diagnostics)
    nodes.push(detailLine(diagnostic));
  const back = document.createElement("button");
  back.type = "button";
  back.className = "ghost";
  back.textContent = "Back to exact Revision";
  back.addEventListener("click", () =>
    actions.navigate({
      kind: "revision",
      changeId: route.changeId,
      revision: route.revision,
      query: route.query,
      ...(route.focus ? { focus: route.focus } : {}),
    }),
  );
  nodes.push(back);
  return nodes;
}

function renderCurrentRevisionChoices(
  changeId: string,
  revisions: RevisionRef[],
  query: ChangePageQuery,
  actions: ChangeInspectorRenderActions,
): HTMLElement {
  const choices = document.createElement("section");
  choices.className = "detail-current-revisions";
  choices.append(detailHeading("Current Revisions", 3));
  if (revisions.length === 0) {
    choices.append(message("No current Revision is available."));
    return choices;
  }
  for (const revision of revisions) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ghost mono";
    button.textContent = shortExact(revision);
    button.title = exactRevisionAccessibleIdentity(revision);
    button.setAttribute(
      "aria-label",
      `Current Revision: open ${exactRevisionAccessibleIdentity(revision)}; for Change ${changeId}`,
    );
    button.dataset.changeId = changeId;
    button.dataset.revisionId = revision.revisionId;
    button.dataset.artifactHash = revision.objectArtifactContentHash;
    button.addEventListener("click", () =>
      actions.navigate({
        kind: "revision",
        changeId,
        revision,
        query,
      }),
    );
    choices.append(button);
  }
  return choices;
}

function renderChangeRelationshipGraph(
  detail: ChangeDetail,
  route: Extract<ChangeInspectorRoute, { kind: "change" }>,
  actions: ChangeInspectorRenderActions,
): HTMLElement | null {
  const graph = detail.inspectorPresentation?.revisionGraph;
  if (!graph) return null;
  const section = document.createElement("section");
  section.className = "detail-relationships";
  section.append(
    detailHeading("Revision relationships", 3),
    renderChangeRevisionGraph(graph, {
      document,
      onActivateRevision: (revision) =>
        actions.navigate({
          kind: "revision",
          changeId: route.changeId,
          revision,
          query: route.query,
        }),
    }),
  );
  return section;
}

function renderExactFactRelationshipGraph(
  detail: Extract<ChangeInspectorReading, { kind: "revision" }>["document"],
  route: Extract<ChangeInspectorRoute, { kind: "revision" }>,
  actions: ChangeInspectorRenderActions,
): HTMLElement | null {
  const graph = detail.inspectorPresentation?.factGraph;
  if (!graph) return null;
  const section = document.createElement("section");
  section.className = "detail-relationships";
  section.append(
    detailHeading("Fact relationships", 3),
    renderFactRelationshipGraph(graph, {
      document,
      onActivateRevision: (revision) =>
        actions.navigate({
          kind: "revision",
          changeId: route.changeId,
          revision,
          query: queryForExactNavigation(route),
        }),
      onFocusFact: (focus) =>
        actions.navigate({
          kind: "revision",
          changeId: route.changeId,
          revision: focus.revision,
          query: queryForExactNavigation(route),
          focus: { factId: focus.factId },
        }),
    }),
  );
  return section;
}

function renderChangeDetail(
  detail: ChangeDetail,
  route: Extract<ChangeInspectorRoute, { kind: "change" }>,
  actions: ChangeInspectorRenderActions,
): Node[] {
  const changeIdentity = document.createElement("p");
  changeIdentity.className = "detail-identity";
  const changeCode = document.createElement("code");
  changeCode.textContent = shortRef(detail.summary.changeId);
  changeCode.title = detail.summary.changeId;
  changeCode.setAttribute("aria-label", `Change ${detail.summary.changeId}`);
  changeIdentity.append(changeCode);
  const nodes: Node[] = [
    detailHeading(
      detail.summary.titleAssertions.length === 1
        ? detail.summary.titleAssertions[0]
        : "Change",
    ),
    changeIdentity,
    detailState([
      ["Topology", detail.summary.topology.replaceAll("_", " ")],
      ["Lifecycle", detail.summary.lifecycle.replaceAll("_", " ")],
      ["Attention", detail.summary.attentionSummary.replaceAll("_", " ")],
      ["Availability", detail.summary.availabilitySummary.replaceAll("_", " ")],
      ["Members", String(detail.summary.memberCount)],
      ["Current peers", String(detail.currentRevisionRefs.length)],
    ]),
    detailActions(showChangeInTimeline(route.changeId)),
    renderCurrentRevisionChoices(
      route.changeId,
      detail.currentRevisionRefs,
      route.query,
      actions,
    ),
  ];
  const relationships = renderChangeRelationshipGraph(detail, route, actions);
  if (relationships) nodes.push(relationships);
  if (detail.summary.titleAssertions.length > 1) {
    const titles = document.createElement("section");
    titles.className = "detail-notice";
    titles.append(detailHeading("Title assertions", 3));
    for (const title of detail.summary.titleAssertions)
      titles.append(detailLine(title));
    nodes.push(titles);
  }
  if (detail.operativeObligations.length > 0) {
    const obligations = document.createElement("section");
    obligations.className = "detail-notice detail-notice-warning";
    obligations.append(detailHeading("What needs attention", 3));
    for (const obligation of detail.operativeObligations)
      obligations.append(detailLine(obligation));
    nodes.push(obligations);
  }
  const sections: Array<[string, string[]]> = [
    [
      "Member Revisions",
      detail.memberRevisions.map(
        (member) =>
          `${exactRevisionText(member.revision)} · support: ${member.supportingClaimIds.join(", ") || "none"}`,
      ),
    ],
    [
      "Unavailable Members",
      detail.unavailableMemberRevisions.map(
        (member) =>
          `${member.revisionId} · ${member.reason.replaceAll("_", " ")} · support: ${member.supportingClaimIds.join(", ") || "none"}`,
      ),
    ],
    [
      "Membership Claims",
      detail.membershipClaims.map(
        (claim) =>
          `${claim.claimId} · revision: ${claim.revisionId} · ${claim.active ? "active" : "inactive"} · supports: ${claim.supports.length} · withdrawals: ${claim.withdrawals.length}`,
      ),
    ],
    [
      "Membership Withdrawals",
      detail.membershipWithdrawals.map(
        (withdrawal) =>
          `${withdrawal.claimId} · support carriers: ${withdrawal.supports.length}`,
      ),
    ],
    [
      "Revision Relation Claims",
      detail.relationClaims.map(
        (claim) =>
          `${claim.claimId} · ${exactRevisionText(claim.predecessor)} → ${exactRevisionText(claim.successor)} · ${claim.active ? "active" : "inactive"}`,
      ),
    ],
    [
      "Relation Withdrawals",
      detail.relationWithdrawals.map(
        (withdrawal) =>
          `${withdrawal.claimId} · support carriers: ${withdrawal.supports.length}`,
      ),
    ],
    [
      "Effective Supersedes",
      detail.effectiveSupersedes.map(
        ([successor, predecessor]) =>
          `${exactRevisionText(predecessor)} → ${exactRevisionText(successor)}`,
      ),
    ],
    [
      "Pending or Conflicting Edges",
      detail.pendingOrConflictingEdges.map(
        (claim) =>
          `${claim.claimId} · ${exactRevisionText(claim.predecessor)} → ${exactRevisionText(claim.successor)} · ${claim.active ? "active" : "inactive"}`,
      ),
    ],
    [
      "Change Links",
      detail.links.map(
        (link) =>
          `${link.leftChangeId} · ${link.relation.replaceAll("_", " ")} · ${link.rightChangeId}`,
      ),
    ],
    [
      "Current Revision Qualification",
      detail.perCurrentRevisionQualification.map(
        (qualification) =>
          `${exactRevisionText(qualification.revision)} · ${qualification.qualified ? "qualified" : "not qualified"}`,
      ),
    ],
    ["Diagnostics", detail.diagnostics],
  ];
  const record = document.createElement("details");
  record.className = "detail-record";
  const recordLabel = document.createElement("summary");
  recordLabel.textContent = "Recorded claims and diagnostics";
  record.append(recordLabel);
  for (const [title, entries] of sections) {
    const section = document.createElement("section");
    section.append(detailHeading(title, 3));
    if (entries.length === 0) section.append(message("None."));
    for (const entry of entries) section.append(detailLine(entry));
    record.append(section);
  }
  nodes.push(record);
  return nodes;
}

function renderReading(
  reading: ChangeInspectorReading,
  snapshot: ChangeInspectorSnapshot,
  actions: ChangeInspectorRenderActions,
): Node[] {
  const route = snapshot.route;
  const copy = document.createElement("button");
  copy.type = "button";
  copy.className = "ghost";
  copy.textContent = "Copy link";
  copy.addEventListener("click", () => copyExact(copy, location.href));
  if (reading.kind === "change" && route.kind === "change") {
    return [...renderChangeDetail(reading.document, route, actions), copy];
  }
  if (reading.kind === "resource" && route.kind === "resource")
    return [...renderCapturedResource(reading.document, route, actions), copy];
  if (reading.kind === "interdiff" && route.kind === "interdiff") {
    const nodes: Node[] = [
      detailHeading("Ordered Revision interdiff"),
      detailLine(
        `${exactRevisionText(reading.document.interdiff.from)} → ${exactRevisionText(reading.document.interdiff.to)}`,
        "mono",
      ),
      detailLine(
        `availability: ${reading.document.availability.replaceAll("_", " ")} · algorithm: ${reading.document.interdiff.algorithmVersion}`,
      ),
      detailLine("This is a comparison, not the authoritative captured diff."),
    ];
    if (reading.document.comparison !== undefined) {
      const comparison = document.createElement("pre");
      comparison.textContent = JSON.stringify(
        reading.document.comparison,
        null,
        2,
      );
      nodes.push(comparison);
    }
    for (const diagnostic of reading.document.diagnostics)
      nodes.push(detailLine(diagnostic));
    for (const revision of [route.from, route.to]) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "ghost";
      button.textContent = `Open authoritative captured diff: ${shortExact(revision)}`;
      button.title = exactRevisionAccessibleIdentity(revision);
      button.setAttribute(
        "aria-label",
        `Open authoritative captured diff for ${exactRevisionAccessibleIdentity(revision)}; Change ${route.changeId}`,
      );
      button.dataset.changeId = route.changeId;
      button.dataset.revisionId = revision.revisionId;
      button.dataset.artifactHash = revision.objectArtifactContentHash;
      button.addEventListener("click", () =>
        actions.navigate({
          kind: "resource",
          changeId: route.changeId,
          revision,
          query: route.query,
        }),
      );
      nodes.push(button);
    }
    nodes.push(copy);
    return nodes;
  }
  if (
    (reading.kind === "revision" || reading.kind === "association") &&
    (route.kind === "revision" || route.kind === "association")
  ) {
    const document = reading.document;
    const nodes: Node[] = [
      detailHeading(
        reading.kind === "association"
          ? "Association comparisons"
          : "Exact Revision",
      ),
      detailIdentity(document.revision),
      detailState([
        ["Currency", document.revisionCurrency.replaceAll("_", " ")],
        ["Relation", document.relationClassification.replaceAll("_", " ")],
        ["Captured resource", document.availability.replaceAll("_", " ")],
        ["Facts", String(document.factPresentations.length)],
        ["Associations", String(document.associations.length)],
      ]),
    ];
    if (reading.kind === "revision") {
      const factRelationships =
        route.kind === "revision"
          ? renderExactFactRelationshipGraph(document, route, actions)
          : null;
      nodes.push(
        detailActions(
          openAnnotatedDiff(route, actions),
          openCapturedResource(route, actions),
          showRevisionInTimeline(route.changeId, route.revision),
        ),
        ...(factRelationships ? [factRelationships] : []),
        renderFacts(reading, route, actions),
        renderFactPorts(reading),
      );
    }
    nodes.push(renderAssociations(reading, route, actions));
    if (reading.kind === "association")
      nodes.push(
        detailActions(
          openAnnotatedDiff(route, actions),
          openCapturedResource(route, actions),
        ),
      );
    nodes.push(copy);
    return nodes;
  }
  return [
    message("Reading surface no longer matches the selected exact route."),
  ];
}

function exactFocusTarget(
  detail: HTMLElement,
  route: ChangeInspectorRoute,
): HTMLElement | null {
  if (
    route.kind !== "revision" &&
    route.kind !== "resource" &&
    route.kind !== "association" &&
    route.kind !== "interdiff"
  ) {
    return null;
  }
  const focus = route.focus;
  if (!focus) return null;
  const targets: HTMLElement[] = [];
  if (focus.factId) {
    const fact = Array.from(
      detail.querySelectorAll<HTMLElement>("[data-fact-id], [data-anno]"),
    ).find(
      (element) =>
        element.dataset.factId === focus.factId ||
        element.dataset.anno === focus.factId,
    );
    if (fact) targets.push(fact);
  }
  if (focus.filePath) {
    const file = Array.from(
      detail.querySelectorAll<HTMLElement>("[data-file-path]"),
    ).find(
      (element) =>
        element.dataset.filePath === focus.filePath ||
        element.dataset.oldFilePath === focus.filePath ||
        element.dataset.newFilePath === focus.filePath,
    );
    if (file) targets.push(file);
  }
  for (const target of targets) {
    target.dataset.exactFocus = "true";
    target.setAttribute("aria-current", "true");
  }
  return targets[0] ?? null;
}

function applyExactFocus(
  detail: HTMLElement,
  route: ChangeInspectorRoute,
): void {
  const target = exactFocusTarget(detail, route);
  if (!target) return;
  if (
    target.dataset.dfile !== undefined &&
    target.dataset.expanded !== "true"
  ) {
    target.querySelector<HTMLElement>(".dfile-head")?.click();
  }
  target.scrollIntoView?.({ block: "center", behavior: "auto" });
  target.focus({ preventScroll: true });
}

function renderDetail(
  snapshot: ChangeInspectorSnapshot,
  actions: ChangeInspectorRenderActions,
  presentation: ChangeInspectorDetailPresentation,
): void {
  const detail = document.querySelector<HTMLElement>("#detail-body");
  if (!detail) return;
  if (snapshot.route.kind === "invalid") {
    replaceDetailWith(message(snapshot.route.message));
    return;
  }
  if (snapshot.diagnostic) {
    replaceDetailWith(message(snapshot.diagnostic));
    return;
  }
  if (
    snapshot.route.kind === "timeline" ||
    snapshot.route.kind === "lens" ||
    snapshot.generation === null
  ) {
    replaceDetailWith(message("Select a Change or exact Revision."));
    return;
  }
  if (snapshot.route.kind === "event") {
    const route = snapshot.route;
    const event = snapshot.generation.history?.entries.find(
      (entry) => entry.eventId === route.eventId,
    );
    replaceDetailWith(
      ...(event
        ? renderEventDetail(event, actions)
        : [
            detailHeading("Event"),
            message(
              "This exact event was not present in the bounded Timeline response.",
            ),
          ]),
    );
    return;
  }
  if (presentation.refusal !== null) {
    replaceDetailWith(
      message(`Reader refused this exact surface: ${presentation.refusal}`),
    );
    return;
  }
  if (presentation.reading !== null) {
    const readingKey = `${formatChangeInspectorRoute(snapshot.route)}:${presentation.reading.document.projectionStamp}`;
    if (detail.dataset.changeReadingKey !== readingKey) {
      detail.replaceChildren(
        ...renderReading(presentation.reading, snapshot, actions),
      );
      detail.dataset.changeReadingKey = readingKey;
      applyExactFocus(detail, snapshot.route);
    }
    return;
  }
  const heading = document.createElement("h2");
  heading.textContent =
    snapshot.route.kind === "change"
      ? "Change"
      : snapshot.route.kind === "resource"
        ? "Captured resource"
        : snapshot.route.kind === "association"
          ? "Association comparison"
          : snapshot.route.kind === "interdiff"
            ? "Revision interdiff"
            : "Exact Revision";
  const identity = document.createElement("p");
  identity.className = "mono";
  const identityText =
    snapshot.route.kind === "change"
      ? `Change ID: ${snapshot.route.changeId}`
      : snapshot.route.kind === "interdiff"
        ? `From: ${snapshot.route.from.revisionId} · ${snapshot.route.from.objectArtifactContentHash}\nTo: ${snapshot.route.to.revisionId} · ${snapshot.route.to.objectArtifactContentHash}`
        : `Revision ID: ${snapshot.route.revision.revisionId} · artifact hash: ${snapshot.route.revision.objectArtifactContentHash}`;
  setCompactIdentityText(identity, identityText);
  const placeholder = message(
    snapshot.route.kind === "change"
      ? "Select an explicit current Revision to inspect its exact context."
      : "Exact reading surface is loading.",
  );
  const copyLink = document.createElement("button");
  copyLink.type = "button";
  copyLink.className = "ghost";
  copyLink.textContent = "Copy link";
  copyLink.addEventListener("click", () => copyExact(copyLink, location.href));
  const peers = document.createElement("section");
  if (snapshot.route.kind === "change" && snapshot.selected !== null) {
    const changeRoute = snapshot.route;
    peers.append(
      renderCurrentRevisionChoices(
        changeRoute.changeId,
        snapshot.selected.currentRevisionRefs,
        changeRoute.query,
        actions,
      ),
    );
  }
  replaceDetailWith(heading, identity, copyLink, placeholder, peers);
}

/** Paint a single already-published generation and its independently resolved route. */
export function renderChangeInspector(
  snapshot: ChangeInspectorSnapshot,
  actions: ChangeInspectorRenderActions,
  presentation: ChangeInspectorDetailPresentation = {
    reading: null,
    refusal: null,
  },
): void {
  const master = document.querySelector<HTMLElement>("#master");
  if (!master) return;
  if (snapshot.route.kind === "diff" && presentation.reading?.kind === "diff") {
    renderChangeInspectorDiffPage(
      presentation.reading.document,
      snapshot.route,
      actions,
    );
    return;
  }
  hideChangeInspectorDiffPage();
  const routeDiagnostic =
    document.querySelector<HTMLElement>("#route-diagnostic");
  if (routeDiagnostic) {
    routeDiagnostic.textContent = snapshot.diagnostic ?? "";
    routeDiagnostic.classList.toggle("hidden", snapshot.diagnostic === null);
  }
  syncFilterChrome(
    snapshot.route,
    snapshot.generation?.history ?? null,
    actions,
  );
  if (snapshot.route.kind !== "timeline") {
    document.querySelector("#follow-toggle")?.classList.add("hidden");
  }
  clearError();
  if (snapshot.route.kind === "invalid") {
    replaceMasterWith(message("Cannot open this Inspector link."));
    renderDetail(snapshot, actions, presentation);
    return;
  }
  const route = snapshot.route;
  if (snapshot.generation === null) {
    replaceMasterWith(message("Loading Change generation…"));
    renderDetail(snapshot, actions, presentation);
    return;
  }
  if (route.kind === "timeline" || route.kind === "event") {
    if (snapshot.generation.history === null) {
      replaceMasterWith(message("Loading Timeline…"));
      renderDetail(snapshot, actions, presentation);
      return;
    }
    const monitor =
      route.kind === "timeline" ? (presentation.timeline ?? null) : null;
    const history = monitor?.display ?? snapshot.generation.history;
    const follow = document.querySelector<HTMLButtonElement>("#follow-toggle");
    if (follow) {
      follow.classList.toggle("hidden", monitor === null);
      if (monitor !== null) {
        const parked = monitor.mode === "parked";
        follow.setAttribute("aria-pressed", String(!parked));
        follow.textContent = parked
          ? monitor.newCount > 0
            ? `Show ${monitor.newCount} new ${monitor.newCount === 1 ? "event" : "events"}`
            : "Parked"
          : "Following";
        follow.setAttribute(
          "aria-label",
          parked
            ? "Show the latest filtered Timeline events and resume following"
            : "Park the Timeline at the current events",
        );
      }
    }
    const timelineRoute =
      route.kind === "timeline"
        ? route
        : { kind: "timeline" as const, historyQuery: route.historyQuery };
    renderChangeInspectorTimeline(
      master,
      history,
      actions,
      timelineRoute,
      route.kind === "event" ? route.eventId : null,
    );
    syncLensLinks("timeline", snapshot.route);
    setText("#stat-events", `${history.eventCount} events`);
    setText(
      "#stat-units",
      `${snapshot.generation.changes.changes.length} Changes`,
    );
    setText(
      "#stat-threads",
      `${snapshot.generation.attention.changes.length} need attention`,
    );
    setText("#stat-hash", history.timelineProjectionStamp);
    renderDetail(snapshot, actions, presentation);
    return;
  }
  const lens = lensForRoute(route);
  const page =
    lens === "changes"
      ? snapshot.generation.changes
      : snapshot.generation.attention;
  // Polling often republishes the same stamped generation. Compute the cache
  // key before allocating card nodes so an unchanged tick does not build and
  // discard the complete bounded tree every three seconds.
  const listKey = JSON.stringify({
    lens,
    query: route.query,
    projectionStamp: page.projectionStamp,
    previous: page.previous ?? null,
    next: page.next,
    last: page.last ?? null,
    changes: page.changes.map((change) => change.changeId),
  });
  if (master.dataset.changeListKey !== listKey) {
    const list = document.createElement("section");
    list.className = "units";
    const heading = document.createElement("h1");
    heading.textContent = `${lens === "changes" ? "Changes" : "Attention"} · ${page.changes.length}`;
    list.append(heading);
    // The protocol caps individual responses lower than this guard. Keep the
    // render-side bound as a second line of defence against an over-full server
    // page: a large store must never create an unbounded live DOM.
    for (const summary of page.changes.slice(0, 150)) {
      const card = changeCardPresentation(
        summary,
        page.presentations?.[summary.changeId],
      );
      const element = document.createElement("article");
      element.className = "unit-card";
      element.dataset.changeId = summary.changeId;
      element.setAttribute("aria-label", card.accessibleName);

      const primary = document.createElement("button");
      primary.type = "button";
      primary.className = "change-card-primary";
      primary.setAttribute(
        "aria-label",
        `${card.primaryAction.label}. ${card.attention ? `${card.attention.primary.accessibleName}. ` : ""}${card.accessibleName}`,
      );
      primary.title = card.title;
      const headline = document.createElement("span");
      headline.className = "change-card-headline";
      headline.textContent = card.headline;
      const identity = document.createElement("code");
      identity.className = "change-card-id mono";
      identity.textContent = card.visibleChangeId;
      identity.title = card.title;
      primary.append(headline, identity);
      primary.addEventListener("click", () =>
        actions.navigate({
          kind: "change",
          changeId: summary.changeId,
          query: queryForExactNavigation(route),
        }),
      );
      element.append(primary);

      if (card.attention) {
        const attention = document.createElement("section");
        attention.className = "change-card-attention";
        attention.setAttribute(
          "aria-label",
          card.attention.primary.accessibleName,
        );
        const reason = document.createElement("strong");
        reason.className = "change-card-attention-reason";
        reason.textContent = card.attention.reason;
        reason.title = card.attention.primary.title;
        const ask = document.createElement("p");
        ask.className = "change-card-attention-ask";
        ask.textContent = card.attention.ask;
        const action = document.createElement("p");
        action.className = "change-card-attention-action";
        action.textContent = `Next: ${card.attention.actionLabel}`;
        attention.append(reason, ask, action);
        if (card.attention.additionalReasons.length > 0) {
          const additional = document.createElement("ul");
          additional.className = "change-card-attention-additional";
          additional.setAttribute("aria-label", "Additional reasons");
          for (const item of card.attention.additionalReasons) {
            const row = document.createElement("li");
            row.textContent = `${item.reason}: ${item.ask}`;
            row.title = item.title;
            row.setAttribute("aria-label", item.accessibleName);
            additional.append(row);
          }
          attention.append(additional);
        }
        if (card.attention.diagnostics?.length) {
          const details = document.createElement("section");
          details.className = "change-card-attention-diagnostics";
          details.setAttribute("aria-label", "Attention details");
          const detailsHeading = document.createElement("strong");
          detailsHeading.textContent = "Details";
          const diagnostics = document.createElement("ul");
          for (const diagnostic of card.attention.diagnostics) {
            const row = document.createElement("li");
            row.textContent = diagnostic;
            diagnostics.append(row);
          }
          details.append(detailsHeading, diagnostics);
          attention.append(details);
        }
        element.append(attention);
      }

      const state = document.createElement("dl");
      state.className = "change-card-state";
      for (const axis of card.stateAxes) {
        const label = document.createElement("dt");
        label.textContent = axis.label;
        const value = document.createElement("dd");
        value.textContent = axis.value;
        state.append(label, value);
      }
      element.append(state);

      if (card.unavailableReason) {
        const unavailable = document.createElement("p");
        unavailable.className = "change-card-unavailable";
        unavailable.textContent = card.unavailableReason;
        element.append(unavailable);
      } else if (card.peers.length === 1) {
        const peer = card.peers[0];
        const current = document.createElement("p");
        current.className = "change-card-current";
        current.append("Current Revision · ");
        const exact = exactRevisionIdentity(peer.revision);
        current.append(exact);
        element.append(current);
      } else {
        const peers = document.createElement("section");
        peers.className = "change-card-peers";
        const peerHeading = document.createElement("h3");
        peerHeading.textContent = "Choose an exact current Revision";
        peers.append(peerHeading);
        for (const peer of card.peers) {
          const choose = document.createElement("button");
          choose.type = "button";
          choose.className = "ghost change-card-peer-open";
          choose.textContent = `${peer.label} · ${peer.visibleIdentity}`;
          choose.title = peer.title;
          choose.setAttribute(
            "aria-label",
            `${peer.accessibleName}; open for Change ${summary.changeId}`,
          );
          choose.dataset.changeId = summary.changeId;
          choose.dataset.revisionId = peer.revision.revisionId;
          choose.dataset.artifactHash = peer.revision.objectArtifactContentHash;
          choose.addEventListener("click", () =>
            actions.navigate({
              kind: "revision",
              changeId: summary.changeId,
              revision: peer.revision,
              query: queryForExactNavigation(route),
            }),
          );
          peers.append(choose);
        }
        element.append(peers);
      }
      list.append(element);
    }
    if (page.changes.length === 0)
      list.append(
        message(
          lens === "changes" ? "No Changes." : "No Changes need attention.",
        ),
      );
    const appendPager = (
      direction: "previous" | "next" | "last",
      continuation: string | null | undefined,
    ) => {
      if (continuation == null) return;
      const target = {
        kind: "lens" as const,
        lens: lens as "changes" | "attention",
        query: { ...route.query, after: continuation },
      };
      const button = document.createElement("button");
      button.type = "button";
      button.className = "ghost";
      button.dataset.changePage = direction;
      button.dataset.changeTargetRoute = formatChangeInspectorRoute(target);
      button.textContent = `${direction === "previous" ? "Previous" : direction === "next" ? "Next" : "Last"} page`;
      button.addEventListener("click", () => actions.navigate(target));
      list.append(button);
    };
    appendPager("previous", page.previous);
    appendPager("next", page.next);
    if (page.next !== null) appendPager("last", page.last);
    master.replaceChildren(list);
    master.dataset.changeListKey = listKey;
  }
  syncLensLinks(lens, snapshot.route);
  setText(
    "#stat-events",
    `${snapshot.generation.profile.authorityCursor.eventCount ?? "—"} events`,
  );
  setText(
    "#stat-units",
    `${snapshot.generation.changes.changes.length} Changes`,
  );
  setText(
    "#stat-threads",
    `${snapshot.generation.attention.changes.length} need attention`,
  );
  setText("#stat-hash", snapshot.generation.changes.projectionStamp);
  renderDetail(snapshot, actions, presentation);
}

export function renderChangeInspectorUnavailable(
  availability: "migration_required" | "migration_in_progress",
): void {
  clearError();
  replaceMasterWith(
    message(
      availability === "migration_required"
        ? "Store migration required. No Change state was loaded."
        : "Store migration in progress. Partial Change state is unavailable.",
    ),
  );
  replaceDetailWith(message("Change state is unavailable."));
}

export function renderChangeInspectorRefusal(error: unknown): void {
  const text = error instanceof Error ? error.message : String(error);
  replaceMasterWith(message(`Reader refused: ${text}`));
  replaceDetailWith(message("Change state was not published."));
  const diagnostic = document.querySelector<HTMLElement>("#route-diagnostic");
  if (diagnostic) {
    diagnostic.textContent = "";
    diagnostic.classList.add("hidden");
  }
  const banner = document.querySelector<HTMLElement>("#error");
  if (banner) {
    banner.textContent = `error: ${text}`;
    banner.classList.remove("hidden");
  }
}
