import { beforeEach, describe, expect, it, vi } from "vitest";

type Access = typeof import("../src/access");
type Store = typeof import("../src/store");

let access: Access;
let store: Store;

beforeEach(async () => {
  vi.resetModules();
  store = await import("../src/store");
  access = await import("../src/access");
});

describe("runWithSelectedAccess", () => {
  it("serializes collection and detail work through one client queue", async () => {
    store.commit({ authoritativeFallback: true });
    const started: string[] = [];
    let releaseFirst!: () => void;
    const firstWait = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });

    const first = access.runWithSelectedAccess(async () => {
      started.push("collection");
      await firstWait;
      return "collection";
    });
    const second = access.runWithSelectedAccess(async () => {
      started.push("detail");
      return "detail";
    });

    await Promise.resolve();
    expect(started).toEqual(["collection"]);
    releaseFirst();
    await expect(Promise.all([first, second])).resolves.toEqual([
      "collection",
      "detail",
    ]);
    expect(started).toEqual(["collection", "detail"]);
  });

  it("retains parallel reads when fallback is not selected", async () => {
    const started: string[] = [];
    let release!: () => void;
    const wait = new Promise<void>((resolve) => {
      release = resolve;
    });
    const operation = (name: string) =>
      access.runWithSelectedAccess(async () => {
        started.push(name);
        await wait;
      });

    const first = operation("first");
    const second = operation("second");
    expect(started).toEqual(["first", "second"]);
    release();
    await Promise.all([first, second]);
  });
});
