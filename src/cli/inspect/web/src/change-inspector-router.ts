/**
 * Fragment routing for the Change-first Inspector. A Change is the stable work
 * object; a contextual Revision route is exact only when it carries both its
 * Revision ID and its captured artifact hash. Route query values are UI input
 * for the bounded page request, never browser-owned Change semantics.
 */

import type {
  ChangeLens,
  ChangePageQuery,
  EventHistoryQuery,
  RevisionRef,
} from "./change-protocol";

export type ChangeInspectorRoute =
  | { kind: "timeline"; historyQuery: EventHistoryQuery }
  | {
      kind: "event";
      eventId: string;
      historyQuery: EventHistoryQuery;
      // Event filters belong to `historyQuery`. The empty card query keeps an
      // event compatible with the shared exact-surface navigation contract:
      // back/close may fall back through a card lens without inventing filters.
      query: ChangePageQuery;
    }
  | { kind: "lens"; lens: ChangeLens; query: ChangePageQuery }
  | { kind: "change"; changeId: string; query: ChangePageQuery }
  | {
      kind: "revision";
      changeId: string;
      revision: RevisionRef;
      query: ChangePageQuery;
      focus?: ExactRouteFocus;
    }
  | {
      kind: "resource";
      changeId: string;
      revision: RevisionRef;
      query: ChangePageQuery;
      focus?: ExactRouteFocus;
    }
  | {
      /**
       * The review reader's full-frame annotated view. Unlike `resource`, it
       * deliberately reads the contextual exact-Revision document so inline
       * facts remain bound to this Change and Revision pair.
       */
      kind: "diff";
      changeId: string;
      revision: RevisionRef;
      query: ChangePageQuery;
      focus?: ExactRouteFocus;
    }
  | {
      kind: "association";
      changeId: string;
      revision: RevisionRef;
      query: ChangePageQuery;
      focus?: ExactRouteFocus;
    }
  | {
      kind: "interdiff";
      changeId: string;
      from: RevisionRef;
      to: RevisionRef;
      query: ChangePageQuery;
      focus?: ExactRouteFocus;
    }
  | { kind: "invalid"; message: string };

const QUERY_KEYS = [
  "q",
  "topology",
  "lifecycle",
  "attention",
  "availability",
  "after",
  "limit",
  "order",
] as const;

const ROUTE_QUERY_KEYS = new Set<string>([
  ...QUERY_KEYS,
  "artifactHash",
  "fromArtifactHash",
  "toArtifactHash",
  "fact",
  "file",
  "fq",
]);

const TIMELINE_QUERY_KEYS = [
  "limit",
  "after",
  "at",
  "q",
  "type",
  "track",
  "change",
  "revision",
  "artifactHash",
  "order",
] as const;
const TIMELINE_QUERY_KEY_SET = new Set<string>(TIMELINE_QUERY_KEYS);

export interface ExactRouteFocus {
  factId?: string;
  filePath?: string;
  /** The local, presentation-only file navigator query for an annotated diff. */
  fileQuery?: string;
}

function decodeSegment(value: string): string | null {
  try {
    const decoded = decodeURIComponent(value);
    return decoded.length > 0 ? decoded : null;
  } catch {
    return null;
  }
}

interface ParsedQuery {
  query: ChangePageQuery;
  artifactHashes: string[];
  fromArtifactHashes: string[];
  toArtifactHashes: string[];
  facts: string[];
  files: string[];
  fileQueries: string[];
}

function validQueryEncoding(search: string): boolean {
  if (!search) return true;
  return search.split("&").every((component) => {
    if (!component) return false;
    const separator = component.indexOf("=");
    const key = separator === -1 ? component : component.slice(0, separator);
    const value = separator === -1 ? "" : component.slice(separator + 1);
    try {
      decodeURIComponent(key.replaceAll("+", " "));
      decodeURIComponent(value.replaceAll("+", " "));
      return true;
    } catch {
      return false;
    }
  });
}

