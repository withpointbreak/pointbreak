// The render orchestrator: the single store subscriber. `render` is a plain
// `() => void` that paints one frame from `getState()` — the stat row, the load
// diagnostics, the type-toggle facets, the lens switcher, the master lens body
// (delegating to the lenses), the detail pane (delegating to detail), the
// selection scroll, and the diff overlay reconciler. Ported from the served app.js
// render cluster (`renderAll`/`render`/`renderMaster`/`renderSelected`/
// `renderStats`/`renderDiagnostics`/`renderTypeToggles`/`renderLensSwitcher`/
// `syncControls`/`scrollSelectionIntoView`).
//
// Two invariants shape it. `render` is the ONLY `subscribe()` target (wired once in
// the composition root); it reads `getState()` and never calls `navigate`/`commit`
// — the once-installed `#master` / `#filter-types` delegates own the commits. The
// served per-render row / card / type-toggle listeners collapse into those two
// delegates: `initControls` installs them once, never per render.

import { filterChipsFor, removeFilterChipToken } from "./chips";
import { CLASS, filterChipClass, typeFacetRowClass } from "./classNames";
import { renderDetail, showComposite } from "./detail";
import { openDiff, renderDiffPage } from "./diff/controller";
import { $ } from "./dom";
import { escapeHtml } from "./escape";
import { catchUpTimeline, parkTimelineRead } from "./follow";
import { partitionAttentionTiers, renderAttention } from "./lenses/attention";
import { renderRevisionList } from "./lenses/revisions";
import {
  renderTimeline,
  scrollTimelineSelectionIntoView,
} from "./lenses/timeline";
import { presentTypes } from "./model";
import { parseSearchQueryFor } from "./query";
import { shortId } from "./refs";
import { clearRouteDiagnostic, navigate, showRouteDiagnostic } from "./router";
import { commit, getState } from "./store";
import {
  EVENT_QUERY_FIELDS,
  type QueryDiagnostic,
  type QuerySurface,
  REVISION_QUERY_FIELDS,
  typeColor,
  typeLabel,
} from "./types";
import { copyWorkflowCommand } from "./workflow-handoff";

const INSPECTOR_TITLE = "Pointbreak Review";

// One load diagnostic (the narrowed shape the diagnostics list reads off the
// `unknown[]` the store holds).
interface Diagnostic {
  code?: string;
  message?: string;
}

// The master lens whose body scaffold is currently mounted, so the scaffold is
// rebuilt only on a lens change (the populate runs every render). Transient
// view-cache — never on the store.
let lastMasterLens: string | null = null;
let lastSelectionScrollKey: string | null = null;

// ---------------------------------------------------------------------------
// Data-driven surfaces (stats, diagnostics, type facets)
// ---------------------------------------------------------------------------

/**
 * Paint the top-bar repo/store identity and the browser tab `<title>` (issue #391).
 * Static per session; the sole writer of `#store-identity` and `document.title`. Left
 * neutral until the one-shot identity fetch lands.
 */
function renderIdentity(): void {
  const root = $("#store-identity");
  if (!root) return;
  const id = getState().identity;
  if (!id) {
    root.classList.remove("hidden");
    const repoEl = $("#store-chip-repo");
    if (repoEl) repoEl.textContent = "local server";
    $("#store-chip")?.setAttribute("aria-label", "local review server");
    document.title = INSPECTOR_TITLE;
    return;
  }
  root.classList.remove("hidden");

  // The detail popover's identity rows: repository + store always, family/worktree
  // only when set. The stat rows and the trust note below them are static markup
  // (renderStats owns #stat-*); this fills only the identity rows, the chip's repo
  // label + accessible name, and the tab title.
  const rows: Array<[string, string]> = [
    ["repository", id.repository],
    ["store", id.placement.label],
  ];
  if (id.family) rows.push(["family", id.family.id]);
  if (id.worktree) rows.push(["worktree", id.worktree]);

  const rowsEl = $("#store-identity-rows");
  if (rowsEl) {
    rowsEl.innerHTML = rows
      .map(([k, v]) => `<dt>${escapeHtml(k)}</dt><dd>${escapeHtml(v)}</dd>`)
      .join("");
  }
  const repoEl = $("#store-chip-repo");
  if (repoEl) repoEl.textContent = id.repository;
  // The chip's accessible label carries the full identity, so a screen reader gets
  // everything without depending on the visual (aria-hidden) popover.
  $("#store-chip")?.setAttribute(
    "aria-label",
    rows.map(([k, v]) => `${k} ${v}`).join(", "),
  );

  // Plain-text title (no HTML escaping needed for document.title).
  document.title = `${id.repository} · ${INSPECTOR_TITLE}`;
}

