// The data-loading layer: fetch the `/api/*` documents and commit them to the
// store. Ported from the served app.js `load` / `pollFreshness` / `showError`.
//
// The server owns the history query now: full reloads fetch only the first page of
// the history for the store's current filter/search/order, and query-only reloads
// fetch just that history page. The server returns the windowed entries plus the
// toggle `facets`, the total `matchCount`, and the window `offset`. Paging is
// positional (`offset`/`at`); there is no opaque cursor. The loader no longer
// builds a client search index — there is nothing to index before commit.
//
// The cycle cut (data must not depend on render): `load` only `commit`s; it never
// calls a render function. The store subscriber repaints in response. The
// query-change watcher (`maybeReloadForQuery`) is a store subscriber wired once in
// the composition root, so a filter/search/order change re-fetches page 1 without
// the controls having to reach the network themselves.

import { setRefreshState } from "./connection";
import { $ } from "./dom";
import {
  resetTimelineFollowForQueryChange,
  timelineFollowGeneration,
} from "./follow";
import { fetchJSON } from "./http";
import { presentTypes } from "./model";
import {
  type AttentionDoc,
  commit,
  getState,
  type HistoryDoc,
  type IdentityDoc,
  type RevisionsDoc,
  type State,
  type ThreadsDoc,
} from "./store";
import type { HistoryEntry } from "./types";

// The `/api/freshness` probe: the event-log head marker (the event count) and
// the commit-graph stamp (the git ref state merge statuses read, #467) the
// poller diffs against the stored baselines. Not store state (only its deltas
// are committed, via a reload).
interface FreshnessDoc {
  eventCount?: number;
  commitGraphStamp?: string;
}

interface NewCountDoc {
  newCount?: number;
}

/** The first-page size — large enough to fill a viewport, small enough to keep the transfer cheap. */
export const HISTORY_PAGE = 100;

/**
 * Build the `/api/history` query string from the current filter/search/order/type
 * state. Mirrors `serializeState`'s param choices: `q`/`track`/`object` when set, a
 * non-default `order`, and a `type=` CSV only when some present type is disabled
 * (absent ⇒ the server matches all types). Always caps the page with `limit`.
 */
export function historyQueryParams(s: State): string {
  const p = new URLSearchParams();
  if (s.filterText) p.set("q", s.filterText);
  if (s.filterTrack) p.set("track", s.filterTrack);
  // Writes `snapshot`; the server still accepts legacy `object` (#334 transition).
  if (s.filterSnapshot) p.set("snapshot", s.filterSnapshot);
  if (s.order && s.order !== "asc") p.set("order", s.order);
  const present = presentTypes();
  if (present.some((id) => !s.enabledTypes.has(id))) {
    p.set("type", present.filter((id) => s.enabledTypes.has(id)).join(","));
  }
  p.set("limit", String(HISTORY_PAGE));
  return p.toString();
}

/**
 * Probe the filter-aware count newer than the frozen parked-head anchor. Ascending
 * timelines and anchorless parked windows preserve their window without a probe.
 */
export async function probeNewCount(): Promise<void> {
  const s = getState();
  const generation = timelineFollowGeneration();
  const anchor = s.timelineHeadAnchor;
  const queryKey = historyQueryParams(s);
  if (s.followByLens.timeline || s.order !== "desc" || !anchor) return;

  const params = new URLSearchParams(queryKey);
  params.delete("limit");
  params.delete("offset");
  params.delete("at");
  params.set("sinceOccurredAt", anchor.occurredAt);
  params.set("sinceEventId", anchor.eventId);
  const doc = (await fetchJSON(
    `/api/history/new-count?${params.toString()}`,
  )) as NewCountDoc;
  const current = getState();
  const currentAnchor = current.timelineHeadAnchor;
  if (
    timelineFollowGeneration() !== generation ||
    current.followByLens.timeline ||
    current.order !== "desc" ||
    historyQueryParams(current) !== queryKey ||
    currentAnchor?.occurredAt !== anchor.occurredAt ||
    currentAnchor?.eventId !== anchor.eventId
  )
    return;
  commit({ timelineNewCount: doc.newCount ?? 0 });
}

