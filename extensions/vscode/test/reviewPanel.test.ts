import { beforeEach, describe, expect, it, vi } from "vitest";
import type { WorkspaceFolder } from "vscode";
import snapshotFixture from "../../../src/cli/inspect/web/test/fixtures/snapshot.json";
import type { ReviewSnapshotDoc } from "../src/cli";
import type { DiffDataSource, DiffRenderData } from "../src/diffDataSource";
import {
  type ReviewPanelLocation,
  ReviewPanelManager,
} from "../src/reviewPanel";
import type { ResolvedTargetResolution } from "../src/targetResolver";
import { workspaceFolder } from "./helpers/vscodeMock";

const vscodeMocks = vi.hoisted(() => {
  class Uri {
    constructor(readonly path: string) {}

    static joinPath(base: Uri, ...parts: string[]): Uri {
      return new Uri([base.path.replace(/\/$/, ""), ...parts].join("/"));
    }

    toString(): string {
      return `file://${this.path}`;
    }
  }

  class EventEmitter<T> {
    private readonly listeners: Array<(value: T) => void> = [];
    readonly event = (listener: (value: T) => void) => {
      this.listeners.push(listener);
      return { dispose: vi.fn() };
    };
    fire(value: T): void {
      for (const listener of this.listeners) listener(value);
    }
    dispose = vi.fn();
  }

  class MockPanel {
    readonly reveal = vi.fn();
    readonly dispose = vi.fn(() => {
      for (const listener of this.disposeListeners) listener();
    });
    readonly disposeListeners: Array<() => void> = [];
    readonly viewStateListeners: Array<
      (event: { webviewPanel: MockPanel }) => void
    > = [];
    readonly messageListeners: Array<(message: unknown) => void> = [];
    title = "";
    visible = true;
    readonly webview = {
      cspSource: "vscode-webview://panel",
      html: "",
      asWebviewUri: vi.fn((uri: Uri) => ({
        toString: () => `vscode-resource:${uri.path}`,
      })),
      postMessage: vi.fn(async () => true),
      onDidReceiveMessage: vi.fn((listener: (message: unknown) => void) => {
        this.messageListeners.push(listener);
        return { dispose: vi.fn() };
      }),
    };
    onDidDispose(listener: () => void) {
      this.disposeListeners.push(listener);
      return { dispose: vi.fn() };
    }
    onDidChangeViewState(
      listener: (event: { webviewPanel: MockPanel }) => void,
    ) {
      this.viewStateListeners.push(listener);
      return { dispose: vi.fn() };
    }
    emitMessage(message: unknown): void {
      for (const listener of this.messageListeners) listener(message);
    }
    setVisible(visible: boolean): void {
      this.visible = visible;
      for (const listener of this.viewStateListeners) {
        listener({ webviewPanel: this });
      }
    }
  }

  const panels: MockPanel[] = [];
  const createWebviewPanel = vi.fn((..._args: unknown[]) => {
    const panel = new MockPanel();
    panels.push(panel);
    return panel;
  });

  return { createWebviewPanel, EventEmitter, MockPanel, panels, Uri };
});

vi.mock("vscode", () => ({
  EventEmitter: vscodeMocks.EventEmitter,
  Uri: vscodeMocks.Uri,
  ViewColumn: { Active: -1 },
  window: { createWebviewPanel: vscodeMocks.createWebviewPanel },
}));

beforeEach(() => {
  vscodeMocks.createWebviewPanel.mockClear();
  vscodeMocks.panels.splice(0);
});