/** Paint the topbar stat row from the loaded document counts and the history event-set hash. */
function renderStats(): void {
  const h = getState().history;
  const r = getState().revisions;
  const o = getState().threads;
  const events = $("#stat-events");
  if (events) events.textContent = `${h?.eventCount ?? "—"} events`;
  const units = $("#stat-units");
  if (units) units.textContent = `${r?.revisionCount ?? "—"} units`;
  const threads = $("#stat-threads");
  if (threads) threads.textContent = `${o?.threadCount ?? "—"} threads`;
  const hash = $("#stat-hash");
  if (hash) hash.textContent = shortId(h?.eventSetHash);
}

/** Show the load diagnostics banner when there are any, else hide it. */
function renderDiagnostics(): void {
  const el = $("#diagnostics");
  if (!el) return;
  const diags = getState().history?.diagnostics ?? [];
  if (!diags.length) {
    el.classList.add("hidden");
    el.innerHTML = "";
    return;
  }
  el.classList.remove("hidden");
  el.innerHTML = diags
    .map((raw) => {
      const d = (raw ?? {}) as Diagnostic;
      return `<div><span class="${CLASS.code}">${escapeHtml(d.code || "diagnostic")}</span>${escapeHtml(d.message || "")}</div>`;
    })
    .join("");
}

/** Paint the per-type facet rows inside the Filters panel. The whole section is
 * Timeline-lens-only visible — `type:` is an event-surface-only qualifier, so
 * it has nothing to filter on the list/attention lenses. */
function renderTypeToggles(): void {
  const container = $("#filter-types");
  const menu = $("#filter-types-menu");
  if (!container || !menu) return;
  // Only the timeline consumes enabledTypes — leaving this visible on the
  // revision list would let a click silently mutate the timeline's ?type=
  // with no visible effect where the click happened.
  const visible = getState().lens === "timeline";
  container.classList.toggle("hidden", !visible);
  if (!visible) return;
  menu.innerHTML = "";
  // Server-computed facet counts: how many events each type contributes under the
  // rest of the current query (the row distribution, excluding the type filter).
  const counts = getState().history?.facets ?? {};
  const state = getState();
  const present = presentTypes();
  for (const id of present) {
    // Default a newly-seen type (e.g. an unknown event type) to enabled once;
    // after that the user's toggle sticks instead of being re-enabled here. This
    // is the served default-seeding (a transient set mutation, not a commit, so it
    // triggers no repaint).
    if (!state.seenTypes.has(id)) {
      state.seenTypes.add(id);
      state.enabledTypes.add(id);
    }
    const enabled = state.enabledTypes.has(id);
    const count = counts[id] ?? 0;
    const li = document.createElement("li");
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = typeFacetRowClass(enabled);
    // The delegated #filter-types handler reads the toggled type off this dataset.
    btn.dataset.type = id;
    btn.setAttribute("aria-pressed", String(enabled));
    btn.setAttribute(
      "aria-label",
      `${enabled ? "Hide" : "Show"} ${typeLabel(id)} events (${count})`,
    );
    btn.innerHTML = `<span class="${CLASS.dot}" style="background:${typeColor(id)}"></span>${escapeHtml(typeLabel(id))}<span class="${CLASS.typeCount}">${count}</span>`;
    btn.title = id;
    li.appendChild(btn);
    menu.appendChild(li);
  }
}

// ---------------------------------------------------------------------------
// Lens switcher + master pane + selection
// ---------------------------------------------------------------------------

/** Reflect the active lens onto the lens-switcher tabs' pressed state. */
function renderLensSwitcher(): void {
  const lens = getState().lens;
  for (const tab of document.querySelectorAll<HTMLElement>(".lens-tab")) {
    tab.setAttribute("aria-pressed", String(tab.dataset.lens === lens));
    if (tab.dataset.lens === "attention") renderAttentionBadge(tab);
  }
}