/**
 * Show `message` in `#error` (prefixed), or hide and clear the banner when given
 * no message.
 */
export function showError(message: string | null): void {
  const el = $("#error");
  if (!el) return;
  if (!message) {
    el.classList.add("hidden");
    el.textContent = "";
    return;
  }
  el.textContent = `error: ${message}`;
  el.classList.remove("hidden");
}

function showLoadError(err: unknown): void {
  showError(err instanceof Error ? err.message : String(err));
}

function commitFreshnessBaseline(freshness: FreshnessDoc): void {
  commit({
    lastEventCount: freshness.eventCount ?? null,
    lastCommitGraphStamp: freshness.commitGraphStamp ?? null,
  });
}

/**
 * Load page 1 of the history for the current query and commit it with the
 * freshness baseline. Resolves true only after the page has been committed.
 */
export async function loadHistoryHead(
  isCurrent: () => boolean = () => true,
): Promise<boolean> {
  try {
    const params = historyQueryParams(getState());
    // Take the freshness marker BEFORE the documents, so the baseline can never be
    // newer than what was loaded. If an event lands during the document fetch, the
    // marker the next poll reads is higher than this baseline and triggers a reload
    // (at worst a redundant one) rather than masking the append. Fetched in parallel
    // with the documents, the marker could come back newer than them and the poll
    // would then report "unchanged" forever. Seeding from the probe (not
    // `history.eventCount`) also keeps a retired/skipped event — where the event-file
    // marker exceeds the post-skip history count — from forcing a reload every tick.
    const freshness = (await fetchJSON("/api/freshness")) as FreshnessDoc;
    const historyRaw = await fetchJSON(`/api/history?${params}`);
    if (!isCurrent()) return false;
    showError(null);
    commit({
      history: { ...(historyRaw as HistoryDoc), queryKey: params },
    });
    commitFreshnessBaseline(freshness);
    return true;
  } catch (err) {
    showLoadError(err);
    return false;
  }
}

/** Fetch and commit the full revisions, threads, and attention documents. */
export async function loadWholeDocuments(): Promise<boolean> {
  try {
    const previousAttentionCount = getState().attention?.items?.length;
    const revisionsScrollTop = $<HTMLElement>("#units")?.scrollTop;
    const [revisionsRaw, threadsRaw, attentionRaw] = await Promise.all([
      fetchJSON("/api/revisions"),
      fetchJSON("/api/threads"),
      fetchJSON("/api/attention"),
    ]);
    const attention = attentionRaw as AttentionDoc;
    showError(null);
    commit({
      revisions: revisionsRaw as RevisionsDoc,
      threads: threadsRaw as ThreadsDoc,
      attention,
      attentionDelta:
        previousAttentionCount == null
          ? null
          : attention.items.length - previousAttentionCount,
    });
    if (revisionsScrollTop != null) {
      const units = $<HTMLElement>("#units");
      if (units) units.scrollTop = revisionsScrollTop;
    }
    return true;
  } catch (err) {
    showLoadError(err);
    return false;
  }
}

/**
 * Bootstrap both loader halves in their existing order. Never calls render — the
 * store subscriber repaints. A load failure surfaces in `#error` rather than throwing.
 */
export async function load(): Promise<boolean> {
  if (!(await loadHistoryHead())) return false;
  return loadWholeDocuments();
}

/**
 * Fetch the static per-session repo/store identity once and commit it. Best-effort
 * chrome (issue #391): a failure leaves `identity` null and is swallowed — it must
 * not surface in `#error` or block the app. Never called on the reload path.
 */
export async function loadIdentity(): Promise<void> {
  try {
    const doc = (await fetchJSON("/api/identity")) as IdentityDoc;
    commit({ identity: doc });
  } catch {
    // Non-fatal: the identity cue is optional; leave `identity` null.
  }
}