function parseQuery(search: string): ParsedQuery | { message: string } {
  if (!validQueryEncoding(search)) {
    return { message: "Malformed route query encoding." };
  }
  const params = new URLSearchParams(search);
  const query: ChangePageQuery = {};
  for (const key of params.keys()) {
    if (!ROUTE_QUERY_KEYS.has(key)) {
      return { message: `Unknown ${key} route query.` };
    }
  }
  for (const key of QUERY_KEYS) {
    const values = params.getAll(key);
    if (values.length > 1) return { message: `Duplicate ${key} route query.` };
    const value = values[0];
    if (value === undefined) continue;
    if (key === "limit") {
      const limit = Number(value);
      if (!Number.isInteger(limit))
        return { message: "Invalid limit route query." };
      query.limit = limit;
    } else if (key === "order") {
      if (value !== "change_id_asc") {
        return { message: "Invalid order route query." };
      }
      query.order = value;
    } else {
      query[key] = value;
    }
  }
  return {
    query,
    artifactHashes: params.getAll("artifactHash"),
    fromArtifactHashes: params.getAll("fromArtifactHash"),
    toArtifactHashes: params.getAll("toArtifactHash"),
    facts: params.getAll("fact"),
    files: params.getAll("file"),
    fileQueries: params.getAll("fq"),
  };
}

function isParseError(
  value: ParsedQuery | { message: string },
): value is { message: string } {
  return "message" in value;
}

function parseTimelineQuery(
  search: string,
): EventHistoryQuery | { message: string } {
  if (!validQueryEncoding(search)) {
    return { message: "Malformed route query encoding." };
  }
  const params = new URLSearchParams(search);
  const query: EventHistoryQuery = {};
  for (const key of params.keys()) {
    if (!TIMELINE_QUERY_KEY_SET.has(key)) {
      return { message: `Unknown ${key} route query.` };
    }
    if (params.getAll(key).length !== 1) {
      return { message: `Duplicate ${key} route query.` };
    }
  }
  for (const key of TIMELINE_QUERY_KEYS) {
    const value = params.get(key);
    if (value === null) continue;
    if (!value) return { message: `Empty ${key} route query.` };
    if (key === "limit") {
      const limit = Number(value);
      if (!Number.isInteger(limit) || limit < 1 || limit > 100)
        return { message: "Invalid limit route query." };
      query.limit = limit;
    } else if (key === "order") {
      if (value !== "asc" && value !== "desc")
        return { message: "Invalid order route query." };
      query.order = value;
    } else if (
      key === "after" ||
      key === "at" ||
      key === "q" ||
      key === "type" ||
      key === "track" ||
      key === "change" ||
      key === "revision" ||
      key === "artifactHash"
    ) {
      query[key] = value;
    }
  }
  if ((query.revision === undefined) !== (query.artifactHash === undefined)) {
    return { message: "Timeline revision requires artifactHash." };
  }
  if (query.at !== undefined && query.after !== undefined) {
    return { message: "Timeline at and after cannot be combined." };
  }
  return query;
}

function isTimelineParseError(
  value: EventHistoryQuery | { message: string },
): value is { message: string } {
  return "message" in value;
}

