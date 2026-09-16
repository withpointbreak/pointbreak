/** Change-only authenticated HTTP leaf. It deliberately has no legacy aggregate routes. */

import {
  getSessionToken,
  recoverUnauthorized,
  sessionCredentialVersion,
} from "./auth";
import {
  type ChangeInspectorAccess,
  type ChangeRecoveryFailure,
  decodeChangeRecoveryFailure,
} from "./change-recovery-protocol";
import {
  markRequestFailure,
  markRequestSuccess,
  type RequestFailureKind,
} from "./connection";

type ChangeRequestFailureKind =
  | RequestFailureKind
  | "aborted"
  | "busy"
  | ChangeRecoveryFailure["kind"];

export class ChangeInspectorRequestFailure extends Error {
  constructor(
    readonly kind: ChangeRequestFailureKind,
    readonly status?: number,
  ) {
    super(
      kind === "aborted"
        ? "request cancelled"
        : kind === "busy"
          ? "authoritative reader is busy"
          : kind === "unauthorized"
            ? "authentication required"
            : kind === "unreachable"
              ? "server unavailable"
              : "server response error",
    );
  }
}

export class ChangeInspectorRecoveryFailure extends ChangeInspectorRequestFailure {
  constructor(readonly document: ChangeRecoveryFailure) {
    super(document.kind, document.status);
    this.message = document.message;
  }
}

export class ChangeInspectorPageFailure extends ChangeInspectorRequestFailure {
  constructor(
    readonly code: "invalid_query" | "stale_projection" | "moving_journal",
    status: number,
  ) {
    super("protocol", status);
    if (code === "moving_journal")
      this.message = "Timeline journal changed while loading; retry";
  }
}

export interface ChangeInspectorResponse {
  value: unknown;
  accessSource: "authoritative-fallback" | null;
}

export interface ChangeInspectorFetchOptions {
  reportConnection?: boolean;
  signal?: AbortSignal;
  access?: ChangeInspectorAccess;
  method?: "GET" | "POST";
}

const ELECTABLE_PATHS = new Set([
  "/api/v2/profile",
  "/api/v2/changes",
  "/api/v2/attention",
  "/api/v2/history",
]);
const CONTROL_PATHS = new Set([
  "/api/derived-access/retry",
  "/api/derived-access/cancel",
]);
let authoritativeQueue: Promise<void> = Promise.resolve();

function isRequestAbort(error: unknown, signal?: AbortSignal): boolean {
  return (
    signal?.aborted === true ||
    (error instanceof DOMException && error.name === "AbortError")
  );
}

function requestPath(path: string): string {
  return path.split("?", 1)[0] ?? path;
}

function withAccess(path: string, access?: ChangeInspectorAccess): string {
  if (access !== "authoritative" || !ELECTABLE_PATHS.has(requestPath(path)))
    return path;
  const query = path.includes("?") ? path.slice(path.indexOf("?") + 1) : "";
  if (new URLSearchParams(query).has("access"))
    throw new ChangeInspectorRequestFailure("protocol");
  return `${path}${path.includes("?") ? "&" : "?"}access=${access}`;
}

function failure(
  kind: RequestFailureKind,
  status?: number,
  reportConnection = true,
): ChangeInspectorRequestFailure {
  if (reportConnection) markRequestFailure(kind, { degradeRefresh: false });
  return new ChangeInspectorRequestFailure(kind, status);
}

function typedFailure(value: unknown, status: number): Error | null {
  const decoded = decodeChangeRecoveryFailure(value, status);
  if (decoded === null) return null;
  if (
    decoded.kind === "page" &&
    (decoded.code === "invalid_query" ||
      decoded.code === "stale_projection" ||
      decoded.code === "moving_journal")
  )
    return new ChangeInspectorPageFailure(decoded.code, status);
  return new ChangeInspectorRecoveryFailure(decoded);
}

