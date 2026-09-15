import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  fetchChangeInspectorJSON,
  fetchChangeInspectorResponse,
} from "../src/change-inspector-http";
import { markRequestFailure, markRequestSuccess } from "../src/connection";

vi.mock("../src/auth", () => ({
  getSessionToken: () => null,
  recoverUnauthorized: vi.fn(async () => false),
  sessionCredentialVersion: () => 0,
}));

vi.mock("../src/connection", () => ({
  markRequestFailure: vi.fn(),
  markRequestSuccess: vi.fn(),
}));

const requestPath = "/api/v2/profile";

beforeEach(() => {
  vi.mocked(markRequestFailure).mockClear();
  vi.mocked(markRequestSuccess).mockClear();
  vi.restoreAllMocks();
});

describe("Change Inspector HTTP cancellation", () => {
  it("passes the caller's signal to fetch", async () => {
    const controller = new AbortController();
    let observedSignal: AbortSignal | null | undefined;
    globalThis.fetch = vi.fn(async (_input, init) => {
      observedSignal = init?.signal;
      return new Response(JSON.stringify({ ready: true }));
    }) as typeof fetch;

    await fetchChangeInspectorJSON(requestPath, {
      signal: controller.signal,
    });

    expect(observedSignal).toBe(controller.signal);
    expect(markRequestSuccess).toHaveBeenCalledOnce();
  });

  it("maps a fetch-rejection abort to the typed aborted failure", async () => {
    const controller = new AbortController();
    globalThis.fetch = vi.fn(async () => {
      throw new DOMException("aborted", "AbortError");
    }) as typeof fetch;

    const pending = fetchChangeInspectorJSON(requestPath, {
      signal: controller.signal,
    });
    controller.abort();

    await expect(pending).rejects.toMatchObject({
      kind: "aborted",
      message: "request cancelled",
    });
    expect(markRequestFailure).not.toHaveBeenCalled();
    expect(markRequestSuccess).not.toHaveBeenCalled();
  });

  it("maps a body-read abort to the typed aborted failure", async () => {
    const controller = new AbortController();
    globalThis.fetch = vi.fn(async () => {
      return {
        ok: true,
        status: 200,
        text: async () => {
          throw new DOMException("aborted", "AbortError");
        },
      } as unknown as Response;
    }) as typeof fetch;

    const pending = fetchChangeInspectorJSON(requestPath, {
      signal: controller.signal,
    });
    controller.abort();

    await expect(pending).rejects.toMatchObject({ kind: "aborted" });
    expect(markRequestFailure).not.toHaveBeenCalled();
    expect(markRequestSuccess).not.toHaveBeenCalled();
  });

  it("an abort after parsing marks neither request success nor failure", async () => {
    const controller = new AbortController();
    globalThis.fetch = vi.fn(async () => {
      return {
        ok: true,
        status: 200,
        text: async () => {
          controller.abort();
          return JSON.stringify({ ready: true });
        },
      } as unknown as Response;
    }) as typeof fetch;

    await expect(
      fetchChangeInspectorJSON(requestPath, { signal: controller.signal }),
    ).rejects.toMatchObject({ kind: "aborted" });
    expect(markRequestFailure).not.toHaveBeenCalled();
    expect(markRequestSuccess).not.toHaveBeenCalled();
  });

  it.each([
    500, 200,
  ])("late error body at status %s remains aborted without health marks", async (status) => {
    const controller = new AbortController();
    let finish!: (body: string) => void;
    const body = new Promise<string>((resolve) => {
      finish = resolve;
    });
    const text = vi.fn(() => body);
    globalThis.fetch = vi.fn(
      async () => ({ ok: status === 200, status, text }) as unknown as Response,
    ) as typeof fetch;
    const pending = fetchChangeInspectorJSON(requestPath, {
      signal: controller.signal,
    });
    await vi.waitFor(() => expect(text).toHaveBeenCalledOnce());
    controller.abort();
    finish(JSON.stringify({ error: "late failure" }));
    await expect(pending).rejects.toMatchObject({ kind: "aborted" });
    expect(markRequestSuccess).not.toHaveBeenCalled();
    expect(markRequestFailure).not.toHaveBeenCalled();
  });

  it("an abort observed with a 401 skips unauthorized recovery", async () => {
    const controller = new AbortController();
    globalThis.fetch = vi.fn(async () => {
      controller.abort();
      return new Response(JSON.stringify({ error: "unauthorized" }), {
        status: 401,
      });
    }) as typeof fetch;
    const { recoverUnauthorized } = await import("../src/auth");
    vi.mocked(recoverUnauthorized).mockClear();

    await expect(
      fetchChangeInspectorJSON(requestPath, { signal: controller.signal }),
    ).rejects.toMatchObject({ kind: "aborted" });
    expect(recoverUnauthorized).not.toHaveBeenCalled();
    expect(markRequestFailure).not.toHaveBeenCalled();
    expect(markRequestSuccess).not.toHaveBeenCalled();
  });

  it("an abort during declined recovery never marks the connection failed", async () => {
    const controller = new AbortController();
    let finishRecovery: (recovered: boolean) => void = () => undefined;
    const recovery = new Promise<boolean>((resolve) => {
      finishRecovery = resolve;
    });
    globalThis.fetch = vi.fn(async () => {
      return new Response(JSON.stringify({ error: "unauthorized" }), {
        status: 401,
      });
    }) as typeof fetch;
    const { recoverUnauthorized } = await import("../src/auth");
    vi.mocked(recoverUnauthorized).mockImplementationOnce(async () => recovery);

    const pending = fetchChangeInspectorJSON(requestPath, {
      signal: controller.signal,
    });
    await vi.waitFor(() => expect(recoverUnauthorized).toHaveBeenCalledOnce());
    controller.abort();
    finishRecovery(false);

    await expect(pending).rejects.toMatchObject({ kind: "aborted" });
    expect(markRequestFailure).not.toHaveBeenCalled();
    expect(markRequestSuccess).not.toHaveBeenCalled();
  });

  it("reuses the same signal after one unauthorized recovery", async () => {
    const controller = new AbortController();
    const observedSignals: Array<AbortSignal | null | undefined> = [];
    globalThis.fetch = vi
      .fn()
      .mockImplementationOnce(async (_input, init) => {
        observedSignals.push(init?.signal);
        return new Response(JSON.stringify({ error: "unauthorized" }), {
          status: 401,
        });
      })
      .mockImplementationOnce(async (_input, init) => {
        observedSignals.push(init?.signal);
        return new Response(JSON.stringify({ ready: true }));
      }) as typeof fetch;
    const { recoverUnauthorized } = await import("../src/auth");
    vi.mocked(recoverUnauthorized).mockResolvedValueOnce(true);

    await fetchChangeInspectorJSON(requestPath, {
      signal: controller.signal,
    });

    expect(observedSignals).toEqual([controller.signal, controller.signal]);
  });
});