// The attention tab's count badge: the store-wide judgment queue, glanceable
// from every lens (the per-revision view lives on the detail page). The
// needs-input count is the number; the advisory count rides muted beside it.
// Both zero removes the element — an honest empty state, never a "0" chip. The
// counts are derived from the served items, so they fall (or rise) on their own
// and the badge follows on the same repaint; there is no dismissal affordance.
/** Reconcile the judgment-queue count badge inside the attention tab. */
function renderAttentionBadge(tab: HTMLElement): void {
  const { primary, secondary } = partitionAttentionTiers(
    getState().attention?.items ?? [],
  );
  tab.querySelector(`.${CLASS.attentionBadge}`)?.remove();
  tab.querySelector(`.${CLASS.attentionDelta}`)?.remove();
  if (primary.length || secondary.length) {
    const badge = document.createElement("span");
    badge.className = CLASS.attentionBadge;
    // The aria-label replaces the badge's text in the tab's accessible name, so
    // the counts are announced with their meaning, not read as bare digits.
    const needsInput =
      primary.length === 1
        ? "1 item needs input"
        : `${primary.length} items need input`;
    badge.setAttribute(
      "aria-label",
      [
        primary.length ? needsInput : "",
        secondary.length ? `${secondary.length} advisory` : "",
      ]
        .filter(Boolean)
        .join(", "),
    );
    badge.innerHTML = `${primary.length ? primary.length : ""}${secondary.length ? `<span class="${CLASS.attentionBadgeSecondary}">+${secondary.length}</span>` : ""}`;
    tab.appendChild(badge);
  }

  const delta = getState().attentionDelta;
  if (delta == null || delta === 0) return;
  const chip = document.createElement("span");
  chip.className = CLASS.attentionDelta;
  chip.textContent = `changed ${delta > 0 ? `+${delta}` : `−${Math.abs(delta)}`}`;
  chip.setAttribute("role", "status");
  tab.appendChild(chip);
}

/** Project explicit stream intent onto the timeline toolbar. */
function syncStreamPositionControls(): void {
  const state = getState();
  const follow = $("#follow-toggle");
  if (follow) {
    follow.classList.toggle(
      "hidden",
      state.lens !== "timeline" || state.order !== "desc",
    );
    follow.setAttribute("aria-pressed", String(state.followByLens.timeline));
    follow.textContent = state.followByLens.timeline ? "Following" : "Follow";
  }
}

/** Project unseen arrivals onto the timeline-local catch-up row. */
function syncTimelineNewPill(): void {
  const state = getState();
  const pill = $("#timeline-new-pill");
  if (!pill) return;
  const visible =
    state.order === "desc" &&
    state.followByLens.timeline &&
    state.timelineHeadAnchor != null &&
    state.timelineNewCount > 0;
  pill.classList.toggle("hidden", !visible);
  pill.textContent = `Show ${state.timelineNewCount} new ${state.timelineNewCount === 1 ? "event" : "events"}`;
}

// Reflect the current filter/order/sort state into the toolbar controls (the
// option lists are rebuilt only on load, so a navigation that changes a filter
// syncs the displayed value here). Each control is gated to the lens(es) that
// actually consume its state — a control visible where nothing reads it would
// let a click silently mutate another lens's view.
/** Sync the toolbar's control values and their per-lens visibility. */
function syncControls(): void {
  const state = getState();
  const text = $<HTMLInputElement>("#filter-text");
  if (text && text.value !== state.filterText) text.value = state.filterText;
  if (text)
    text.placeholder = `search — text or field:value (${keysFor(state.lens)
      .map((k) => `${k}:`)
      .join(" ")})`;

  const onAttention = state.lens === "attention";
  const viewToggle = $("#view-toggle");
  if (viewToggle) {
    viewToggle.textContent = onAttention
      ? "View"
      : `View · ${state.order === "desc" ? "newest" : "oldest"}`;
  }
  $("#view-order-section")?.classList.toggle("hidden", onAttention);
  const newest = $<HTMLInputElement>("#order-newest");
  const oldest = $<HTMLInputElement>("#order-oldest");
  if (newest) newest.checked = state.order === "desc";
  if (oldest) oldest.checked = state.order === "asc";

  // The sort-key picker belongs to the revision list alone; the timeline's
  // order is server-applied and the attention queue's order is fixed.
  const onList = state.lens === "list";
  $("#view-sort-section")?.classList.toggle("hidden", !onList);
  const picker = $<HTMLSelectElement>("#sort-picker");
  if (picker) {
    if (picker.value !== state.sortKey) picker.value = state.sortKey;
  }

  // The toolbar shell's own gate is coarser (hidden only on the attention lens,
  // which consumes none of these controls) and sits underneath the per-control
  // gates above — necessary but not sufficient.
  const toolbar = $("#toolbar");
  if (toolbar) toolbar.classList.toggle("hidden", onAttention);
}