// A single-flight guard so the query watcher never re-enters while its own reload
// is in flight (the reload commits a fresh `history` whose `queryKey` matches the
// active query, which is what stops the loop).
let reloading = false;

async function reloadHistoryForQuery(): Promise<boolean> {
  const queryKey = historyQueryParams(getState());
  const doc = await fetchHistoryDoc(`/api/history?${queryKey}`);
  if (!doc) return false;
  showError(null);
  commitHistoryPage(doc, queryKey);
  resetTimelineFollowForQueryChange();
  return true;
}

/**
 * Fetch page 1 when the active query no longer matches the loaded page's query
 * key. A store subscriber: the toolbar/search/type-toggle/order controls only
 * mutate state, so this watcher is what turns a query change into a server fetch.
 * Loop-safe by the in-flight guard plus the query-key equality check.
 */
export function maybeReloadForQuery(): void {
  const s = getState();
  const want = historyQueryParams(s);
  if (reloading || !s.history || s.history.queryKey === want) return;
  reloading = true;
  void reloadHistoryForQuery()
    .then((reloaded) => {
      reloading = false;
      // A later query change may have arrived while this fetch was in flight. The
      // commit was ignored by the re-entry guard above, so check once more now.
      if (reloaded) maybeReloadForQuery();
    })
    .catch(() => {
      reloading = false;
    });
}

// ---------------------------------------------------------------------------
// Incremental page fetching (scroll-extend / reveal / keyboard paging)
// ---------------------------------------------------------------------------

/** A page selector: extend the loaded window forward/back by `offset`. */
export interface HistoryPageSelector {
  offset?: number;
}

// In-flight page fetches keyed by URL, so concurrent identical requests share one
// promise (a scroll fires renderTimeline repeatedly) and an awaiter still receives
// the resolved result rather than a dropped no-op.
const pageFetches = new Map<string, Promise<void>>();

/** Build the `/api/history` URL for a page selector under the active query. */
function pageUrl(s: State, selector: HistoryPageSelector): string {
  const params = new URLSearchParams(historyQueryParams(s));
  if (selector.offset != null) params.set("offset", String(selector.offset));
  return `/api/history?${params}`;
}

// Fetch a history doc without committing — the shared fetch for the page-fetch and
// reveal paths. A failure surfaces in `#error` and yields null.
async function fetchHistoryDoc(url: string): Promise<HistoryDoc | null> {
  try {
    return (await fetchJSON(url)) as HistoryDoc;
  } catch (err) {
    showError(err instanceof Error ? err.message : String(err));
    return null;
  }
}

// Merge a fetched page into the loaded window. Contiguous or overlapping windows
// union (page entries win on overlap); a disjoint page (e.g. a reveal jumped far
// away) replaces the window. Returns the merged entries and their global offset.
function mergeWindows(
  prev: HistoryDoc,
  page: HistoryDoc,
): { entries: HistoryEntry[]; offset: number } {
  const prevOffset = prev.offset ?? 0;
  const prevEntries = prev.entries ?? [];
  const prevEnd = prevOffset + prevEntries.length;
  const pageOffset = page.offset ?? 0;
  const pageEntries = page.entries ?? [];
  const pageEnd = pageOffset + pageEntries.length;
  if (pageOffset > prevEnd || pageEnd < prevOffset) {
    return { entries: pageEntries, offset: pageOffset };
  }
  const offset = Math.min(prevOffset, pageOffset);
  const end = Math.max(prevEnd, pageEnd);
  const entries: HistoryEntry[] = [];
  for (let g = offset; g < end; g++) {
    entries.push(
      g >= pageOffset && g < pageEnd
        ? pageEntries[g - pageOffset]
        : prevEntries[g - prevOffset],
    );
  }
  return { entries, offset };
}

