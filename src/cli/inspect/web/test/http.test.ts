import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  AuthCoordinator,
  installAuthCoordinator,
  resetAuthForTests,
  setSessionToken,
} from "../src/auth";
import { getConnectionSnapshot, resetConnectionState } from "../src/connection";
import { fetchJSON, RequestFailure } from "../src/http";

// `fetchJSON` is the fetch leaf. These tests drive it against a hand-stubbed
// global `fetch` so the status / body / error-field combinations of its error
// contract can be exercised directly (the route-serving fixture mock lives in the
// data-layer tests).
let savedFetch: typeof fetch;

beforeEach(() => {
  savedFetch = globalThis.fetch;
  resetAuthForTests();
  resetConnectionState();
});

afterEach(() => {
  globalThis.fetch = savedFetch;
  resetAuthForTests();
});

/** Make the next fetch resolve with `body` at the given HTTP status. */
function stub(body: string, status = 200): void {
  globalThis.fetch = () => Promise.resolve(new Response(body, { status }));
}

describe("fetchJSON", () => {
  it("resolves the parsed JSON for a 200 response", async () => {
    stub(JSON.stringify({ schema: "pointbreak.inspect-history", n: 3 }));
    await expect(fetchJSON("/api/history")).resolves.toEqual({
      schema: "pointbreak.inspect-history",
      n: 3,
    });
    expect(getConnectionSnapshot().connection).toBe("connected");
  });

  it("posts only the exact lifecycle control while preserving schema validation", async () => {
    let method = "";
    globalThis.fetch = ((_input: RequestInfo | URL, init?: RequestInit) => {
      method = init?.method ?? "";
      return Promise.resolve(
        new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-derived-access-status",
            version: 1,
            active: true,
            availability: "absent",
            rebuildInFlight: false,
            rebuildPaused: false,
            servingCurrent: false,
            fallbackInFlight: false,
            actions: ["retry"],
          }),
        ),
      );
    }) as typeof fetch;

    await expect(
      fetchJSON("/api/derived-access/retry", "POST"),
    ).resolves.toMatchObject({ availability: "absent" });
    expect(method).toBe("POST");
  });

  it("attaches the session bearer centrally without changing the request target", async () => {
    const token = "request_leaf_secret_0123456789";
    setSessionToken(token);
    let target = "";
    let authorized = false;
    let safeOptions = false;
    globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
      target = String(input);
      const headers = new Headers(init?.headers);
      authorized = headers.get("Authorization") === `Bearer ${token}`;
      safeOptions =
        init?.cache === "no-store" &&
        init?.credentials === "omit" &&
        init?.referrerPolicy === "no-referrer";
      return Promise.resolve(
        new Response(JSON.stringify({ schema: "pointbreak.inspect-history" })),
      );
    }) as typeof fetch;

    await fetchJSON("/api/history?q=token");

    expect(target).toBe("/api/history?q=token");
    expect(target.includes(token)).toBe(false);
    expect(authorized).toBe(true);
    expect(safeOptions).toBe(true);
  });

  it("classifies invalid JSON as a secret-free protocol failure", async () => {
    stub("<html>nope</html>", 502);
    const failure = await fetchJSON("/api/history").catch((error) => error);
    expect(failure).toBeInstanceOf(RequestFailure);
    expect(failure).toMatchObject({ kind: "protocol", status: 502 });
    expect(String(failure).includes("<html>nope</html>")).toBe(false);
    expect(getConnectionSnapshot()).toEqual({
      connection: "connected",
      refresh: "degraded",
    });
  });

  it("does not echo a payload error from an authenticated protocol failure", async () => {
    stub(JSON.stringify({ error: "boom" }), 200);
    const failure = await fetchJSON("/api/revisions").catch((error) => error);
    expect(failure).toMatchObject({ kind: "protocol", status: 200 });
    expect(String(failure)).not.toContain("boom");
  });

  it("classifies authenticated non-401 status responses as protocol failures", async () => {
    stub(JSON.stringify({ data: 1 }), 500);
    const failure = await fetchJSON("/api/threads").catch((error) => error);
    expect(failure).toMatchObject({ kind: "protocol", status: 500 });
  });

  it("classifies a wrong promoted schema as connected plus degraded", async () => {
    stub(JSON.stringify({ schema: "wrong", version: 1, eventCount: 1 }));
    const failure = await fetchJSON("/api/freshness").catch((error) => error);
    expect(failure).toMatchObject({ kind: "protocol", status: 200 });
    expect(getConnectionSnapshot()).toEqual({
      connection: "connected",
      refresh: "degraded",
    });
  });

  it("classifies fetch rejection as unreachable without echoing its message", async () => {
    globalThis.fetch = () =>
      Promise.reject(new Error("dial included a secret"));
    const failure = await fetchJSON("/api/history").catch((error) => error);
    expect(failure).toMatchObject({ kind: "unreachable" });
    expect(String(failure)).not.toContain("dial included a secret");
    expect(getConnectionSnapshot().connection).toBe("unreachable");
  });

  it("shares one reconnect prompt and retries each concurrent 401 once", async () => {
    const nextToken = "replacement_secret_0123456789";
    let prompts = 0;
    installAuthCoordinator(
      new AuthCoordinator({
        prompt: async () => {
          prompts += 1;
          return nextToken;
        },
        navigate: () => undefined,
        currentOrigin: () => "http://127.0.0.1:7878",
        currentRoute: () => "#/timeline",
      }),
    );
    let requests = 0;
    globalThis.fetch = ((_input: RequestInfo | URL, init?: RequestInit) => {
      requests += 1;
      const headers = new Headers(init?.headers);
      const correct = headers.get("Authorization") === `Bearer ${nextToken}`;
      return Promise.resolve(
        correct
          ? new Response(
              JSON.stringify({ schema: "pointbreak.inspect-history" }),
            )
          : new Response("", { status: 401 }),
      );
    }) as typeof fetch;

    await expect(
      Promise.all([
        fetchJSON("/api/history"),
        fetchJSON("/api/history?limit=1"),
      ]),
    ).resolves.toHaveLength(2);
    expect(prompts).toBe(1);
    expect(requests).toBe(4);
  });

  it("retries a late stale 401 with the credential renewed by another request", async () => {
    const nextToken = "replacement_secret_9876543210";
    let prompts = 0;
    installAuthCoordinator(
      new AuthCoordinator({
        prompt: async () => {
          prompts += 1;
          return nextToken;
        },
        navigate: () => undefined,
        currentOrigin: () => "http://127.0.0.1:7878",
        currentRoute: () => "#/timeline",
      }),
    );
    let releaseLate!: () => void;
    const lateUnauthorized = new Promise<Response>((resolve) => {
      releaseLate = () => resolve(new Response("", { status: 401 }));
    });
    globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
      const headers = new Headers(init?.headers);
      if (headers.get("Authorization") === `Bearer ${nextToken}`) {
        return Promise.resolve(
          new Response(
            JSON.stringify({ schema: "pointbreak.inspect-history" }),
          ),
        );
      }
      return String(input).includes("late")
        ? lateUnauthorized
        : Promise.resolve(new Response("", { status: 401 }));
    }) as typeof fetch;

    const first = fetchJSON("/api/history");
    const late = fetchJSON("/api/history?late=1");
    await first;
    releaseLate();
    await late;

    expect(prompts).toBe(1);
  });

  it("does not retry or reprompt when reconnect is cancelled", async () => {
    let requests = 0;
    let prompts = 0;
    installAuthCoordinator(
      new AuthCoordinator({
        prompt: async () => {
          prompts += 1;
          return null;
        },
        navigate: () => undefined,
        currentOrigin: () => "http://127.0.0.1:7878",
        currentRoute: () => "#/timeline",
      }),
    );
    globalThis.fetch = () => {
      requests += 1;
      return Promise.resolve(new Response("", { status: 401 }));
    };

    const failure = await fetchJSON("/api/history").catch((error) => error);
    expect(failure).toMatchObject({ kind: "unauthorized", status: 401 });
    expect(requests).toBe(1);
    expect(prompts).toBe(1);
    expect(getConnectionSnapshot().connection).toBe("unauthorized");
  });
});
