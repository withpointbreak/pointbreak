/**
 * Change-aware Timeline projection. It consumes only the typed `/api/v2/history`
 * document, never the retired aggregate store. The server owns filtering,
 * chronology, continuation signing, and semantic subjects; this module owns
 * bounded DOM geometry, keyboard-focusable rows, and explicit navigation.
 */

import {
  eventGroupLabel,
  eventTypeColor,
  presentEvent,
} from "./change-inspector-event-presentation";
import { createLensHeading } from "./change-inspector-lens";
import type { ChangeInspectorNavigationActions } from "./change-inspector-render";
import {
  type ChangeInspectorRoute,
  formatChangeInspectorRoute,
  timelineEventRoute,
} from "./change-inspector-router";
import {
  collapsedGroupAt,
  GROUP_MIN_RUN,
  groupKey,
  groupTimelineEntries,
  navigableEventIds,
  owningGroupKey,
  type TimelineGroup,
  type TimelineRow,
  visualRows,
} from "./change-inspector-timeline-grouping";
import type {
  EventHistoryDocument,
  EventHistoryEntry,
  EventHistoryRevisionRef,
} from "./change-protocol";
import { CLASS } from "./classNames";
import { registerDensityListener } from "./prefs";
import {
  compactIdentityText,
  exactRevisionAccessibleIdentity,
  shortExactRevision,
  shortRef,
} from "./refs";

const FALLBACK_ROW_HEIGHT = 72;
const OVERSCAN = 8;
const REMEASURE_SETTLE_MS = 150;

interface TimelineView {
  document: EventHistoryDocument;
  /** Page-local grouping derived from exactly this document. */
  grouped: readonly TimelineRow[];
  /** Groups the reader opened; keyed by first member id, render-key lifetime. */
  expanded: Set<string>;
  /** The visual rows the virtual window paints: one `<li>` per row. */
  rows: readonly TimelineRow[];
  list: HTMLOListElement;
  remeasureTimer: ReturnType<typeof setTimeout> | null;
  resizeObserver: ResizeObserver | null;
  rowHeight: number;
  route: Extract<ChangeInspectorRoute, { kind: "timeline" }>;
  routeSelectedEventId: string | null;
  selectedEventId: string | null;
}

let active: TimelineView | null = null;

function label(value: string): string {
  return value.replaceAll("_", " ");
}

const MAX_TIMELINE_TITLE = 120;
const MAX_TIMELINE_EXCERPT = 180;

function compactTimelineText(value: string, limit: number): string {
  const compact = compactIdentityText(value).replace(/\s+/g, " ").trim();
  if (compact.length <= limit) return compact;
  return `${compact.slice(0, limit - 1).trimEnd()}…`;
}

function timelineTitle(value: string): string {
  return compactTimelineText(value, MAX_TIMELINE_TITLE);
}

function timelineExcerpt(value: string): string {
  return compactTimelineText(value, MAX_TIMELINE_EXCERPT);
}

function appendTimelineLink(
  parent: HTMLElement,
  identity: string,
  kind: "Change" | "Revision" | "event",
  href: string,
): HTMLAnchorElement {
  const link = document.createElement("a");
  link.className = "ref";
  link.href = href;
  link.tabIndex = -1;
  link.title = identity;
  link.dataset.timelineContextKind = kind.toLowerCase();
  link.dataset.timelineContextId = identity;
  link.setAttribute("aria-label", `Open ${kind} ${identity}`);
  link.textContent = shortRef(identity);
  parent.append(link);
  return link;
}

function appendExactRevisionLink(
  parent: HTMLElement,
  reference: EventHistoryRevisionRef,
  route: Extract<ChangeInspectorRoute, { kind: "timeline" }>,
): void {
  // An event may name several Changes. A Timeline filter gives this exact
  // Revision an honest action without inventing which Change owns it.
  const link = appendTimelineLink(
    parent,
    reference.revisionId,
    "Revision",
    formatChangeInspectorRoute({
      kind: "timeline",
      historyQuery: {
        ...route.historyQuery,
        after: undefined,
        at: undefined,
        change: undefined,
        revision: reference.revisionId,
        artifactHash: reference.objectArtifactContentHash,
      },
    }),
  );
  const fullIdentity = exactRevisionAccessibleIdentity(reference);
  link.textContent = shortExactRevision(reference);
  link.title = fullIdentity;
  link.setAttribute("aria-label", `Filter Timeline to ${fullIdentity}`);
  link.dataset.revisionId = reference.revisionId;
  link.dataset.artifactHash = reference.objectArtifactContentHash;
}

