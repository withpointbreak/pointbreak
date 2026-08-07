import {
  getSessionToken,
  recoverUnauthorized,
  sessionCredentialVersion,
} from "./auth";
import {
  markRequestFailure,
  markRequestSuccess,
  type RequestFailureKind,
} from "./connection";

export class RequestFailure extends Error {
  constructor(
    readonly kind: RequestFailureKind,
    readonly status?: number,
  ) {
    super(
      kind === "unauthorized"
        ? "authentication required"
        : kind === "unreachable"
          ? "server unavailable"
          : "server response error",
    );
    this.name = "RequestFailure";
  }
}

/** A typed refusal from the bounded Change-page grammar. */
export class ChangePageFailure extends RequestFailure {
  constructor(
    readonly code: "invalid_query" | "stale_projection",
    status: number,
  ) {
    super("protocol", status);
    this.name = "ChangePageFailure";
  }
}

function failure(kind: RequestFailureKind, status?: number): RequestFailure {
  markRequestFailure(kind);
  return new RequestFailure(kind, status);
}

interface ExpectedDocument {
  schema: string;
  version?: number;
}

function expectedDocument(path: string): ExpectedDocument | null {
  const pathname = new URL(path, location.origin).pathname;
  const collections: Record<string, ExpectedDocument> = {
    "/api/v2/profile": {
      schema: "pointbreak.inspect-reader-profile",
      version: 1,
    },
    "/api/v2/changes": {
      schema: "pointbreak.inspect-changes-page",
      version: 1,
    },
    "/api/v2/attention": {
      schema: "pointbreak.inspect-attention",
      version: 2,
    },
    "/api/attention": { schema: "pointbreak.inspect-attention" },
    "/api/derived-access/status": {
      schema: "pointbreak.inspect-derived-access-status",
      version: 1,
    },
    "/api/freshness": {
      schema: "pointbreak.inspect-freshness",
      version: 1,
    },
    "/api/history": { schema: "pointbreak.inspect-history" },
    "/api/history/new-count": {
      schema: "pointbreak.inspect-history-new-count",
    },
    "/api/identity": { schema: "pointbreak.inspect-identity" },
    "/api/revisions": { schema: "pointbreak.inspect-revisions-page.v1" },
    "/api/threads": { schema: "pointbreak.inspect-threads" },
    "/api/version": { schema: "pointbreak.version", version: 1 },
  };
  if (collections[pathname]) return collections[pathname];
  if (
    pathname === "/api/derived-access/cancel" ||
    pathname === "/api/derived-access/retry"
  ) {
    return {
      schema: "pointbreak.inspect-derived-access-status",
      version: 1,
    };
  }
  if (/^\/api\/revisions\/[^/]+$/.test(pathname)) {
    return { schema: "pointbreak.review-revision", version: 2 };
  }
  if (/^\/api\/snapshots\/[^/]+$/.test(pathname)) {
    return { schema: "pointbreak.review-snapshot", version: 1 };
  }
  if (/^\/api\/v2\/changes\/[^/]+$/.test(pathname)) {
    return { schema: "pointbreak.review-change", version: 1 };
  }
  if (/^\/api\/v2\/changes\/[^/]+\/revisions\/[^/]+$/.test(pathname)) {
    return { schema: "pointbreak.review-change-revision", version: 1 };
  }
  if (
    /^\/api\/v2\/changes\/[^/]+\/revisions\/[^/]+\/resource$/.test(pathname)
  ) {
    return { schema: "pointbreak.review-revision-resource", version: 1 };
  }
  if (/^\/api\/v2\/changes\/[^/]+\/interdiff\/[^/]+\/[^/]+$/.test(pathname)) {
    return { schema: "pointbreak.review-revision-interdiff", version: 1 };
  }
  return null;
}

function isExpectedDocument(
  data: unknown,
  expected: ExpectedDocument,
): boolean {
  if (typeof data !== "object" || data === null) return false;
  const document = data as Record<string, unknown>;
  return (
    document.schema === expected.schema &&
    (expected.version === undefined || document.version === expected.version)
  );
}

function hasPayloadError(data: unknown): boolean {
  return (
    typeof data === "object" &&
    data !== null &&
    "error" in data &&
    Boolean(data.error)
  );
}

function changePageFailure(
  data: unknown,
  status: number,
): ChangePageFailure | null {
  if (
    typeof data !== "object" ||
    data === null ||
    (data as Record<string, unknown>).schema !==
      "pointbreak.inspect-change-page-error" ||
    (data as Record<string, unknown>).version !== 1
  ) {
    return null;
  }
  const code = (data as Record<string, unknown>).code;
  if (code !== "invalid_query" && code !== "stale_projection") return null;
  if (
    (code === "invalid_query" && status !== 400) ||
    (code === "stale_projection" && status !== 409)
  ) {
    return null;
  }
  markRequestFailure("protocol");
  return new ChangePageFailure(code, status);
}

async function fetchOnce(
  path: string,
  method: "GET" | "POST",
): Promise<unknown> {
  const headers: Record<string, string> = {};
  const token = getSessionToken();
  if (token) headers.Authorization = `Bearer ${token}`;

  let response: Response;
  try {
    response = await fetch(path, {
      method,
      cache: "no-store",
      credentials: "omit",
      referrerPolicy: "no-referrer",
      headers,
    });
  } catch {
    throw failure("unreachable");
  }
  if (response.status === 401) throw new RequestFailure("unauthorized", 401);

  let text: string;
  try {
    text = await response.text();
  } catch {
    throw failure("protocol", response.status);
  }
  let data: unknown;
  try {
    data = JSON.parse(text);
  } catch {
    throw failure("protocol", response.status);
  }
  if (!response.ok) {
    const typed = changePageFailure(data, response.status);
    if (typed !== null) throw typed;
    throw failure("protocol", response.status);
  }
  const expected = expectedDocument(path);
  if (
    hasPayloadError(data) ||
    (expected !== null && !isExpectedDocument(data, expected))
  ) {
    throw failure("protocol", response.status);
  }
  markRequestSuccess();
  return data;
}

/** Fetch one authenticated API document, retrying once after shared 401 recovery. */
export async function fetchJSON(
  path: string,
  method: "GET" | "POST" = "GET",
): Promise<unknown> {
  const requestCredentialVersion = sessionCredentialVersion();
  try {
    return await fetchOnce(path, method);
  } catch (error) {
    if (!(error instanceof RequestFailure) || error.kind !== "unauthorized") {
      throw error;
    }
  }

  const credentialAlreadyRenewed =
    sessionCredentialVersion() !== requestCredentialVersion;
  if (credentialAlreadyRenewed || (await recoverUnauthorized())) {
    try {
      return await fetchOnce(path, method);
    } catch (error) {
      if (error instanceof RequestFailure && error.kind === "unauthorized") {
        throw failure("unauthorized", 401);
      }
      throw error;
    }
  }
  throw failure("unauthorized", 401);
}