async function fetchOnce(
  path: string,
  options: ChangeInspectorFetchOptions,
  transportSignal: AbortSignal | null | undefined = options.signal,
): Promise<ChangeInspectorResponse> {
  const reportConnection = options.reportConnection !== false;
  const method = options.method ?? "GET";
  if (method === "POST" && !CONTROL_PATHS.has(requestPath(path)))
    throw new ChangeInspectorRequestFailure("protocol");
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
      signal: transportSignal,
    });
  } catch (error) {
    if (isRequestAbort(error, options.signal))
      throw new ChangeInspectorRequestFailure("aborted");
    throw failure("unreachable", undefined, reportConnection);
  }
  let body: string;
  try {
    body = await response.text();
  } catch (error) {
    if (isRequestAbort(error, options.signal))
      throw new ChangeInspectorRequestFailure("aborted");
    throw failure("protocol", response.status, reportConnection);
  }
  if (options.signal?.aborted)
    throw new ChangeInspectorRequestFailure("aborted");
  if (response.status === 401)
    throw new ChangeInspectorRequestFailure("unauthorized", 401);
  let data: unknown;
  try {
    data = JSON.parse(body);
  } catch {
    throw failure("protocol", response.status, reportConnection);
  }
  if (!response.ok) {
    const decoded = typedFailure(data, response.status);
    if (decoded !== null) throw decoded;
    if (response.status === 429 && options.access === "authoritative")
      throw new ChangeInspectorRequestFailure("busy", 429);
    throw failure("protocol", response.status, reportConnection);
  }
  if (
    typeof data !== "object" ||
    data === null ||
    ("error" in data && Boolean((data as Record<string, unknown>).error))
  )
    throw failure("protocol", response.status, reportConnection);
  if (options.signal?.aborted)
    throw new ChangeInspectorRequestFailure("aborted");
  if (reportConnection) markRequestSuccess();
  const source = response.headers?.get?.("X-Pointbreak-Access-Source");
  return {
    value: data,
    accessSource: source === "authoritative-fallback" ? source : null,
  };
}

async function fetchAuthenticated(
  path: string,
  options: ChangeInspectorFetchOptions,
  transportSignal: AbortSignal | null | undefined = options.signal,
): Promise<ChangeInspectorResponse> {
  const credentialVersion = sessionCredentialVersion();
  try {
    return await fetchOnce(path, options, transportSignal);
  } catch (error) {
    if (
      !(error instanceof ChangeInspectorRequestFailure) ||
      error.kind !== "unauthorized"
    )
      throw error;
  }
  if (options.signal?.aborted)
    throw new ChangeInspectorRequestFailure("aborted");
  const changed = sessionCredentialVersion() !== credentialVersion;
  const recovered = changed ? true : await recoverUnauthorized();
  if (options.signal?.aborted)
    throw new ChangeInspectorRequestFailure("aborted");
  if (recovered && (options.method ?? "GET") === "GET")
    return fetchOnce(path, options, transportSignal);
  throw failure("unauthorized", 401, options.reportConnection !== false);
}

function settleForConsumer<T>(
  transport: Promise<T>,
  signal?: AbortSignal,
): Promise<T> {
  if (signal === undefined) return transport;
  return new Promise<T>((resolve, reject) => {
    let settled = false;
    const finish = (complete: () => void) => {
      if (settled) return;
      settled = true;
      signal.removeEventListener("abort", onAbort);
      complete();
    };
    const onAbort = () =>
      finish(() => reject(new ChangeInspectorRequestFailure("aborted")));
    signal.addEventListener("abort", onAbort, { once: true });
    if (signal.aborted) onAbort();
    transport.then(
      (value) => finish(() => resolve(value)),
      (error: unknown) => finish(() => reject(error)),
    );
  });
}

/** Fetch one Change reader document with explicit generation access and response metadata. */
export function fetchChangeInspectorResponse(
  path: string,
  options: ChangeInspectorFetchOptions = {},
): Promise<ChangeInspectorResponse> {
  const selectedPath = withAccess(path, options.access);
  const operation = () => {
    if (options.signal?.aborted)
      return Promise.reject(new ChangeInspectorRequestFailure("aborted"));
    return fetchAuthenticated(selectedPath, options);
  };
  if (
    options.access !== "authoritative" ||
    !ELECTABLE_PATHS.has(requestPath(path))
  )
    return operation();
  const transportOperation = () => {
    if (options.signal?.aborted)
      return Promise.reject(new ChangeInspectorRequestFailure("aborted"));
    // The consumer may become obsolete after dispatch, but the issued read
    // retains this queue slot until its body settles.
    return fetchAuthenticated(selectedPath, options, null);
  };
  const transport = authoritativeQueue.then(
    transportOperation,
    transportOperation,
  );
  authoritativeQueue = transport.then(
    () => undefined,
    () => undefined,
  );
  return settleForConsumer(transport, options.signal);
}

/** Fetch one Change reader document, retrying a GET exactly once after capability recovery. */
export async function fetchChangeInspectorJSON(
  path: string,
  options: ChangeInspectorFetchOptions = {},
): Promise<unknown> {
  return (await fetchChangeInspectorResponse(path, options)).value;
}