function optionId(eventId: string): string {
  return `timeline-event-${encodeURIComponent(eventId).replaceAll("%", "_")}`;
}

/**
 * A page filtered to exactly one event type is the reader asking for that
 * whole run; collapsing it would hide the page behind a single row. Any other
 * query groups at the design threshold.
 */
function groupingMinRun(
  route: Extract<ChangeInspectorRoute, { kind: "timeline" }>,
): number {
  const type = route.historyQuery.type;
  return type !== undefined && !type.includes(",")
    ? Number.POSITIVE_INFINITY
    : GROUP_MIN_RUN;
}

function deriveTimelineRows(view: TimelineView): void {
  view.rows = visualRows(view.grouped, view.expanded);
}

/**
 * An exact event route names an event, never a group summary. Open the
 * group that owns it, including when it is the group's first member, so the
 * reveal that follows lands on the member's own option row.
 */
function expandOwningGroup(view: TimelineView, eventId: string): void {
  const owner = owningGroupKey(view.grouped, eventId);
  if (owner === null || view.expanded.has(owner)) return;
  view.expanded.add(owner);
  deriveTimelineRows(view);
}

/** The shared option scaffolding every Timeline row carries. */
function optionRow(
  eventId: string,
  selectedEventId: string | null,
): HTMLLIElement {
  const row = document.createElement("li");
  row.className = "event";
  row.dataset.eventId = eventId;
  row.id = optionId(eventId);
  row.tabIndex = -1;
  row.setAttribute("role", "option");
  row.setAttribute("aria-selected", String(eventId === selectedEventId));
  return row;
}

function appendOccurredAt(row: HTMLElement, occurredAt: string): void {
  const occurred = new Date(occurredAt);
  const time = document.createElement("time");
  time.className = "time";
  time.dateTime = occurredAt;
  if (Number.isNaN(occurred.valueOf())) {
    time.textContent = occurredAt;
  } else {
    const date = document.createElement("span");
    date.className = "event-date";
    date.textContent = occurred.toLocaleDateString();
    const clock = document.createElement("span");
    clock.textContent = occurred.toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
    time.append(date, clock);
  }
  row.append(time);
}

function appendRail(
  row: HTMLElement,
  eventType: EventHistoryEntry["eventType"],
): void {
  const rail = document.createElement("span");
  rail.className = "rail";
  rail.style.background = eventTypeColor(eventType);
  rail.setAttribute("aria-hidden", "true");
  row.append(rail);
}

/**
 * One collapsed same-type run as a single option row. It is addressed by its
 * first member's id, carries no links, and (per WAI-ARIA 1.2) no
 * `aria-expanded`: that attribute is unsupported on `role="option"`.
 */
/** The accessible name shared by a group's collapsed row and expanded container. */
function groupName(group: TimelineGroup): string {
  const first = group.members[0];
  if (first === undefined) throw new Error("Timeline group has no members");
  return `${eventGroupLabel(first)}, ${group.members.length} events`;
}

/**
 * An expanded group is a labelled `role="group"` container (permitted inside
 * a listbox) whose children are the ordinary member option rows. It carries
 * no `data-event-id` and no `event` class, so selection, measurement, and the
 * keyboard cursor see only the member rows.
 */
function groupContainer(group: TimelineGroup): {
  item: HTMLLIElement;
  members: HTMLOListElement;
} {
  const item = document.createElement("li");
  item.className = CLASS.timelineGroupMembers;
  item.dataset.timelineGroupMembers = group.eventType;
  item.setAttribute("role", "group");
  item.setAttribute("aria-label", groupName(group));
  const members = document.createElement("ol");
  members.setAttribute("role", "none");
  item.append(members);
  return { item, members };
}