/** The advertised qualifier set for a lens's search surface. */
function keysFor(lens: string): readonly string[] {
  return lens === "list" ? REVISION_QUERY_FIELDS : EVENT_QUERY_FIELDS;
}

// The surface `filterText` parses against for the active lens: the list lens
// filters client-side over revisions; every other lens (including attention,
// which has no filterText affordance of its own — the toolbar hides there)
// defaults to the event surface.
function currentQuerySurface(): QuerySurface {
  return getState().lens === "list" ? "revision" : "event";
}

/** Paint the applied-filter chips: one per parsed qualifier clause in the
 * active filterText, for the active lens's surface. A pure view — chips carry
 * no state of their own; removing one re-derives filterText from the
 * remaining tokens (see chips.ts). */
function renderFilterChips(): void {
  const container = $("#filter-chips");
  if (!container) return;
  const chips = filterChipsFor(getState().filterText, currentQuerySurface());
  const rendered = chips.map((c) => {
    // The parser canonicalizes actor values to the stored full id; labeling
    // that after the `actor:` key would double the scheme prefix, so the
    // label uses the short spelling (the form the actor-ref click mints).
    const value =
      c.field === "actor" ? c.value.replace(/^actor:/, "") : c.value;
    const label = `${escapeHtml(c.field)}:${escapeHtml(value)}`;
    return (
      `<span class="${filterChipClass(c.negate)}" data-token-index="${c.tokenIndex}">` +
      `${c.negate ? "− " : ""}${label}` +
      `<button type="button" class="${CLASS.filterChipRemove}" data-token-index="${c.tokenIndex}" aria-label="remove ${label} filter">✕</button>` +
      `</span>`
    );
  });
  const state = getState();
  for (const [scope, value] of [
    ["track", state.filterTrack],
    ["snapshot", state.filterSnapshot],
  ] as const) {
    if (!value) continue;
    const label = `${escapeHtml(scope)}:${escapeHtml(value)}`;
    rendered.push(
      `<span class="${filterChipClass(false)}" data-filter-scope="${scope}">` +
        `${label}` +
        `<button type="button" class="${CLASS.filterChipRemove}" data-filter-scope="${scope}" aria-label="remove ${label} filter">✕</button>` +
        `</span>`,
    );
  }
  container.innerHTML = rendered.join("");
  $("#filter-chips-empty")?.classList.toggle("hidden", rendered.length > 0);
}

/** Summarize the panel's active state on its compact trigger and footer. */
function syncFilterControls(): void {
  const state = getState();
  const structuredCount = filterChipsFor(
    state.filterText,
    currentQuerySurface(),
  ).length;
  const scopedCount =
    Number(Boolean(state.filterTrack)) + Number(Boolean(state.filterSnapshot));
  const present = presentTypes();
  const typesFiltered =
    state.lens === "timeline" &&
    present.some((type) => !state.enabledTypes.has(type));
  const count = structuredCount + scopedCount + Number(typesFiltered);
  const toggle = $("#filters-toggle");
  if (toggle) {
    toggle.textContent = count > 0 ? `Filters · ${count}` : "Filters";
    toggle.setAttribute(
      "aria-label",
      count > 0 ? `Filters — ${count} active` : "Filters — none active",
    );
  }
  const clearable =
    Boolean(state.filterText.trim()) ||
    Boolean(state.filterTrack) ||
    Boolean(state.filterSnapshot) ||
    typesFiltered;
  $("#filter-footer")?.classList.toggle("hidden", !clearable);
}

