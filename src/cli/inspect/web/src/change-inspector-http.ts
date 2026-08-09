/** Change-only authenticated HTTP leaf. It deliberately has no legacy aggregate routes. */

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

export class ChangeInspectorRequestFailure extends Error {
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
  }
}

export class ChangeInspectorPageFailure extends ChangeInspectorRequestFailure {
  constructor(
    readonly code: "invalid_query" | "stale_projection" | "moving_journal",
    status: number,
  ) {
    super("protocol", status);
    if (code === "moving_journal") {
      this.message = "Timeline journal changed while loading; retry";
    }
  }
}

function failure(
  kind: RequestFailureKind,
  status?: number,
  reportConnection = true,
): ChangeInspectorRequestFailure {
  if (reportConnection) markRequestFailure(kind, { degradeRefresh: false });
  return new ChangeInspectorRequestFailure(kind, status);
}

function typedPageFailure(
  value: unknown,
  status: number,
): ChangeInspectorPageFailure | null {
  if (typeof value !== "object" || value === null) return null;
  const document = value as Record<string, unknown>;
  if (
    (document.schema !== "pointbreak.inspect-change-page-error" &&
      document.schema !== "pointbreak.inspect-event-history-error") ||
    document.version !== 1
  )
    return null;
  if (document.code === "invalid_query" && status === 400)
    return new ChangeInspectorPageFailure("invalid_query", status);
  if (document.code === "stale_projection" && status === 409)
    return new ChangeInspectorPageFailure("stale_projection", status);
  if (
    document.schema === "pointbreak.inspect-event-history-error" &&
    document.code === "moving_journal" &&
    status === 503
  ) {
    return new ChangeInspectorPageFailure("moving_journal", status);
  }
  return null;
}

async function fetchOnce(
  path: string,
  reportConnection: boolean,
): Promise<unknown> {
  const headers: Record<string, string> = {};
  const token = getSessionToken();
  if (token) headers.Authorization = `Bearer ${token}`;
  let response: Response;
  try {
    response = await fetch(path, {
      method: "GET",
      cache: "no-store",
      credentials: "omit",
      referrerPolicy: "no-referrer",
      headers,
    });
  } catch {
    throw failure("unreachable", undefined, reportConnection);
  }
  if (response.status === 401)
    throw new ChangeInspectorRequestFailure("unauthorized", 401);
  let data: unknown;
  try {
    data = JSON.parse(await response.text());
  } catch {
    throw failure("protocol", response.status, reportConnection);
  }
  if (!response.ok)
    throw (
      typedPageFailure(data, response.status) ??
      failure("protocol", response.status, reportConnection)
    );
  if (
    typeof data !== "object" ||
    data === null ||
    ("error" in data && Boolean((data as Record<string, unknown>).error))
  ) {
    throw failure("protocol", response.status, reportConnection);
  }
  if (reportConnection) markRequestSuccess();
  return data;
}

/** Fetch one Change reader document, retrying exactly once after capability recovery. */
export async function fetchChangeInspectorJSON(
  path: string,
  options: { reportConnection?: boolean } = {},
): Promise<unknown> {
  const reportConnection = options.reportConnection !== false;
  const credentialVersion = sessionCredentialVersion();
  try {
    return await fetchOnce(path, reportConnection);
  } catch (error) {
    if (
      !(error instanceof ChangeInspectorRequestFailure) ||
      error.kind !== "unauthorized"
    )
      throw error;
  }
  if (
    sessionCredentialVersion() !== credentialVersion ||
    (await recoverUnauthorized())
  )
    return fetchOnce(path, reportConnection);
  throw failure("unauthorized", 401, reportConnection);
}