function groupRow(
  group: TimelineGroup,
  selectedEventId: string | null,
): HTMLLIElement {
  const first = group.members[0];
  if (first === undefined) throw new Error("Timeline group has no members");
  const presentation = presentEvent(first);
  const row = optionRow(first.eventId, selectedEventId);
  row.classList.add(CLASS.timelineGroup);
  row.dataset.timelineGroup = group.eventType;
  row.dataset.timelineGroupSize = String(group.members.length);
  // The collapsed state lives in the accessible name: WAI-ARIA 1.2 does not
  // support aria-expanded on role=option. Activating the row expands it.
  row.setAttribute("aria-label", `${groupName(group)}, collapsed`);
  appendOccurredAt(row, first.occurredAt);
  appendRail(row, group.eventType);
  const body = document.createElement("div");
  body.className = "body";
  const heading = document.createElement("h3");
  heading.className = "title";
  heading.textContent = eventGroupLabel(first);
  const meta = document.createElement("div");
  meta.className = "mono";
  meta.classList.add("meta");
  const eventType = document.createElement("span");
  eventType.className = "type";
  eventType.textContent = presentation.label;
  eventType.title = group.eventType;
  eventType.style.color = eventTypeColor(group.eventType);
  const count = document.createElement("span");
  count.className = CLASS.typeCount;
  count.textContent = String(group.members.length);
  meta.append(eventType, count);
  body.append(heading, meta);
  row.append(body);
  return row;
}

function rowSpacer(height: number): HTMLLIElement {
  const spacer = document.createElement("li");
  spacer.dataset.timelineSpacer = "true";
  spacer.setAttribute("aria-hidden", "true");
  spacer.style.height = `${height}px`;
  return spacer;
}

function appendChip(row: HTMLElement, text: string): void {
  const chip = document.createElement("span");
  chip.className = "badge";
  chip.textContent = text;
  row.append(chip);
}

function appendVerificationChip(
  row: HTMLElement,
  status: EventHistoryEntry["verificationStatus"],
): void {
  const chip = document.createElement("span");
  chip.className = `verify verify-${status}`;
  chip.title = "event signature verification status";
  chip.textContent = `verify: ${label(status)}`;
  row.append(chip);
}

function entryRow(
  entry: EventHistoryEntry,
  selectedEventId: string | null,
  route: Extract<ChangeInspectorRoute, { kind: "timeline" }>,
): HTMLLIElement {
  const presentation = presentEvent(entry);
  const row = optionRow(entry.eventId, selectedEventId);
  row.setAttribute(
    "aria-label",
    `${presentation.title}; ${entry.eventType}; writer ${entry.writer.actorId}; ${entry.occurredAt}; event ${entry.eventId}; Changes ${entry.changeIds.join(", ") || "none"}; exact Revisions ${entry.revisionRefs.map((reference) => `${reference.revisionId} ${reference.objectArtifactContentHash}`).join(", ") || "none"}; unresolved Revisions ${entry.unresolvedRevisionIds.join(", ") || "none"}`,
  );
  appendOccurredAt(row, entry.occurredAt);
  appendRail(row, entry.eventType);
  const body = document.createElement("div");
  body.className = "body";
  const heading = document.createElement("h3");
  heading.className = "title";
  // A supplied presentation title is prose, not an unrestricted layout
  // channel. Keep Timeline geometry bounded and use the same short form as
  // other opaque identities, while the native title and row label retain the
  // full semantic source for assistive technology and inspection.
  heading.textContent = timelineTitle(presentation.title);
  heading.title = presentation.title;
  if (presentation.body) {
    const summary = document.createElement("p");
    summary.className = "event-summary";
    summary.textContent = timelineExcerpt(presentation.body);
    body.append(heading, summary);
  } else {
    body.append(heading);
  }
  const meta = document.createElement("div");
  meta.className = "mono";
  meta.classList.add("meta");
  const eventType = document.createElement("span");
  eventType.className = "type";
  eventType.textContent = presentation.label;
  eventType.title = entry.eventType;
  eventType.style.color = eventTypeColor(entry.eventType);
  meta.append(eventType);
  appendVerificationChip(meta, entry.verificationStatus);
  if (entry.trackId) appendChip(meta, `track ${entry.trackId}`);
  const actor = document.createElement("span");
  actor.textContent = entry.writer.actorId;
  actor.title = `writer ${entry.writer.actorId}`;
  meta.append(actor);
  appendTimelineLink(
    meta,
    entry.eventId,
    "event",
    formatChangeInspectorRoute(
      timelineEventRoute(entry.eventId, route.historyQuery),
    ),
  );

  const contexts = document.createElement("p");
  contexts.className = "event-context mono";
  if (entry.changeIds.length) {
    const changes = document.createElement("span");
    changes.textContent = "Changes ";
    contexts.append(changes);
    entry.changeIds.forEach((changeId, index) => {
      if (index) contexts.append(document.createTextNode(", "));
      appendTimelineLink(
        contexts,
        changeId,
        "Change",
        formatChangeInspectorRoute({ kind: "change", changeId, query: {} }),
      );
    });
  }
  if (entry.revisionRefs.length) {
    if (contexts.childNodes.length)
      contexts.append(document.createTextNode(" · "));
    const revisions = document.createElement("span");
    revisions.textContent = "Revisions ";
    contexts.append(revisions);
    entry.revisionRefs.forEach((reference, index) => {
      if (index) contexts.append(document.createTextNode(", "));
      appendExactRevisionLink(contexts, reference, route);
    });
  }
  if (entry.unresolvedRevisionIds.length) {
    if (contexts.childNodes.length)
      contexts.append(document.createTextNode(" · "));
    const unresolved = document.createElement("span");
    unresolved.className = "warning";
    unresolved.title = entry.unresolvedRevisionIds.join(", ");
    unresolved.textContent = `unresolved ${entry.unresolvedRevisionIds.map(shortRef).join(", ")}`;
    contexts.append(unresolved);
  }
  body.append(meta);
  if (contexts.childNodes.length) body.append(contexts);
  row.append(body);
  return row;
}

