import { describe, expect, it, vi } from "vitest";
import type { Memento } from "vscode";
import {
  type ReviewServerRecord,
  ReviewServerRegistry,
  reviewServerUrl,
} from "../src/reviewServerRegistry";

describe("ReviewServerRegistry", () => {
  it("loads valid workspace records and ignores malformed entries", () => {
    const state = memento({
      version: 1,
      servers: [record(), { ...record(), targetKey: "bad", port: 0 }],
    });

    const registry = new ReviewServerRegistry(state);

    expect(registry.entries()).toEqual([record()]);
    expect(reviewServerUrl(record())).toBe("http://127.0.0.1:63831");
  });

  it("replaces the port for one exact target without disturbing others", async () => {
    const other = record({ targetKey: "store/context-other", port: 64000 });
    const state = memento({ version: 1, servers: [record(), other] });
    const registry = new ReviewServerRegistry(state);

    await registry.remember(record({ port: 65000 }));

    expect(registry.entries()).toEqual([record({ port: 65000 }), other]);
    expect(state.update).toHaveBeenCalledWith("pointbreak.reviewServers", {
      version: 1,
      servers: [record({ port: 65000 }), other],
    });
  });

  it("floors an unknown state version to an empty registry", () => {
    const registry = new ReviewServerRegistry(
      memento({ version: 2, servers: [record()] }),
    );

    expect(registry.entries()).toEqual([]);
  });

  it("allows a later write after workspace persistence fails", async () => {
    const state = memento({ version: 1, servers: [] });
    state.update
      .mockRejectedValueOnce(new Error("storage unavailable"))
      .mockResolvedValueOnce(undefined);
    const registry = new ReviewServerRegistry(state);

    await expect(registry.remember(record())).rejects.toThrow(
      "storage unavailable",
    );
    await expect(
      registry.remember(record({ port: 64000 })),
    ).resolves.toBeUndefined();

    expect(state.update).toHaveBeenCalledTimes(2);
  });
});

function record(
  overrides: Partial<ReviewServerRecord> = {},
): ReviewServerRecord {
  return {
    targetKey: "store/context",
    storeIdentity: "store",
    contextIdentity: "context",
    folderUri: "file:///repo",
    port: 63831,
    ...overrides,
  };
}

function memento(
  value: unknown,
): Memento & { update: ReturnType<typeof vi.fn> } {
  return {
    keys: () => ["pointbreak.reviewServers"],
    get: vi.fn(() => value) as Memento["get"],
    update: vi.fn(async () => undefined),
  };
}
