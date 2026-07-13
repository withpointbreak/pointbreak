// Test harness: a `globalThis.fetch` mock that serves the committed `/api/*`
// fixtures by route, so `data.ts` (`load`/`pollFreshness`) and the on-demand
// `fetchJSON` fetches (detail / diff) can be driven without a live server. Mocking
// at the `fetch` boundary (not `fetchJSON`) keeps the ported `http.ts`/`fetchJSON`
// in the path under test. Append-only shared surface: extend with new routes here.

import attentionJson from "../fixtures/attention.json";
import historyJson from "../fixtures/history.json";
import identityJson from "../fixtures/identity.json";
import revisionJson from "../fixtures/revision.json";
import revisionsJson from "../fixtures/revisions.json";
import snapshotJson from "../fixtures/snapshot.json";
import threadsJson from "../fixtures/threads.json";

// The exact-path collection/list routes, keyed by request pathname. The path-member
// routes (`/api/revisions/{id}`, `/api/snapshots/{id}`) are dispatched separately
// below; their id member is ignored (any id resolves to the single committed
// fixture), mirroring how the old `?id=` mock ignored the query.
const FIXTURES: Record<string, unknown> = {
  "/api/revisions": revisionsJson,
  "/api/threads": threadsJson,
  "/api/identity": identityJson,
};

let attentionResponse: unknown = attentionJson;

/** Override the unscoped `/api/attention` response the mock returns. */
export function setAttentionResponse(payload: unknown): void {
  attentionResponse = payload;
}

/** Restore the committed unscoped attention fixture. */
export function resetAttentionResponse(): void {
  attentionResponse = attentionJson;
}

// `/api/history` is now query-parameterized (page/facets/matchCount) but the mock
// ignores the query string (matches on pathname), defaulting to the committed
// fixture. A paging/reveal test overrides it to drive a synthetic window (a later
// page, an `at=` reveal page) the single-page fixture cannot exercise.
let historyResponse: unknown = historyJson;
let historyError: { status: number; message: string } | null = null;

/** Override the `/api/history` response the mock returns (paging / reveal tests). */
export function setHistoryResponse(payload: unknown): void {
  historyResponse = {
    schema: "pointbreak.inspect-history",
    ...(payload as Record<string, unknown>),
  };
}

/** Make the `/api/history` read fail (resume/query-reload degradation tests). */
export function setHistoryError(status: number, message: string): void {
  historyError = { status, message };
}

/** Restore the default `/api/history` response (the committed history fixture). */
export function resetHistoryResponse(): void {
  historyResponse = historyJson;
  historyError = null;
}

// The freshness probe is not a captured fixture (it is the cheap event-count
// marker): default it to history.json's eventCount so a poll right after `load()`
// reports "unchanged", and let a test override it to drive the changed/reload
// path via {@link setFreshnessResponse}.
const historyDoc = historyJson as {
  eventCount?: number;
};
const DEFAULT_FRESHNESS: unknown = {
  schema: "pointbreak.inspect-freshness",
  version: 1,
  eventCount: historyDoc.eventCount,
  commitGraphStamp: "stamp-fixture",
};
let freshness: unknown = DEFAULT_FRESHNESS;

let newCountResponse: unknown = {
  schema: "pointbreak.inspect-history-new-count",
  newCount: 0,
};

/** Override the `/api/history/new-count` response the mock returns. */
export function setNewCountResponse(payload: unknown): void {
  newCountResponse = payload;
}

/** Restore the default zero new-count response. */
export function resetNewCountResponse(): void {
  newCountResponse = {
    schema: "pointbreak.inspect-history-new-count",
    newCount: 0,
  };
}

/** Override the `/api/freshness` response the mock returns (changed-marker tests). */
export function setFreshnessResponse(payload: unknown): void {
  freshness = {
    schema: "pointbreak.inspect-freshness",
    version: 1,
    ...(payload as Record<string, unknown>),
  };
}

/** Restore the default freshness response (history.json's eventCount marker). */
export function resetFreshnessResponse(): void {
  freshness = DEFAULT_FRESHNESS;
}

// The single-resource `/api/snapshots/{id}` artifact defaults to the committed
// fixture; a diff-controller test overrides it to drive a synthetic snapshot (e.g.
// a many-file accordion the single-file fixture cannot exercise).
let snapshotResponse: unknown = snapshotJson;

/** Override the `/api/snapshots/{id}` response the mock returns (synthetic-snapshot tests). */
export function setSnapshotResponse(payload: unknown): void {
  snapshotResponse = {
    schema: "pointbreak.review-snapshot",
    version: 1,
    ...(payload as Record<string, unknown>),
  };
}

/** Restore the default `/api/snapshots/{id}` response (the committed snapshot fixture). */
export function resetSnapshotResponse(): void {
  snapshotResponse = snapshotJson;
}

// The scoped `/api/attention?revision=` read defaults to an empty set; the
// detail outstanding-block tests override it (or force an error) to drive the
// per-revision judgment view. Every `/api/attention` request target (path +
// query) is recorded so a test can assert the scoped form was actually
// requested rather than a client-side filter of the global document.
let scopedAttentionResponse: unknown = {
  schema: "pointbreak.inspect-attention",
  items: [],
};
let scopedAttentionError: { status: number; message: string } | null = null;
let attentionRequestLog: string[] = [];