function paintVisible(view: TimelineView): void {
  const { list, rows, rowHeight } = view;
  const viewport = list.clientHeight;
  const localStart =
    viewport > 0
      ? Math.max(0, Math.floor(list.scrollTop / rowHeight) - OVERSCAN)
      : 0;
  const localEnd =
    viewport > 0
      ? Math.min(
          rows.length,
          Math.ceil((list.scrollTop + viewport) / rowHeight) + OVERSCAN,
        )
      : rows.length;
  // The browser materializes one server-bounded page. Virtual geometry is
  // therefore page-local: global `offset`/`matchCount` are labels, not rows the
  // browser may pretend are loaded or scrollable. The virtual index runs over
  // VISUAL rows: a collapsed group is one `<li>`, so the uniform-height
  // estimator stays valid.
  const top = rowSpacer(localStart * rowHeight);
  const bottom = rowSpacer(Math.max(0, rows.length - localEnd) * rowHeight);
  const painted: HTMLLIElement[] = [];
  // A group's members are contiguous in the visual rows, so one container
  // per expanded group inside the painted window is exactly the DOM needed.
  const containers = new Map<TimelineGroup, HTMLOListElement>();
  for (const row of rows.slice(localStart, localEnd)) {
    if (row.kind === "group") {
      painted.push(groupRow(row, view.selectedEventId));
      continue;
    }
    const member = entryRow(row.entry, view.selectedEventId, view.route);
    if (row.ofGroup === undefined) {
      painted.push(member);
      continue;
    }
    let members = containers.get(row.ofGroup);
    if (members === undefined) {
      const created = groupContainer(row.ofGroup);
      members = created.members;
      containers.set(row.ofGroup, members);
      painted.push(created.item);
    }
    members.append(member);
  }
  list.replaceChildren(top, ...painted, bottom);
  const activeOption = view.selectedEventId
    ? Array.from(list.querySelectorAll<HTMLElement>("[data-event-id]")).find(
        (row) => row.dataset.eventId === view.selectedEventId,
      )
    : null;
  if (activeOption) {
    list.setAttribute("aria-activedescendant", activeOption.id);
  } else {
    list.removeAttribute("aria-activedescendant");
  }
}

/**
 * Preserve the row and pixel offset at the viewport top while a new measured
 * height re-derives the virtual spacers. The fallback scales the current
 * position when layout is unavailable or the viewport is inside a spacer.
 */
