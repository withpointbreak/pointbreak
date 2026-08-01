import { getState } from "./store";

let fallbackTail: Promise<void> = Promise.resolve();

/**
 * Add the explicit authoritative-read selector to a fallback-capable API path.
 * The choice is session-local and user-elected; callers never infer it from a
 * 503. The server labels successful fallback responses and serializes their
 * expensive work through one service-wide permit.
 */
export function withSelectedAccess(path: string): string {
  if (!getState().authoritativeFallback) return path;
  const url = new URL(path, location.origin);
  url.searchParams.set("access", "authoritative");
  return `${url.pathname}${url.search}`;
}

/**
 * Serialize this client's explicit high-cost fallback work. The server remains
 * the cross-client authority and rejects overlap with 429; this queue prevents
 * the Inspector's own collection polling and detail reads from colliding with
 * one another first.
 */
export function runWithSelectedAccess<T>(
  operation: () => Promise<T>,
): Promise<T> {
  if (!getState().authoritativeFallback) return operation();
  const result = fallbackTail.then(operation, operation);
  fallbackTail = result.then(
    () => undefined,
    () => undefined,
  );
  return result;
}
