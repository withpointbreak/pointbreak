// The data-loading layer: fetch the `/api/*` documents and commit them to the
// store. Ported from the served app.js `load` / `pollFreshness` / `showError`.
//
// The server owns the history query now: `load` fetches only the first page of the
// history for the store's current filter/search/order, and the server returns the
// windowed entries plus the toggle `facets`, the total `matchCount`, the window
// `offset`, and the forward `nextCursor`. The loader no longer builds a client
// search index — there is nothing to index before commit.
//
// The cycle cut (data must not depend on render): `load` only `commit`s; it never
// calls a render function. The store subscriber repaints in response. The
// query-change watcher (`maybeReloadForQuery`) is a store subscriber wired once in
// the composition root, so a filter/search/order change re-fetches page 1 without
// the controls having to reach the network themselves.

import { $ } from "./dom";
import { fetchJSON } from "./http";
import { presentTypes } from "./model";
import {
  commit,
  getState,
  type HistoryDoc,
  type RevisionsDoc,
  type State,
  type ThreadsDoc,
} from "./store";
import type { HistoryEntry } from "./types";

// The `/api/freshness` probe: the event-log head marker (the event count) the
// poller diffs against the stored baseline. Not store state (only its deltas are
// committed, via a reload).
interface FreshnessDoc {
  eventCount?: number;
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
  if (s.filterObject) p.set("object", s.filterObject);
  if (s.order && s.order !== "asc") p.set("order", s.order);
  const present = presentTypes();
  if (present.some((id) => !s.enabledTypes.has(id))) {
    p.set("type", present.filter((id) => s.enabledTypes.has(id)).join(","));
  }
  p.set("limit", String(HISTORY_PAGE));
  return p.toString();
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

/**
 * Load page 1 of the history for the current query, plus the (still full)
 * revisions and threads documents, then commit them + the freshness baseline to
 * the store in one shot. Never calls render — the store subscriber repaints. A
 * load failure surfaces in `#error` rather than throwing.
 */
export async function load(): Promise<void> {
  try {
    // Take the freshness marker BEFORE the documents, so the baseline can never be
    // newer than what was loaded. If an event lands during the document fetch, the
    // marker the next poll reads is higher than this baseline and triggers a reload
    // (at worst a redundant one) rather than masking the append. Fetched in parallel
    // with the documents, the marker could come back newer than them and the poll
    // would then report "unchanged" forever. Seeding from the probe (not
    // `history.eventCount`) also keeps a retired/skipped event — where the event-file
    // marker exceeds the post-skip history count — from forcing a reload every tick.
    const freshness = (await fetchJSON("/api/freshness")) as FreshnessDoc;
    const params = historyQueryParams(getState());
    const [historyRaw, revisionsRaw, threadsRaw] = await Promise.all([
      fetchJSON(`/api/history?${params}`),
      fetchJSON("/api/revisions"),
      fetchJSON("/api/threads"),
    ]);
    showError(null);
    commit({
      history: { ...(historyRaw as HistoryDoc), queryKey: params },
      revisions: revisionsRaw as RevisionsDoc,
      threads: threadsRaw as ThreadsDoc,
      lastEventCount: freshness.eventCount ?? null,
    });
  } catch (err) {
    showError(err instanceof Error ? err.message : String(err));
  }
}

// A single-flight guard so the query watcher never re-enters while its own reload
// is in flight (the reload commits a fresh `history` whose `queryKey` matches the
// active query, which is what stops the loop).
let reloading = false;

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
  void load().finally(() => {
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
function commitHistoryPage(page: HistoryDoc): void {
  const s = getState();
  const queryKey = historyQueryParams(s);
  const prev = s.history;
  const merged =
    prev && prev.queryKey === queryKey
      ? mergeWindows(prev, page)
      : { entries: page.entries ?? [], offset: page.offset ?? 0 };
  commit({
    history: {
      ...page,
      entries: merged.entries,
      offset: merged.offset,
      queryKey,
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
  const run = (async () => {
    try {
      const raw = (await fetchJSON(url)) as HistoryDoc;
      commitHistoryPage(raw);
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    }
  })().finally(() => {
    pageFetches.delete(url);
  });
  pageFetches.set(url, run);
  return run;
}

/**
 * Probe `/api/freshness` and reload when the event-log head marker changed,
 * updating the `#refresh` indicator. A probe failure marks it stalled.
 */
export async function pollFreshness(): Promise<void> {
  try {
    const f = (await fetchJSON("/api/freshness")) as FreshnessDoc;
    const refresh = $("#refresh");
    const s = getState();
    const changed = (f.eventCount ?? null) !== s.lastEventCount;
    if (changed) {
      if (refresh) {
        refresh.textContent = "updated";
        refresh.classList.add("live");
      }
      await load();
      setTimeout(() => {
        if (refresh) {
          refresh.textContent = "watching";
          refresh.classList.remove("live");
        }
      }, 1200);
    } else if (refresh) {
      refresh.textContent = "watching";
    }
  } catch {
    const refresh = $("#refresh");
    if (refresh) refresh.textContent = "stalled";
  }
}