function anchoredScrollTop(view: TimelineView, nextRowHeight: number): number {
  const { list, rowHeight: previousRowHeight } = view;
  const listTop = list.getBoundingClientRect().top;
  const leading = list.firstElementChild as HTMLElement | null;
  const leadingHeight = leading?.dataset.timelineSpacer
    ? Number.parseFloat(leading.style.height) || 0
    : 0;
  const paintStart = Math.round(leadingHeight / previousRowHeight);
  const rows = list.querySelectorAll<HTMLElement>("li.event[data-event-id]");
  let localIndex = 0;
  for (const row of rows) {
    const bounds = row.getBoundingClientRect();
    if (bounds.height > 0 && bounds.bottom > listTop) {
      return Math.max(
        0,
        (paintStart + localIndex) * nextRowHeight - (bounds.top - listTop),
      );
    }
    localIndex += 1;
  }
  return (list.scrollTop / previousRowHeight) * nextRowHeight;
}

/** Reconcile virtual geometry from the currently painted rows. */
export function remeasureChangeInspectorTimelineRows(): boolean {
  const view = active;
  if (view === null || !view.list.isConnected) return false;
  const rows = Array.from(
    view.list.querySelectorAll<HTMLElement>("li.event[data-event-id]"),
  );
  if (rows.length === 0) return false;
  const mean =
    rows.reduce((total, row) => total + row.getBoundingClientRect().height, 0) /
    rows.length;
  if (!Number.isFinite(mean) || mean <= 0) return false;
  if (Math.abs(mean - view.rowHeight) < 0.5) return false;
  const anchored = anchoredScrollTop(view, mean);
  view.rowHeight = mean;
  view.list.scrollTop = anchored;
  paintVisible(view);
  return true;
}

/** Coalesce resize, density, and reading-mode changes into one stable repaint. */
export function scheduleChangeInspectorTimelineRemeasure(): void {
  const view = active;
  if (view === null) return;
  if (view.remeasureTimer !== null) clearTimeout(view.remeasureTimer);
  view.remeasureTimer = setTimeout(() => {
    view.remeasureTimer = null;
    if (active === view) remeasureChangeInspectorTimelineRows();
  }, REMEASURE_SETTLE_MS);
}

registerDensityListener(scheduleChangeInspectorTimelineRemeasure);

function disposeActiveTimeline(): void {
  if (active === null) return;
  if (active.remeasureTimer !== null) clearTimeout(active.remeasureTimer);
  active.resizeObserver?.disconnect();
}

