import { beforeEach, describe, expect, it, vi } from "vitest";
import { fetchChangeInspectorJSON } from "../src/change-inspector-http";
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
