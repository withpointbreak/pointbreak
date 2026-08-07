/**
 * Fragment routing for the Change-first Inspector. A Change is the stable work
 * object; a contextual Revision route is exact only when it carries both its
 * Revision ID and its captured artifact hash. Route query values are UI input
 * for the bounded page request, never browser-owned Change semantics.
 */

import type {
  ChangeLens,
  ChangePageQuery,
  RevisionRef,
} from "./change-protocol";

export type ChangeInspectorRoute =
  | { kind: "lens"; lens: ChangeLens; query: ChangePageQuery }
  | { kind: "change"; changeId: string; query: ChangePageQuery }
  | {
      kind: "revision";
      changeId: string;
      revision: RevisionRef;
      query: ChangePageQuery;
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

const ROUTE_QUERY_KEYS = new Set<string>([...QUERY_KEYS, "artifactHash"]);

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
  return { query, artifactHashes: params.getAll("artifactHash") };
}

function isParseError(
  value: ParsedQuery | { message: string },
): value is { message: string } {
  return "message" in value;
}

export function parseChangeInspectorRoute(hash: string): ChangeInspectorRoute {
  const raw = hash.startsWith("#") ? hash.slice(1) : hash;
  const separator = raw.indexOf("?");
  const path = separator === -1 ? raw : raw.slice(0, separator);
  const search = separator === -1 ? "" : raw.slice(separator + 1);
  const parsed = parseQuery(search);
  if (isParseError(parsed)) return { kind: "invalid", message: parsed.message };
  const { query, artifactHashes } = parsed;
  const segments = path.split("/").filter(Boolean);
  if (
    segments.length === 1 &&
    (segments[0] === "changes" || segments[0] === "attention")
  ) {
    if (artifactHashes.length > 0) {
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
    if (artifactHashes.length > 0) {
      return {
        kind: "invalid",
        message: "artifactHash is only valid on an exact Revision route.",
      };
    }
    return { kind: "change", changeId, query };
  }
  if (segments.length !== 4 || segments[2] !== "revisions") {
    return { kind: "invalid", message: "Unknown Change Inspector route." };
  }
  const revisionId = decodeSegment(segments[3]);
  if (revisionId === null)
    return {
      kind: "invalid",
      message: "Revision routes require a Revision ID.",
    };
  if (artifactHashes.length !== 1 || !artifactHashes[0])
    return {
      kind: "invalid",
      message:
        artifactHashes.length > 1
          ? "Exact Revision routes require exactly one artifactHash."
          : "Exact Revision routes require artifactHash.",
    };
  return {
    kind: "revision",
    changeId,
    revision: {
      revisionId,
      objectArtifactContentHash: artifactHashes[0],
    },
    query,
  };
}

function appendQuery(query: ChangePageQuery, params: URLSearchParams): void {
  for (const key of QUERY_KEYS) {
    const value = query[key];
    if (value !== undefined) params.set(key, String(value));
  }
}

export function formatChangeInspectorRoute(
  route: Exclude<ChangeInspectorRoute, { kind: "invalid" }>,
): string {
  const params = new URLSearchParams();
  appendQuery(route.query, params);
  if (route.kind === "revision")
    params.set("artifactHash", route.revision.objectArtifactContentHash);
  const suffix = params.size ? `?${params}` : "";
  if (route.kind === "lens") return `#/${route.lens}${suffix}`;
  const change = encodeURIComponent(route.changeId);
  if (route.kind === "change") return `#/changes/${change}${suffix}`;
  return `#/changes/${change}/revisions/${encodeURIComponent(route.revision.revisionId)}${suffix}`;
}

export function lensForRoute(route: ChangeInspectorRoute): ChangeLens {
  return route.kind === "lens" ? route.lens : "changes";
}