// The exact string this module last wrote to the region; "" when it wrote nothing.
// Ownership is by CONTENT: if the region no longer holds our string, a router write
// owns it and we must not touch it.
let lastQueryNotice = "";

// Surface the active query's parse diagnostics (client parse + server
// queryNotices, deduped) in the route-diagnostic live region without clobbering
// a router-owned message — route diagnostics are higher priority.
function syncQueryNotices(): void {
  const el = $("#route-diagnostic");
  if (!el) return;
  const state = getState();
  const parsed = parseSearchQueryFor(state.filterText, currentQuerySurface());
  const server = (state.history?.queryNotices ?? []) as QueryDiagnostic[];
  const message = dedupeNotices([...parsed.diagnostics, ...server])
    .map((d) => d.message)
    .join(" · ");

  const current = el.classList.contains("hidden") ? "" : (el.textContent ?? "");
  // A router (or any non-us) write owns the region — never clobber it.
  if (current !== "" && current !== lastQueryNotice) return;
  if (message) {
    showRouteDiagnostic(message);
    lastQueryNotice = message;
  } else if (lastQueryNotice) {
    clearRouteDiagnostic();
    lastQueryNotice = "";
  }
}

/** Collapse notices identical in (code, key, message) — a client hint and its
 *  server twin render once. */