// Commit a fetched page: merge it into the loaded window when the active query is
// unchanged, else adopt it wholesale (the query moved on since the page was
// requested, e.g. a reveal cleared the filters). Always stamps the current query
// key so the loaded window tracks the active query.
function commitHistoryPage(page: HistoryDoc, requestedQueryKey?: string): void {
  const s = getState();
  const queryKey = requestedQueryKey ?? historyQueryParams(s);
  const prev = s.history;
  const merged =
    prev && prev.queryKey === queryKey
      ? mergeWindows(prev, page)
      : { entries: page.entries ?? [], offset: page.offset ?? 0 };
  const selected =
    s.selected.kind === "event" && s.selected.id ? s.selected.id : null;
  const selectedIsVisible =
    selected != null &&
    merged.entries.some((entry) => entry.eventId === selected);
  const retainedEntry =
    selected != null && !selectedIsVisible
      ? (prev?.entries.find((entry) => entry.eventId === selected) ??
        (prev?.retainedEntry?.eventId === selected
          ? prev.retainedEntry
          : undefined))
      : undefined;
  commit({
    history: {
      ...page,
      entries: merged.entries,
      offset: merged.offset,
      queryKey,
      retainedEntry,
    },
  });
}

/**
 * Fetch one more page of the current query and merge it into the loaded window.
 * The single page-fetch path shared by the scroll-extend, keyboard, and reveal
 * callers. Cycle-safe: it only `commit`s, never renders (the subscriber repaints).
 * A failure surfaces in `#error` rather than throwing.
 */
export function fetchHistoryPage(selector: HistoryPageSelector): Promise<void> {
  const s = getState();
  if (!s.history) return Promise.resolve();
  const url = pageUrl(s, selector);
  const existing = pageFetches.get(url);
  if (existing) return existing;
  const run = fetchHistoryDoc(url)
    .then((doc) => {
      if (doc) commitHistoryPage(doc);
    })
    .finally(() => {
      pageFetches.delete(url);
    });
  pageFetches.set(url, run);
  return run;
}

// ---------------------------------------------------------------------------
// Fetch-to-reveal (deep link / ref chip → an off-page event)
// ---------------------------------------------------------------------------

/**
 * A fetched reveal page: the window doc (query key stamped), whether the target is
 * present on it, and the type set to enable — all present types, so nothing hides
 * the target and the query watcher stays quiet after the reveal commit.
 */
export interface RevealPage {
  doc: HistoryDoc;
  present: boolean;
  enabledTypes: Set<string>;
}

// The reset (unfiltered) query a reveal fetches under — order preserved, no
// q/track/object/type — so the located page is a page of the default timeline view
// and nothing filters the target out.
function resetQuery(order: string): string {
  const params = new URLSearchParams();
  if (order && order !== "asc") params.set("order", order);
  params.set("limit", String(HISTORY_PAGE));
  return params.toString();
}

/**
 * Fetch the page containing `eventId` (via `at=`) under the reset query, so a
 * reveal or deep link can jump to an off-page event. Returns the window doc, whether
 * the target is on the page, and the type set to enable. A pure fetch — the caller
 * commits (navigation pushes a URL; the router reacts to one).
 */
export async function fetchRevealPage(
  eventId: string,
): Promise<RevealPage | null> {
  const s = getState();
  const queryKey = resetQuery(s.order);
  const params = new URLSearchParams(queryKey);
  params.set("at", eventId);
  const doc = await fetchHistoryDoc(`/api/history?${params}`);
  if (!doc) return null;
  const present = (doc.entries ?? []).some((e) => e.eventId === eventId);
  const facetKeys = doc.facets ? Object.keys(doc.facets) : [];
  const enabledTypes = new Set([...s.enabledTypes, ...facetKeys]);
  return { doc: { ...doc, queryKey }, present, enabledTypes };
}

/** The state patch a reveal applies: reset filters, the located window, the type
 * set, and the event selection — shared by the chip reveal and the deep-link path. */