export function parseChangeInspectorRoute(hash: string): ChangeInspectorRoute {
  const raw = hash.startsWith("#") ? hash.slice(1) : hash;
  const separator = raw.indexOf("?");
  const path = separator === -1 ? raw : raw.slice(0, separator);
  const search = separator === -1 ? "" : raw.slice(separator + 1);
  const segments = path.split("/").filter(Boolean);
  // The empty fragment intentionally opens the monitorable Timeline rather
  // than silently falling back to a card lens.
  if (
    segments.length === 0 ||
    (segments.length === 1 && segments[0] === "timeline")
  ) {
    const historyQuery = parseTimelineQuery(search);
    return isTimelineParseError(historyQuery)
      ? { kind: "invalid", message: historyQuery.message }
      : { kind: "timeline", historyQuery };
  }
  if (
    segments.length === 3 &&
    segments[0] === "timeline" &&
    segments[1] === "events"
  ) {
    const eventId = decodeSegment(segments[2]);
    if (eventId === null)
      return { kind: "invalid", message: "Event routes require an event ID." };
    const historyQuery = parseTimelineQuery(search);
    if (isTimelineParseError(historyQuery))
      return { kind: "invalid", message: historyQuery.message };
    if (historyQuery.at !== undefined)
      return {
        kind: "invalid",
        message: "Event routes select their anchor from the event ID.",
      };
    return { kind: "event", eventId, historyQuery, query: {} };
  }
  const parsed = parseQuery(search);
  if (isParseError(parsed)) return { kind: "invalid", message: parsed.message };
  const {
    query,
    artifactHashes,
    fromArtifactHashes,
    toArtifactHashes,
    facts,
    files,
    fileQueries,
  } = parsed;
  const focus = (
    allowFileQuery = false,
  ): ExactRouteFocus | null | undefined => {
    if (
      facts.length > 1 ||
      files.length > 1 ||
      fileQueries.length > 1 ||
      facts.some((value) => !value) ||
      files.some((value) => !value) ||
      fileQueries.some((value) => !value) ||
      (!allowFileQuery && fileQueries.length > 0)
    ) {
      return null;
    }
    const selected = {
      ...(facts[0] ? { factId: facts[0] } : {}),
      ...(files[0] ? { filePath: files[0] } : {}),
      ...(fileQueries[0] ? { fileQuery: fileQueries[0] } : {}),
    };
    return Object.keys(selected).length ? selected : undefined;
  };
  if (
    segments.length === 1 &&
    (segments[0] === "changes" || segments[0] === "attention")
  ) {
    if (
      artifactHashes.length > 0 ||
      fromArtifactHashes.length > 0 ||
      toArtifactHashes.length > 0 ||
      facts.length > 0 ||
      files.length > 0 ||
      fileQueries.length > 0
    ) {
      return {
        kind: "invalid",
        message: "artifactHash is only valid on an exact Revision route.",
      };
    }
    return { kind: "lens", lens: segments[0], query };
  }
  if (segments[0] !== "changes")
    return { kind: "invalid", message: "Unknown Change Inspector route." };
  const changeId = decodeSegment(segments[1] ?? "");
  if (changeId === null)
    return { kind: "invalid", message: "Change routes require a Change ID." };
  if (segments.length === 2) {
    if (
      artifactHashes.length > 0 ||
      fromArtifactHashes.length > 0 ||
      toArtifactHashes.length > 0 ||
      facts.length > 0 ||
      files.length > 0 ||
      fileQueries.length > 0
    ) {
      return {
        kind: "invalid",
        message: "artifactHash is only valid on an exact Revision route.",
      };
    }
    return { kind: "change", changeId, query };
  }
  const exactRevision = (revisionId: string | null): RevisionRef | null => {
    if (
      revisionId === null ||
      artifactHashes.length !== 1 ||
      !artifactHashes[0]
    )
      return null;
    return {
      revisionId,
      objectArtifactContentHash: artifactHashes[0],
    };
  };
  const exactFailure = (): ChangeInspectorRoute => ({
    kind: "invalid",
    message:
      artifactHashes.length > 1
        ? "Exact Revision routes require exactly one artifactHash."
        : "Exact Revision routes require artifactHash.",
  });
  if (segments[2] === "revisions" && segments.length >= 4) {
    const revision = exactRevision(decodeSegment(segments[3]));
    if (revision === null) return exactFailure();
    const exactFocus = focus(segments.length === 5 && segments[4] === "diff");
    if (exactFocus === null)
      return {
        kind: "invalid",
        message:
          "Exact route focus requires at most one non-empty fact and file.",
      };
    if (fromArtifactHashes.length > 0 || toArtifactHashes.length > 0)
      return {
        kind: "invalid",
        message: "Revision routes do not accept interdiff hashes.",
      };
    if (segments.length === 4)
      return {
        kind: "revision",
        changeId,
        revision,
        query,
        ...(exactFocus ? { focus: exactFocus } : {}),
      };
    if (segments.length === 5 && segments[4] === "resource")
      return {
        kind: "resource",
        changeId,
        revision,
        query,
        ...(exactFocus ? { focus: exactFocus } : {}),
      };
    if (segments.length === 5 && segments[4] === "diff")
      return {
        kind: "diff",
        changeId,
        revision,
        query,
        ...(exactFocus ? { focus: exactFocus } : {}),
      };
    if (segments.length === 5 && segments[4] === "association")
      return {
        kind: "association",
        changeId,
        revision,
        query,
        ...(exactFocus ? { focus: exactFocus } : {}),
      };
  }
  if (segments[2] === "interdiff" && segments.length === 5) {
    if (artifactHashes.length > 0)
      return {
        kind: "invalid",
        message: "Interdiff routes use endpoint artifact hashes.",
      };
    const fromRevisionId = decodeSegment(segments[3]);
    const toRevisionId = decodeSegment(segments[4]);
    if (fromRevisionId === null || toRevisionId === null)
      return {
        kind: "invalid",
        message: "Interdiff routes require both Revision IDs.",
      };
    if (
      fromArtifactHashes.length !== 1 ||
      !fromArtifactHashes[0] ||
      toArtifactHashes.length !== 1 ||
      !toArtifactHashes[0]
    )
      return {
        kind: "invalid",
        message:
          "Interdiff routes require exactly one artifact hash for each endpoint.",
      };
    const exactFocus = focus();
    if (exactFocus === null)
      return {
        kind: "invalid",
        message:
          "Exact route focus requires at most one non-empty fact and file.",
      };
    return {
      kind: "interdiff",
      changeId,
      from: {
        revisionId: fromRevisionId,
        objectArtifactContentHash: fromArtifactHashes[0],
      },
      to: {
        revisionId: toRevisionId,
        objectArtifactContentHash: toArtifactHashes[0],
      },
      query,
      ...(exactFocus ? { focus: exactFocus } : {}),
    };
  }
  return { kind: "invalid", message: "Unknown Change Inspector route." };
}