function dedupeNotices(notices: QueryDiagnostic[]): QueryDiagnostic[] {
  const seen = new Set<string>();
  return notices.filter((n) => {
    const key = `${n.code}\u0000${n.key}\u0000${n.message}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

// Master pane: swap in the active lens body and populate it. The scaffold is
// rebuilt only on a lens change; the populate runs every render so the lens
// reflects the current filters/selection.
/** Mount the active lens body scaffold (on a lens change) and populate it. */
function renderMaster(): void {
  const master = $("#master");
  if (!master) return;
  const lens = getState().lens;
  if (lens !== lastMasterLens) {
    lastMasterLens = lens;
    if (lens === "list") {
      master.innerHTML = `<div id="units" class="${CLASS.units}"></div>`;
    } else if (lens === "attention") {
      master.innerHTML = `<div id="attention" class="${CLASS.units}" aria-label="attention"></div>`;
    } else {
      master.innerHTML = `<div class="${CLASS.timelineShell}"><button id="timeline-new-pill" class="ghost ${CLASS.timelineNewPill} hidden" type="button" aria-live="polite">Show 0 new events</button><ol id="timeline" class="${CLASS.timeline}" aria-label="event timeline" tabindex="0"></ol></div>`;
    }
  }
  if (lens === "list") renderRevisionList();
  else if (lens === "attention") renderAttention();
  else {
    syncTimelineNewPill();
    renderTimeline();
  }
}

// Project the open state onto the split grid: `split-closed` is a string-literal
// state class (the `hidden` precedent, not a CLASS entry) the stylesheet keys the
// single-column collapse — and, at narrow widths, the slide-over sheet — off.
/** Toggle the split grid's mode classes from the open/reading state. */
function applySplitMode(): void {
  const split = $(".split");
  if (!split) return;
  const s = getState();
  split.classList.toggle("split-closed", !s.open);
  const reading = s.reading && s.open;
  split.classList.toggle("reading", reading);
  // The reading toggle presents its own state: ⤢ enters, ⤡ restores.
  const readBtn = $("#detail-read");
  if (readBtn) {
    readBtn.textContent = reading ? "⤡" : "⤢";
    const label = reading ? "Restore split" : "Reading mode";
    readBtn.setAttribute("aria-label", label);
    readBtn.setAttribute("title", label);
  }
}

// Detail pane: a pure projection of the single selection — the event detail or
// the revision composite (fetched on demand by detail). A closed pane paints
// nothing and, for a revision cursor, must not fetch the composite.
/** Paint the detail pane from the open selection (delegating to detail). */
function renderSelected(): void {
  if (!getState().open) return;
  const sel = getState().selected;
  if (sel.kind === "revision" && sel.id) void showComposite(sel.id);
  else renderDetail();
}

// Scroll the selected entry into view within the master pane (the timeline row,
// the list card, or the DAG node), so cursor stepping keeps the selection visible.
// The timeline is virtualized, so an off-screen event row is not in the DOM; event
// selection routes through the timeline's index-scroller, which scrolls the row's
// index into the virtual window and repaints before scrolling it into view. This is
// the single materialization point — route-state, deep-link, and keyboard selection
// all reach off-screen rows through here.
/** Scroll the selected master entry into view, if any. */
function scrollSelectionIntoView(): void {
  const sel = getState().selected;
  if (!sel.id) return;
  if (sel.kind === "event") {
    scrollTimelineSelectionIntoView(sel.id);
    return;
  }
  const master = $("#master");
  if (!master) return;
  const el = master.querySelector(`[data-revision-id="${sel.id}"]`);
  if (el) el.scrollIntoView({ block: "center" });
}

// Selection movement is an explicit navigation effect, not a consequence of
// every store repaint. Include presence so an off-window deep link scrolls once
// its reveal page arrives, while background refreshes of a visible selection do
// not recenter the reader.
function selectionScrollKey(): string {
  const state = getState();
  const selected = state.selected;
  if (!selected.id) return `${state.lens}:none`;
  let present = false;
  if (selected.kind === "event") {
    present = (state.history?.entries ?? []).some(
      (entry) => entry.eventId === selected.id,
    );
  } else if (selected.kind === "revision") {
    present = (state.revisions?.entries ?? []).some(
      (revision) => revision.revisionId === selected.id,
    );
  }
  return `${state.lens}:${selected.kind}:${selected.id}:${present ? "present" : "absent"}`;
}

function scrollChangedSelectionIntoView(): void {
  const key = selectionScrollKey();
  if (key === lastSelectionScrollKey) return;
  lastSelectionScrollKey = key;
  scrollSelectionIntoView();
}

// ---------------------------------------------------------------------------
// The single render entry (the store subscriber)
// ---------------------------------------------------------------------------

// Project the routed diff page's frame ownership: while `diffPage` is set the
// page shows and the lens layout (toolbar + master-detail split) hides — no
// lens renders underneath. Off the page every piece is restored (syncControls
// then re-owns the toolbar's lens-conditional visibility). Returns whether the
// page owns this frame.
function applyDiffPageMode(): boolean {
  const onPage = getState().diffPage;
  $("#diff-page")?.classList.toggle("hidden", !onPage);
  for (const sel of [
    "#toolbar",
    "#master",
    "#detail",
    "#master-rail",
    ".divider",
  ]) {
    $(sel)?.classList.toggle("hidden", onPage);
  }
  return onPage;
}

/**
 * The single render entry: project the whole view from state. Registered once in
 * the composition root as the only `subscribe()` callback, so every `commit`
 * (navigate, load, freshness poll) repaints through here. While the diff page
 * owns the frame only the topbar surfaces repaint above it; otherwise the
 * page reconciler still runs (and resets) so a closed page repaints fresh on
 * its next open. The returned promise is ignored (app.js parity).
 */
export function render(): void {
  renderIdentity();
  renderStats();
  renderDiagnostics();
  renderLensSwitcher();
  syncStreamPositionControls();
  if (applyDiffPageMode()) {
    void renderDiffPage();
    return;
  }
  syncControls();
  syncQueryNotices();
  renderFilterChips();
  renderTypeToggles();
  syncFilterControls();
  applySplitMode();
  renderMaster();
  renderSelected();
  scrollChangedSelectionIntoView();
  void renderDiffPage();
}

// ---------------------------------------------------------------------------
// Delegated controls (installed once by the composition root)
// ---------------------------------------------------------------------------

// Toggle a present type in/out of the enabled set and navigate (replace), so the
// route is the single source of truth for the filter. The delegate, not render,
// owns this commit.
function onTypeToggleClick(ev: Event): void {
  const t = ev.target;
  if (!(t instanceof Element)) return;
  const btn = t.closest<HTMLElement>("[data-type]");
  const id = btn?.dataset.type;
  if (!id) return;
  const types = new Set(getState().enabledTypes);
  if (types.has(id)) types.delete(id);
  else types.add(id);
  navigate({ enabledTypes: types }, { replace: true });
}

// Delete the removed chip's raw token from filterText and navigate (replace) —
// the chip row is a pure view, so the route stays the single source of truth.
function onFilterChipsClick(ev: Event): void {
  const t = ev.target;
  if (!(t instanceof Element)) return;
  const btn = t.closest<HTMLElement>(`.${CLASS.filterChipRemove}`);
  const scope = btn?.dataset.filterScope;
  if (scope === "track") {
    navigate({ filterTrack: "" }, { replace: true });
    return;
  }
  if (scope === "snapshot") {
    navigate({ filterSnapshot: "" }, { replace: true });
    return;
  }
  const indexAttr = btn?.dataset.tokenIndex;
  if (indexAttr == null) return;
  const next = removeFilterChipToken(getState().filterText, Number(indexAttr));
  navigate({ filterText: next }, { replace: true });
}

// The single master-pane delegate (replacing the served per-row / per-card
// listeners). Order matters: ref chips fall through to the navigation delegate
// first; the attention-cue and open-diff affordances are handled before card
// selection (replacing the served stopPropagation). Revision selection is scoped
// to the list `.unit-card` so other `data-revision-id` carriers (badges, chips)
// never double-navigate.
function onMasterClick(ev: Event): void {
  const t = ev.target;
  if (!(t instanceof Element)) return;
  if (t.closest("#timeline-new-pill")) {
    void catchUpTimeline();
    return;
  }
  if (t.closest("[data-ref-kind]")) return;
  // Command handoffs are handled before card selection: the copy button writes
  // only to the clipboard, and any other click inside the block stays inert so
  // the command text is selectable without navigating away.
  const workflowCopyBtn = t.closest<HTMLElement>(
    "[data-copy-workflow-command]",
  );
  if (workflowCopyBtn) {
    void copyWorkflowCommand(workflowCopyBtn);
    return;
  }
  if (t.closest("[data-workflow-handoff]")) return;
  const cue = t.closest<HTMLElement>("[data-attention-query]");
  if (cue) {
    const query = cue.dataset.attentionQuery;
    if (query) navigate({ filterText: query });
    return;
  }
  const diffBtn = t.closest<HTMLElement>("[data-open-diff]");
  if (diffBtn) {
    const snapshotId = diffBtn.dataset.openDiff;
    if (snapshotId)
      openDiff(snapshotId, null, diffBtn.dataset.diffHash || null);
    return;
  }
  // A mouse click both parks the cursor and opens the detail (today's feel);
  // keyboard stepping, by contrast, leaves `open` unchanged.
  const eventEl = t.closest<HTMLElement>("[data-event-id]");
  if (eventEl) {
    const id = eventEl.dataset.eventId;
    if (id) {
      parkTimelineRead();
      navigate({ selected: { kind: "event", id }, open: true });
    }
    return;
  }
  const revEl = t.closest<HTMLElement>(".unit-card[data-revision-id]");
  if (revEl) {
    const id = revEl.dataset.revisionId;
    if (id) navigate({ selected: { kind: "revision", id }, open: true });
  }
}

/**
 * Install the delegated master-pane and filter handlers — once, on the stable
 * `#master` / `#filter-types` containers (their innerHTML is repainted per render,
 * but the containers persist). Called by the composition root, never per render.
 */
export function initControls(): void {
  $<HTMLElement>("#master")?.addEventListener("click", onMasterClick);
  $<HTMLElement>("#filter-types")?.addEventListener("click", onTypeToggleClick);
  $<HTMLElement>("#filter-chips")?.addEventListener(
    "click",
    onFilterChipsClick,
  );
  // The pane's close control: closing is deselection of the *pane*, not the
  // cursor — the selection survives so Enter/j/k resume from it.
  $<HTMLElement>("#detail-close")?.addEventListener("click", () =>
    navigate({ open: false }),
  );
  // The narrow-viewport sheet's back affordance — the same close rung with a
  // different face; CSS hides it at wide widths (no TS knows the breakpoint).
  $<HTMLElement>("#detail-back")?.addEventListener("click", () =>
    navigate({ open: false }),
  );
  // Reading mode toggles through commit, never navigate — it is session-only
  // state (not URL), and a navigate would push a junk history entry.
  $<HTMLElement>("#detail-read")?.addEventListener("click", () =>
    commit({ reading: !getState().reading }),
  );
  $<HTMLElement>("#master-rail")?.addEventListener("click", () =>
    commit({ reading: false }),
  );
}