export function revealPatch(page: RevealPage, eventId: string): Partial<State> {
  return {
    lens: "timeline",
    selected: { kind: "event", id: eventId },
    filterText: "",
    filterTrack: "",
    filterSnapshot: "",
    enabledTypes: page.enabledTypes,
    diff: null,
    diffHash: null,
    focus: null,
    history: page.doc,
  };
}

/**
 * Resolve the event id that carries a structured id (observation / assessment /
 * input-request) via a server search, or null when nothing matches. The haystack
 * indexes those ids, so `q=<id>` finds the carrying event.
 */
export async function fetchEventIdForQuery(q: string): Promise<string | null> {
  const params = new URLSearchParams({ q, limit: "1" });
  const doc = await fetchHistoryDoc(`/api/history?${params}`);
  return doc?.entries?.[0]?.eventId ?? null;
}

let pollSettleTimer: ReturnType<typeof setTimeout> | undefined;

/** Clear transient poll chrome without letting a failed reload look healthy. */
function settlePoll(markWatching: boolean): void {
  clearTimeout(pollSettleTimer);
  pollSettleTimer = setTimeout(() => {
    commit({ attentionDelta: null });
    if (markWatching) setRefreshState("watching");
  }, 1200);
}
/**
 * Probe `/api/freshness` and reload when the event-log head marker or the
 * commit-graph stamp changed, updating the liveness indicator. The stamp
 * catches a pure-git landing — a fast-forward flips merge statuses without
 * appending an event (#467). A parked timeline window is preserved so its
 * selected event remains available to the detail pane (#461). A probe failure
 * degrades refresh independently of the classified connection state.
 */
export async function pollFreshness(): Promise<void> {
  let documentsLoaded = false;
  try {
    const f = (await fetchJSON("/api/freshness")) as FreshnessDoc;
    const s = getState();
    // An OMITTED stamp is never a signal: the server could not derive it
    // (transient git failure), and an omit↔value flap must not fire reloads —
    // the baseline is only written by load(), so omission leaves it intact. A
    // PRESENT stamp against a null baseline (the load itself ran degraded) IS
    // a signal: the displayed documents were fetched under an unknown git
    // state and may already be stale, so recovery reloads — which re-seeds the
    // baseline — rather than silently adopting a stamp the data may predate.
    const stampChanged =
      f.commitGraphStamp != null &&
      (s.lastCommitGraphStamp == null ||
        f.commitGraphStamp !== s.lastCommitGraphStamp);
    const changed = (f.eventCount ?? null) !== s.lastEventCount || stampChanged;
    if (changed) {
      clearTimeout(pollSettleTimer);
      setRefreshState("updated");
      documentsLoaded = await loadWholeDocuments();
      if (!documentsLoaded) {
        setRefreshState("degraded");
        commit({ attentionDelta: null });
        return;
      }
      let historyLoaded = true;
      if (getState().followByLens.timeline) {
        const generation = timelineFollowGeneration();
        const queryKey = historyQueryParams(getState());
        const isCurrent = (): boolean => {
          const current = getState();
          return (
            timelineFollowGeneration() === generation &&
            current.followByLens.timeline &&
            historyQueryParams(current) === queryKey
          );
        };
        historyLoaded = await loadHistoryHead(isCurrent);
        if (!historyLoaded && !isCurrent()) {
          historyLoaded = true;
          await probeNewCount();
        }
      } else {
        await probeNewCount();
      }
      if (!historyLoaded) {
        setRefreshState("degraded");
        settlePoll(false);
        return;
      }
      // The original probe remains final after a nested head load: its later
      // freshness sample cannot outrun the whole documents accepted this tick.
      commitFreshnessBaseline(f);
      settlePoll(true);
    } else {
      setRefreshState("watching");
    }
  } catch {
    setRefreshState("degraded");
    if (documentsLoaded) settlePoll(false);
    else {
      clearTimeout(pollSettleTimer);
      commit({ attentionDelta: null });
    }
  }
}