function appendQuery(query: ChangePageQuery, params: URLSearchParams): void {
  for (const key of QUERY_KEYS) {
    const value = query[key];
    if (value !== undefined) params.set(key, String(value));
  }
}

function appendTimelineQuery(
  query: EventHistoryQuery,
  params: URLSearchParams,
): void {
  for (const key of TIMELINE_QUERY_KEYS) {
    const value = query[key];
    if (value !== undefined) params.set(key, String(value));
  }
}

export function formatChangeInspectorRoute(
  route: Exclude<ChangeInspectorRoute, { kind: "invalid" }>,
): string {
  const params = new URLSearchParams();
  if (route.kind === "timeline") {
    appendTimelineQuery(route.historyQuery, params);
    return `#/timeline${params.size ? `?${params}` : ""}`;
  }
  if (route.kind === "event") {
    appendTimelineQuery(route.historyQuery, params);
    const eventId = encodeURIComponent(route.eventId);
    return `#/timeline/events/${eventId}${params.size ? `?${params}` : ""}`;
  }
  appendQuery(route.query, params);
  if (
    route.kind === "revision" ||
    route.kind === "resource" ||
    route.kind === "diff" ||
    route.kind === "association"
  )
    params.set("artifactHash", route.revision.objectArtifactContentHash);
  if (route.kind === "interdiff") {
    params.set("fromArtifactHash", route.from.objectArtifactContentHash);
    params.set("toArtifactHash", route.to.objectArtifactContentHash);
  }
  if ("focus" in route && route.focus?.factId)
    params.set("fact", route.focus.factId);
  if ("focus" in route && route.focus?.filePath)
    params.set("file", route.focus.filePath);
  if (route.kind === "diff" && route.focus?.fileQuery)
    params.set("fq", route.focus.fileQuery);
  const suffix = params.size ? `?${params}` : "";
  if (route.kind === "lens") return `#/${route.lens}${suffix}`;
  const change = encodeURIComponent(route.changeId);
  if (route.kind === "change") return `#/changes/${change}${suffix}`;
  if (route.kind === "revision")
    return `#/changes/${change}/revisions/${encodeURIComponent(route.revision.revisionId)}${suffix}`;
  if (route.kind === "resource")
    return `#/changes/${change}/revisions/${encodeURIComponent(route.revision.revisionId)}/resource${suffix}`;
  if (route.kind === "diff")
    return `#/changes/${change}/revisions/${encodeURIComponent(route.revision.revisionId)}/diff${suffix}`;
  if (route.kind === "association")
    return `#/changes/${change}/revisions/${encodeURIComponent(route.revision.revisionId)}/association${suffix}`;
  return `#/changes/${change}/interdiff/${encodeURIComponent(route.from.revisionId)}/${encodeURIComponent(route.to.revisionId)}${suffix}`;
}

export function lensForRoute(
  route: ChangeInspectorRoute,
): "timeline" | ChangeLens {
  return route.kind === "timeline" || route.kind === "event"
    ? "timeline"
    : route.kind === "lens"
      ? route.lens
      : "changes";
}

/** Return the same bounded query at its first page. */
export function firstPageQuery(query: ChangePageQuery): ChangePageQuery {
  const { after: _after, ...firstPage } = query;
  return firstPage;
}

/**
 * Exact routes use the Changes page as their bounded companion generation.
 * Attention continuations are signed to the Attention lens, so leaving a
 * paginated Attention page must retain its filters while returning to page one
 * before the route becomes exact. Changes continuations remain valid because
 * exact routes keep Changes as their companion lens.
 */
export function queryForExactNavigation(
  route: Exclude<ChangeInspectorRoute, { kind: "invalid" }>,
): ChangePageQuery {
  if (route.kind === "timeline" || route.kind === "event") return {};
  if (route.kind !== "lens" || route.lens !== "attention") return route.query;
  return firstPageQuery(route.query);
}