/** Override the scoped `/api/attention?revision=` response the mock returns. */
export function setScopedAttentionResponse(payload: unknown): void {
  scopedAttentionResponse = {
    schema: "pointbreak.inspect-attention",
    ...(payload as Record<string, unknown>),
  };
}

/** Make the scoped `/api/attention?revision=` read fail (degrade-to-omission tests). */
export function setScopedAttentionError(status: number, message: string): void {
  scopedAttentionError = { status, message };
}

/** The recorded `/api/attention` request targets (path + query), oldest first. */
export function attentionRequests(): readonly string[] {
  return attentionRequestLog;
}

/** Restore the default scoped-attention response and clear the request log. */
export function resetScopedAttention(): void {
  scopedAttentionResponse = {
    schema: "pointbreak.inspect-attention",
    items: [],
  };
  scopedAttentionError = null;
  attentionRequestLog = [];
}

// The composite `/api/revisions/{id}` document defaults to the committed fixture;
// a fact-supersession test overrides it to drive a fork the plain fixture lacks.
let compositeResponse: unknown = revisionJson;

/** Override the `/api/revisions/{id}` response the mock returns (fact-graph tests). */
export function setCompositeResponse(payload: unknown): void {
  compositeResponse = payload;
}

/** Restore the default `/api/revisions/{id}` response (the committed fixture). */
export function resetCompositeResponse(): void {
  compositeResponse = revisionJson;
}

/** The request target as a string, accepting the full `fetch` input union. */
function urlOf(input: RequestInfo | URL): string {
  if (typeof input === "string") return input;
  if (input instanceof URL) return input.href;
  return input.url;
}

/** The parsed request target (relative `/api/*` paths resolve against a stub origin). */
function targetOf(input: RequestInfo | URL): URL {
  return new URL(urlOf(input), "http://inspector.test");
}

/** A JSON `200` response. */
function json(data: unknown): Promise<Response> {
  return Promise.resolve(
    new Response(JSON.stringify(data), {
      status: 200,
      headers: { "content-type": "application/json" },
    }),
  );
}

/** An error response with a JSON body, mirroring the server's `{ error }` shape. */
function errorResponse(status: number, message: string): Promise<Response> {
  return Promise.resolve(
    new Response(JSON.stringify({ error: message }), {
      status,
      headers: { "content-type": "application/json" },
    }),
  );
}

/**
 * Classify a path under a member `prefix`, mirroring the server's
 * `path_member`/`decode_member` route decisions:
 * - `{ kind: "member", id }` — a single non-empty segment (→ 200 fixture);
 * - `{ kind: "empty" }` — a trailing slash with no id (→ 400, like `decode_member` None);
 * - `null` — not under `prefix`, or a deeper/multi-segment path (→ caller falls through to 404).
 */
function classifyMember(
  pathname: string,
  prefix: string,
): { kind: "member"; id: string } | { kind: "empty" } | null {
  if (!pathname.startsWith(prefix)) return null;
  const rest = pathname.slice(prefix.length);
  if (rest === "") return { kind: "empty" };
  if (rest.includes("/")) return null;
  return { kind: "member", id: rest };
}

const mockFetch: typeof fetch = (input) => {
  const target = targetOf(input);
  const pathname = target.pathname;
  if (pathname === "/api/freshness") return json(freshness);
  if (pathname === "/api/history/new-count") return json(newCountResponse);
  if (pathname === "/api/history") {
    if (historyError)
      return errorResponse(historyError.status, historyError.message);
    return json(historyResponse);
  }
  if (pathname === "/api/attention") {
    attentionRequestLog.push(pathname + target.search);
    if (target.searchParams.get("revision") !== null) {
      if (scopedAttentionError)
        return errorResponse(
          scopedAttentionError.status,
          scopedAttentionError.message,
        );
      return json(scopedAttentionResponse);
    }
    return json(attentionResponse);
  }
  for (const [prefix, fixture] of [
    ["/api/snapshots/", snapshotResponse],
    ["/api/revisions/", compositeResponse],
  ] as const) {
    const m = classifyMember(pathname, prefix);
    if (m?.kind === "member") return json(fixture);
    if (m?.kind === "empty") return errorResponse(400, "missing id");
    // null → not this prefix (or deeper); keep checking, then fall through to 404.
  }
  const data = FIXTURES[pathname];
  if (data === undefined)
    return errorResponse(404, `no fixture for ${pathname}`);
  return json(data);
};

// `null` is the "not installed" sentinel; the `=== null` guards narrow it away on
// restore, so no cast is needed.
let saved: typeof fetch | null = null;

/** Swap `globalThis.fetch` for the fixture-serving mock. Restore with {@link uninstallFetchMock}. */
export function installFetchMock(): void {
  if (saved === null) saved = globalThis.fetch;
  globalThis.fetch = mockFetch;
}

/** Restore the global `fetch` the harness replaced (a no-op if it was never installed). */
export function uninstallFetchMock(): void {
  if (saved === null) return;
  globalThis.fetch = saved;
  saved = null;
}