describe("ReviewPanelManager", () => {
  it("creates one lazy panel and separates reveal, focus, and data navigation", async () => {
    const load = vi.fn(async ({ revisionId }) => data(revisionId));
    const manager = createManager({ load });
    const first = location("a", "rev:sha256:one");

    await manager.open(first, { preserveFocus: true });
    const panel = vscodeMocks.panels[0];
    panel.emitMessage({ type: "ready" });
    await settled();

    expect(vscodeMocks.createWebviewPanel).toHaveBeenCalledTimes(1);
    expect(vscodeMocks.createWebviewPanel.mock.calls[0]?.[2]).toEqual({
      viewColumn: -1,
      preserveFocus: true,
    });
    expect(load).toHaveBeenCalledTimes(1);
    expect(panel.webview.postMessage).toHaveBeenLastCalledWith(
      expect.objectContaining({ type: "render" }),
    );

    await manager.open(first, { preserveFocus: true });
    expect(load).toHaveBeenCalledTimes(1);
    expect(panel.reveal).toHaveBeenLastCalledWith(-1, true);

    await manager.open({
      ...first,
      focus: { kind: "attention", id: "open_input_request:request:one" },
    });
    expect(load).toHaveBeenCalledTimes(1);
    expect(panel.webview.postMessage).toHaveBeenLastCalledWith({
      type: "focus",
      focus: { kind: "attention", id: "open_input_request:request:one" },
    });

    const second = location("a", "rev:sha256:two");
    const navigation = manager.open(second);
    panel.emitMessage({ type: "ready" });
    await navigation;
    await settled();
    expect(vscodeMocks.createWebviewPanel).toHaveBeenCalledTimes(1);
    expect(load).toHaveBeenCalledTimes(2);
    expect(panel.webview.postMessage).toHaveBeenLastCalledWith(
      expect.objectContaining({
        type: "render",
        data: expect.objectContaining({ revisionId: second.revisionId }),
      }),
    );
  });

  it("lets only the latest overlapping location render or fail", async () => {
    const oldLoad = deferred<DiffRenderData>();
    const newLoad = deferred<DiffRenderData>();
    const load = vi
      .fn<DiffDataSource["load"]>()
      .mockReturnValueOnce(oldLoad.promise)
      .mockReturnValueOnce(newLoad.promise);
    const manager = createManager({ load });

    const oldOpen = manager.open(location("a", "rev:sha256:old"));
    const panel = vscodeMocks.panels[0];
    const newLocation = {
      ...location("b", "rev:sha256:new"),
      focus: { kind: "attention", id: "stale_assessment:assess:new" } as const,
    };
    const newOpen = manager.open(newLocation);
    panel.emitMessage({ type: "ready" });
    oldLoad.reject(new Error("/private/repo token=secret 127.0.0.1:7878"));
    newLoad.resolve(data(newLocation.revisionId));
    await Promise.all([oldOpen, newOpen]);
    await settled();

    expect(panel.webview.postMessage).toHaveBeenCalledTimes(1);
    expect(panel.webview.postMessage).toHaveBeenCalledWith({
      type: "render",
      data: expect.objectContaining({ revisionId: newLocation.revisionId }),
      focus: newLocation.focus,
    });
    expect(JSON.stringify(panel.webview.postMessage.mock.calls)).not.toMatch(
      /private|secret|127\.0\.0\.1|7878/,
    );
  });

  it("clears active work on close, recreates cleanly, and reloads only when active", async () => {
    const pending = deferred<DiffRenderData>();
    const load = vi
      .fn<DiffDataSource["load"]>()
      .mockReturnValueOnce(pending.promise)
      .mockResolvedValue(data("rev:sha256:reopened"));
    const manager = createManager({ load });
    const firstOpen = manager.open(location("a", "rev:sha256:first"));
    const firstPanel = vscodeMocks.panels[0];
    firstPanel.dispose();
    pending.resolve(data("rev:sha256:first"));
    await firstOpen;

    await manager.reloadActive();
    expect(load).toHaveBeenCalledTimes(1);

    const reopen = manager.open(location("a", "rev:sha256:reopened"));
    const secondPanel = vscodeMocks.panels[1];
    secondPanel.emitMessage({ type: "ready" });
    await reopen;
    await settled();
    expect(vscodeMocks.createWebviewPanel).toHaveBeenCalledTimes(2);
    expect(secondPanel.webview.postMessage).toHaveBeenCalledWith(
      expect.objectContaining({ type: "render" }),
    );

    await manager.reloadActive();
    expect(load).toHaveBeenCalledTimes(3);
  });

  it("uses a local-only CSP and exposes stamped visibility without panel internals", async () => {
    const manager = createManager({ load: vi.fn(async () => data("rev:one")) });
    const visible: boolean[] = [];
    manager.onDidChangeVisibility((value) => visible.push(value));
    await manager.open(location("a", "rev:one"));
    const panel = vscodeMocks.panels[0];
    const options = vscodeMocks.createWebviewPanel.mock.calls[0]?.[3];

    expect(options).toMatchObject({
      enableScripts: true,
      localResourceRoots: [expect.objectContaining({ path: "/extension/out" })],
    });
    expect(panel.webview.html).toContain("default-src 'none'");
    expect(panel.webview.html).toContain(
      "vscode-resource:/extension/out/review.js",
    );
    expect(panel.webview.html).toContain(
      "vscode-resource:/extension/out/review.css",
    );
    expect(panel.webview.html).not.toMatch(
      /https?:|127\.0\.0\.1|token|\/private\/repo/,
    );

    panel.setVisible(false);
    panel.setVisible(true);
    expect(visible).toEqual([false, true]);
  });

  it("keys persisted controller state by the complete host data location", async () => {
    const manager = createManager({
      load: vi.fn(async ({ revisionId }) => data(revisionId)),
    });
    await manager.open(location("a", "rev:same"));
    const panel = vscodeMocks.panels[0];
    const firstKey = panel.webview.html.match(
      /data-location-key="([^"]+)"/,
    )?.[1];

    await manager.open(location("b", "rev:same"));
    const secondKey = panel.webview.html.match(
      /data-location-key="([^"]+)"/,
    )?.[1];

    expect(firstKey).toMatch(/^[a-f0-9]{64}$/);
    expect(secondKey).toMatch(/^[a-f0-9]{64}$/);
    expect(secondKey).not.toBe(firstKey);
    expect(panel.webview.html).not.toContain("store:b/context:b");
  });
});

function createManager(source: DiffDataSource): ReviewPanelManager {
  return new ReviewPanelManager(
    new vscodeMocks.Uri("/extension") as never,
    source,
  );
}

function location(target: string, revisionId: string): ReviewPanelLocation {
  return { resolution: resolution(target), revisionId };
}

function resolution(key: string): ResolvedTargetResolution {
  return {
    kind: "resolved",
    folder: workspaceFolder(`/private/${key}`, key) as WorkspaceFolder,
    target: {
      key,
      label: key,
      storeIdentity: `store:${key}`,
      contextIdentity: `context:${key}`,
    },
    emptyInventory: false,
  };
}

function data(revisionId: string): DiffRenderData {
  return {
    revisionId,
    snapshotId: `obj:${revisionId}`,
    artifact: snapshotFixture as ReviewSnapshotDoc,
    annotations: [],
  };
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((next, fail) => {
    resolve = next;
    reject = fail;
  });
  return { promise, reject, resolve };
}

async function settled(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}