/** Render a bounded virtual window; paging remains a server-signed navigation. */
export function renderChangeInspectorTimeline(
  master: HTMLElement,
  timeline: EventHistoryDocument,
  actions: ChangeInspectorNavigationActions,
  route: Extract<ChangeInspectorRoute, { kind: "timeline" }>,
  selectedEventId: string | null = null,
): void {
  // A timeline projection stamp identifies the server snapshot, but a query
  // can change the visible subset without changing that snapshot. Include the
  // whole routed query so changing search, filters, order, or continuation
  // replaces the DOM rather than repainting stale rows under new controls.
  const key = `${timeline.timelineProjectionStamp}\u0000${JSON.stringify(route.historyQuery)}`;
  if (master.dataset.timelineKey === key && active !== null) {
    const exactRouteChanged = selectedEventId !== active.routeSelectedEventId;
    if (active.document !== timeline) {
      active.document = timeline;
      active.grouped = groupTimelineEntries(
        timeline.entries,
        groupingMinRun(route),
      );
      deriveTimelineRows(active);
    }
    active.route = route;
    active.list.dataset.timelineRoute = formatChangeInspectorRoute(route);
    active.routeSelectedEventId = selectedEventId;
    if (exactRouteChanged && selectedEventId !== null) {
      active.selectedEventId = selectedEventId;
      expandOwningGroup(active, selectedEventId);
    }
    paintVisible(active);
    if (exactRouteChanged && selectedEventId !== null) {
      revealChangeInspectorTimelineEvent(selectedEventId);
    }
    return;
  }
  const grouped = groupTimelineEntries(timeline.entries, groupingMinRun(route));
  const section = document.createElement("section");
  section.className = "timeline-shell";
  // Page-local grouping discloses its scope in the metadata line, the same
  // honesty convention the Attention lens uses for its page-scoped groups.
  const collapsedNotice = grouped.some((row) => row.kind === "group")
    ? " · adjacent same-type events collapsed"
    : "";
  const [heading, metadata] = createLensHeading(
    "Timeline",
    `${timeline.matchCount} ${timeline.matchCount === 1 ? "event" : "events"} · ${timeline.order === "desc" ? "newest" : "oldest"} first${collapsedNotice}`,
  );
  const notice = document.createElement("p");
  notice.className = "timeline-summary dim";
  const loadedStart = timeline.entries.length ? timeline.offset + 1 : 0;
  const loadedEnd = timeline.offset + timeline.entries.length;
  notice.textContent = `loaded ${loadedStart}-${loadedEnd} of ${timeline.matchCount} matches · ${timeline.eventCount} recorded events. Presentation chronology uses writer timestamps; late events can backfill when writer clocks differ.`;
  const notices = document.createElement("div");
  notices.className = "timeline-notices";
  notices.setAttribute("aria-live", "polite");
  for (const message of timeline.queryNotices) {
    const line = document.createElement("p");
    line.className = "info";
    line.textContent = `Query notice: ${message}`;
    notices.append(line);
  }
  for (const message of timeline.diagnostics) {
    const line = document.createElement("p");
    line.className = "warning";
    line.textContent = `Timeline diagnostic: ${message}`;
    notices.append(line);
  }
  const page = document.createElement("div");
  page.className = "actions";
  if (timeline.previous) {
    const previousRoute = {
      kind: "timeline" as const,
      historyQuery: {
        ...route.historyQuery,
        at: undefined,
        after: timeline.previous,
      },
    };
    const previous = document.createElement("button");
    previous.type = "button";
    previous.className = "ghost";
    previous.dataset.timelinePage = "previous";
    previous.dataset.timelineTargetRoute =
      formatChangeInspectorRoute(previousRoute);
    previous.textContent = "Previous page";
    previous.addEventListener("click", () => actions.navigate(previousRoute));
    page.append(previous);
  }
  if (timeline.next) {
    const nextRoute = {
      kind: "timeline" as const,
      historyQuery: {
        ...route.historyQuery,
        at: undefined,
        after: timeline.next,
      },
    };
    const next = document.createElement("button");
    next.type = "button";
    next.className = "ghost";
    next.dataset.timelinePage = "next";
    next.dataset.timelineTargetRoute = formatChangeInspectorRoute(nextRoute);
    next.textContent = "Next page";
    next.addEventListener("click", () => actions.navigate(nextRoute));
    page.append(next);
  }
  const list = document.createElement("ol");
  list.id = "timeline";
  list.className = "timeline";
  list.dataset.timelineRoute = formatChangeInspectorRoute(route);
  list.tabIndex = timeline.entries.length ? 0 : -1;
  list.setAttribute("role", "listbox");
  list.setAttribute("aria-label", "event timeline");
  if (!timeline.entries.length) list.setAttribute("aria-disabled", "true");
  section.append(heading, metadata, notice, notices, page);
  if (timeline.matchCount === 0) {
    const empty = document.createElement("p");
    empty.className = "timeline-empty dim";
    empty.setAttribute("role", "status");
    empty.textContent = "No Timeline events match the current filters.";
    section.append(empty);
  }
  section.append(list);
  delete master.dataset.changeListKey;
  disposeActiveTimeline();
  master.replaceChildren(section);
  master.dataset.timelineKey = key;
  active = {
    document: timeline,
    grouped,
    expanded: new Set(),
    rows: visualRows(grouped, new Set()),
    list,
    remeasureTimer: null,
    resizeObserver: null,
    rowHeight: FALLBACK_ROW_HEIGHT,
    route,
    routeSelectedEventId: selectedEventId,
    selectedEventId,
  };
  const view = active;
  list.addEventListener("scroll", () => {
    if (active === view) paintVisible(view);
  });
  if (typeof ResizeObserver !== "undefined") {
    view.resizeObserver = new ResizeObserver(() => {
      if (active === view) scheduleChangeInspectorTimelineRemeasure();
    });
    view.resizeObserver.observe(list);
  }
  if (selectedEventId !== null) expandOwningGroup(view, selectedEventId);
  paintVisible(view);
  if (selectedEventId !== null) {
    // An exact event route can anchor a bounded page whose selected event is
    // outside the first virtual window. Materialize that row immediately so
    // the deep link has the same visible selection and active-descendant
    // semantics as an event opened from the Timeline.
    revealChangeInspectorTimelineEvent(selectedEventId);
  } else {
    remeasureChangeInspectorTimelineRows();
  }
}