describe("Change Inspector recovery transport", () => {
  it("adds one explicit selector only to electable entry routes", async () => {
    const paths: string[] = [];
    globalThis.fetch = vi.fn(async (input) => {
      paths.push(String(input));
      return new Response(JSON.stringify({ ready: true }), {
        headers: { "X-Pointbreak-Access-Source": "authoritative-fallback" },
      });
    }) as typeof fetch;

    await fetchChangeInspectorResponse("/api/v2/changes?order=desc", {
      access: "authoritative",
    });
    await fetchChangeInspectorResponse("/api/v2/changes/change%3Aone", {
      access: "authoritative",
    });

    expect(paths).toEqual([
      "/api/v2/changes?order=desc&access=authoritative",
      "/api/v2/changes/change%3Aone",
    ]);
  });

  it("preserves the validated response source", async () => {
    globalThis.fetch = vi.fn(
      async () =>
        new Response(JSON.stringify({ ready: true }), {
          headers: { "X-Pointbreak-Access-Source": "authoritative-fallback" },
        }),
    ) as typeof fetch;
    await expect(
      fetchChangeInspectorResponse("/api/v2/profile", {
        access: "authoritative",
      }),
    ).resolves.toEqual({
      value: { ready: true },
      accessSource: "authoritative-fallback",
    });
  });

  it("serializes authoritative entry reads while derived reads stay parallel", async () => {
    let active = 0;
    let maxActive = 0;
    const releases: Array<() => void> = [];
    globalThis.fetch = vi.fn(async () => {
      active += 1;
      maxActive = Math.max(maxActive, active);
      await new Promise<void>((resolve) => releases.push(resolve));
      active -= 1;
      return new Response(JSON.stringify({ ready: true }), {
        headers: { "X-Pointbreak-Access-Source": "authoritative-fallback" },
      });
    }) as typeof fetch;

    const first = fetchChangeInspectorResponse("/api/v2/profile", {
      access: "authoritative",
    });
    const second = fetchChangeInspectorResponse("/api/v2/changes", {
      access: "authoritative",
    });
    await vi.waitFor(() => expect(releases).toHaveLength(1));
    releases.shift()?.();
    await vi.waitFor(() => expect(releases).toHaveLength(1));
    releases.shift()?.();
    await Promise.all([first, second]);
    expect(maxActive).toBe(1);

    const derived = [
      fetchChangeInspectorResponse("/api/v2/profile", { access: "derived" }),
      fetchChangeInspectorResponse("/api/v2/changes", { access: "derived" }),
    ];
    await vi.waitFor(() => expect(releases).toHaveLength(2));
    for (const release of releases.splice(0)) release();
    await Promise.all(derived);
    expect(maxActive).toBe(2);
  });

  it("never replays an ambiguously admitted control POST", async () => {
    globalThis.fetch = vi.fn(
      async () =>
        new Response(JSON.stringify({ error: "unauthorized" }), {
          status: 401,
        }),
    ) as typeof fetch;
    const { recoverUnauthorized } = await import("../src/auth");
    vi.mocked(recoverUnauthorized).mockResolvedValueOnce(true);

    await expect(
      fetchChangeInspectorResponse("/api/derived-access/retry", {
        method: "POST",
      }),
    ).rejects.toMatchObject({ kind: "unauthorized" });
    expect(fetch).toHaveBeenCalledOnce();
  });

  it("surfaces typed projection failure without treating a label as success", async () => {
    globalThis.fetch = vi.fn(
      async () =>
        new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-change-projection-error",
            version: 1,
            code: "projection_invalid",
            message: "projection invalid",
            retryable: false,
          }),
          {
            status: 503,
            headers: { "X-Pointbreak-Access-Source": "authoritative-fallback" },
          },
        ),
    ) as typeof fetch;
    await expect(
      fetchChangeInspectorResponse("/api/v2/profile", {
        access: "authoritative",
      }),
    ).rejects.toMatchObject({
      kind: "projection",
      status: 503,
      message: "projection invalid",
    });
  });
});