/**
 * Reveal one event already present in the materialized page. Keyboard cursor
 * policy owns the event identity; this function only adjusts local virtual
 * paint geometry so the corresponding option can receive active-descendant
 * focus without asking the server for another page.
 */
export function revealChangeInspectorTimelineEvent(eventId: string): boolean {
  if (active === null) return false;
  let localIndex = active.rows.findIndex((row) => groupKey(row) === eventId);
  if (localIndex < 0) {
    // Reveal expands: a member of a collapsed group is in the document but
    // not among the visual rows, so open its owner before searching again.
    const owner = collapsedGroupAt(active.grouped, eventId, active.expanded);
    if (owner === null) return false;
    active.expanded.add(owner);
    deriveTimelineRows(active);
    localIndex = active.rows.findIndex((row) => groupKey(row) === eventId);
    if (localIndex < 0) return false;
  }
  active.selectedEventId = eventId;
  remeasureChangeInspectorTimelineRows();
  const top = localIndex * active.rowHeight;
  const bottom = top + active.rowHeight;
  if (top < active.list.scrollTop) active.list.scrollTop = top;
  else if (bottom > active.list.scrollTop + active.list.clientHeight) {
    active.list.scrollTop = Math.max(0, bottom - active.list.clientHeight);
  }
  paintVisible(active);
  // The selected row's real height may differ from the prior window's mean.
  // Reconcile once more, repaint from the same anchored reading position, then
  // let the browser make the final exact correction within the mounted window.
  remeasureChangeInspectorTimelineRows();
  paintVisible(active);
  let selected = Array.from(
    active.list.querySelectorAll<HTMLElement>("li.event[data-event-id]"),
  ).find((row) => row.dataset.eventId === eventId);
  if (selected === undefined) {
    // Row content has deliberately variable height. A page-local mean is a
    // good first estimate, but cumulative error can leave a boundary event
    // just outside the overscanned window. Snap exact page boundaries to the
    // real scroll extent, then repaint once from that browser-owned geometry.
    // This keeps `g`/`G` and exact deep links honest without materializing the
    // whole server-bounded page.
    if (localIndex === 0) active.list.scrollTop = 0;
    else if (localIndex === active.rows.length - 1) {
      active.list.scrollTop = active.list.scrollHeight;
    }
    paintVisible(active);
    selected = Array.from(
      active.list.querySelectorAll<HTMLElement>("li.event[data-event-id]"),
    ).find((row) => row.dataset.eventId === eventId);
  }
  selected?.scrollIntoView({ block: "nearest", behavior: "auto" });
  return selected !== undefined;
}

/**
 * The navigable id sequence for the mounted Timeline: exactly the visible
 * sequence, so the keyboard cursor never walks an id with no rendered row.
 * A document this module has not painted has no groups, so its raw entry
 * order is returned unchanged.
 */
export function changeInspectorTimelineNavigableEventIds(
  timeline?: EventHistoryDocument,
): readonly string[] {
  if (
    active !== null &&
    (timeline === undefined || active.document === timeline)
  ) {
    return navigableEventIds(active.rows);
  }
  return timeline === undefined
    ? []
    : timeline.entries.map((entry) => entry.eventId);
}

/**
 * The group owning `eventId`, or null. By default only a COLLAPSED owner is
 * reported; `includeExpanded` also reports an expanded one, which a collapse
 * key needs to close the group the cursor is currently inside.
 */
export function changeInspectorTimelineGroupAt(
  eventId: string | null,
  options?: { includeExpanded?: boolean },
): string | null {
  if (active === null || eventId === null) return null;
  return options?.includeExpanded
    ? owningGroupKey(active.grouped, eventId)
    : collapsedGroupAt(active.grouped, eventId, active.expanded);
}

/** Open or close one group and repaint the visual rows in place. */
export function setChangeInspectorTimelineGroupExpanded(
  groupKeyValue: string,
  expanded: boolean,
): void {
  if (active === null) return;
  if (owningGroupKey(active.grouped, groupKeyValue) !== groupKeyValue) return;
  if (active.expanded.has(groupKeyValue) === expanded) return;
  if (expanded) active.expanded.add(groupKeyValue);
  else active.expanded.delete(groupKeyValue);
  deriveTimelineRows(active);
  paintVisible(active);
  remeasureChangeInspectorTimelineRows();
}
