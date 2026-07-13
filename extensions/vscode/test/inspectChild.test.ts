import type { ChildProcessWithoutNullStreams } from "node:child_process";
import { EventEmitter as NodeEventEmitter } from "node:events";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { StatusBarItem, WorkspaceFolder } from "vscode";
import type { ResolvedBinary } from "../src/binary";
import {
  InspectChildManager,
  parseInspectStartupLine,
  type SpawnFn,
} from "../src/inspectChild";
import type { InspectClient } from "../src/inspectClient";
import type { InspectConnectionStore } from "../src/inspectConnectionStore";
import type { ResolvedTargetResolution } from "../src/targetResolver";
import { workspaceFolder } from "./helpers/vscodeMock";

const vscodeMocks = vi.hoisted(() => ({
  status: {
    command: undefined as string | undefined,
    dispose: vi.fn(),
    hide: vi.fn(),
    show: vi.fn(),
    text: "",
    tooltip: "",
  },
}));

vi.mock("vscode", () => ({
  EventEmitter: class<T> {
    private listeners: Array<(value: T) => void> = [];
    readonly event = (listener: (value: T) => void) => {
      this.listeners.push(listener);
      return { dispose: vi.fn() };
    };
    fire(value: T): void {
      for (const listener of this.listeners) listener(value);
    }
    dispose = vi.fn();
  },
  StatusBarAlignment: { Left: 1 },
  window: { createStatusBarItem: () => vscodeMocks.status },
}));

beforeEach(() => {
  vscodeMocks.status.command = undefined;
  vscodeMocks.status.dispose.mockReset();
  vscodeMocks.status.hide.mockReset();
  vscodeMocks.status.show.mockReset();
  vscodeMocks.status.text = "";
  vscodeMocks.status.tooltip = "";
});

describe("inspect startup parsing", () => {
  it("accepts the exact loopback startup document", () => {
    expect(parseInspectStartupLine(startupLine())).toEqual({
      schema: "pointbreak.inspect-startup",
      version: 1,
      host: "127.0.0.1",
      port: 63831,
      token: token(),
    });
  });

  it.each([
    "not json",
    "{}",
    JSON.stringify({ ...startup(), schema: "other" }),
    JSON.stringify({ ...startup(), version: 2 }),
    JSON.stringify({ ...startup(), host: "localhost" }),
    JSON.stringify({ ...startup(), host: "0.0.0.0" }),
    JSON.stringify({ ...startup(), port: 0 }),
    JSON.stringify({ ...startup(), port: 65_536 }),
    JSON.stringify({ ...startup(), token: "short" }),
    `${startupLine()}\n${startupLine()}`,
  ])("rejects an invalid or multi-line startup document", (line) => {
    expect(() => parseInspectStartupLine(line)).toThrow(
      "Pointbreak inspect child returned an invalid startup document.",
    );
  });
});

describe("InspectChildManager", () => {
  it("tries one authenticated exact candidate before spawning and attaches on a match", async () => {
    const store = fakeStore({
      record: {
        targetKey: resolution().target.key,
        host: "127.0.0.1",
        port: 63831,
        storeIdentity: resolution().target.storeIdentity,
        contextIdentity: resolution().target.contextIdentity,
      },
      token: token(),
    });
    const spawn = vi.fn<SpawnFn>();
    const client = verifiedClient();
    const manager = managerWith(store, spawn, () => client);
    const events: Array<{ targetKey: string } | undefined> = [];
    manager.onDidChangeSession((event) => events.push(event));

    const session = await manager.ensure(resolution());

    expect(session).toEqual({ targetKey: resolution().target.key, client });
    expect(store.load).toHaveBeenCalledOnce();
    expect(client.verify).toHaveBeenCalledWith({
      storeIdentity: resolution().target.storeIdentity,
      contextIdentity: resolution().target.contextIdentity,
    });
    expect(spawn).not.toHaveBeenCalled();
    expect(events).toEqual([{ targetKey: resolution().target.key }]);
    expect(vscodeMocks.status.text).toContain("repo");
    expect(vscodeMocks.status.text).not.toMatch(/63831|127\.0\.0\.1/);
    expect(vscodeMocks.status.command).toBe("pointbreak.stopInspect");

    await manager.stop();
    expect(events.at(-1)).toBeUndefined();
  });

  it("clears a failed candidate and spawns the exact API-only child", async () => {
    const store = fakeStore({
      record: {
        targetKey: resolution().target.key,
        host: "127.0.0.1",
        port: 63831,
        storeIdentity: resolution().target.storeIdentity,
        contextIdentity: resolution().target.contextIdentity,
      },
      token: token(),
    });
    const child = fakeChild();
    const spawn = vi.fn<SpawnFn>(() => {
      queueMicrotask(() =>
        child.stdout.emit("data", Buffer.from(`${startupLine()}\n`)),
      );
      return child.process;
    });
    const rejected = verifiedClient();
    rejected.verify.mockRejectedValueOnce(new Error("refused secret"));
    const accepted = verifiedClient();
    const clients = [rejected, accepted];
    const manager = managerWith(store, spawn, () => {
      const next = clients.shift();
      if (!next) throw new Error("unexpected client");
      return next;
    });

    await manager.ensure(resolution());

    expect(store.forget).toHaveBeenCalledWith(resolution().target.key);
    expect(spawn).toHaveBeenCalledWith(
      "/custom/shore",
      ["inspect", "--port", "0", "--api-only", "--format", "json"],
      expect.objectContaining({ cwd: "/repo" }),
    );
    const env = spawn.mock.calls[0]?.[2].env;
    expect(env.SHORE_ACTOR_ID).toBeUndefined();
    expect(env.SHORE_FORMAT).toBeUndefined();
    expect(store.remember).toHaveBeenCalledWith(
      expect.objectContaining({
        targetKey: resolution().target.key,
        host: "127.0.0.1",
        port: 63831,
      }),
      token(),
    );

    child.exitOnSignal();
    await manager.stop();
    expect(child.kill).toHaveBeenCalledOnce();
  });

  it("treats credential-store read failures as one missed candidate", async () => {
    const store = fakeStore();
    store.load.mockRejectedValueOnce(new Error("SecretStorage unavailable"));
    const child = fakeChild();
    child.exitOnSignal();
    const spawn = vi.fn<SpawnFn>(() => {
      queueMicrotask(() =>
        child.stdout.emit("data", Buffer.from(`${startupLine()}\n`)),
      );
      return child.process;
    });
    const manager = managerWith(store, spawn, () => verifiedClient());

    await manager.ensure(resolution());

    expect(store.load).toHaveBeenCalledOnce();
    expect(store.forget).toHaveBeenCalledWith(resolution().target.key);
    expect(spawn).toHaveBeenCalledOnce();
    await manager.stop();
  });

  it("accepts a startup document split across stdout chunks", async () => {
    const child = fakeChild();
    child.exitOnSignal();
    const line = `${startupLine()}\n`;
    const spawn = vi.fn<SpawnFn>(() => {
      queueMicrotask(() => {
        child.stdout.emit("data", Buffer.from(line.slice(0, 31)));
        child.stdout.emit("data", Buffer.from(line.slice(31)));
      });
      return child.process;
    });
    const manager = managerWith(fakeStore(), spawn, () => verifiedClient());

    await expect(manager.ensure(resolution())).resolves.toMatchObject({
      targetKey: resolution().target.key,
    });
    await manager.stop();
  });

  it("shares same-target concurrent ensures and tears down before a target switch", async () => {
    const first = fakeChild();
    const second = fakeChild();
    const children = [first, second];
    const spawn = vi.fn<SpawnFn>(() => {
      const child = children.shift();
      if (!child) throw new Error("unexpected spawn");
      queueMicrotask(() =>
        child.stdout.emit("data", Buffer.from(`${startupLine()}\n`)),
      );
      return child.process;
    });
    const manager = managerWith(fakeStore(), spawn, () => verifiedClient());

    const one = manager.ensure(resolution());
    const duplicate = manager.ensure(resolution());
    expect(await one).toBe(await duplicate);
    expect(spawn).toHaveBeenCalledTimes(1);

    first.exitOnSignal();
    const switched = await manager.ensure(resolution("other", "/other"));
    expect(first.kill.mock.invocationCallOrder[0]).toBeLessThan(
      spawn.mock.invocationCallOrder[1] ?? Number.POSITIVE_INFINITY,
    );
    expect(switched.targetKey).toBe("store/context-other");
    expect(spawn).toHaveBeenCalledTimes(2);

    second.exitOnSignal();
    await manager.stop();
  });

  it("rejects overflow, timeout, and exit-before-handshake without disclosing startup data", async () => {
    vi.useFakeTimers();
    try {
      const timeoutChild = fakeChild();
      const timeoutManager = managerWith(
        fakeStore(),
        vi.fn<SpawnFn>(() => timeoutChild.process),
        () => verifiedClient(),
        { startupTimeoutMs: 25, stopTimeoutMs: 1 },
      );
      const timedOut = timeoutManager.ensure(resolution());
      await vi.advanceTimersByTimeAsync(25);
      await vi.runAllTimersAsync();
      await expect(timedOut).rejects.toThrow(
        "Pointbreak inspect child did not complete startup.",
      );
      expect(timeoutChild.kill.mock.calls.map(([signal]) => signal)).toEqual([
        "SIGINT",
        "SIGKILL",
      ]);

      const overflowChild = fakeChild();
      overflowChild.exitOnSignal();
      const overflowManager = managerWith(
        fakeStore(),
        vi.fn<SpawnFn>(() => {
          queueMicrotask(() =>
            overflowChild.stdout.emit("data", Buffer.alloc(20_000, "x")),
          );
          return overflowChild.process;
        }),
        () => verifiedClient(),
        { stopTimeoutMs: 1 },
      );
      await expect(overflowManager.ensure(resolution())).rejects.toThrow(
        "Pointbreak inspect child returned an invalid startup document.",
      );

      const exitedChild = fakeChild();
      const exitedManager = managerWith(
        fakeStore(),
        vi.fn<SpawnFn>(() => {
          queueMicrotask(() => exitedChild.emitExit(2));
          return exitedChild.process;
        }),
        () => verifiedClient(),
      );
      const error = await exitedManager
        .ensure(resolution())
        .catch((caught) => caught);
      expect(error.message).toBe(
        "Pointbreak inspect child exited before startup completed.",
      );
      expect(error.message).not.toMatch(/secret|127\.0\.0\.1|63831|\/repo/);
    } finally {
      vi.useRealTimers();
    }
  });

  it("kills owned children, detaches attached sessions, and recovers after a crash", async () => {
    const attachedStore = fakeStore({
      record: {
        targetKey: resolution().target.key,
        host: "127.0.0.1",
        port: 63831,
        storeIdentity: resolution().target.storeIdentity,
        contextIdentity: resolution().target.contextIdentity,
      },
      token: token(),
    });
    const attachSpawn = vi.fn<SpawnFn>();
    const attached = managerWith(attachedStore, attachSpawn, () =>
      verifiedClient(),
    );
    await attached.ensure(resolution());
    await attached.stop();
    expect(attachSpawn).not.toHaveBeenCalled();

    const first = fakeChild();
    const second = fakeChild();
    const children = [first, second];
    const owned = managerWith(
      fakeStore(),
      vi.fn<SpawnFn>(() => {
        const child = children.shift();
        if (!child) throw new Error("unexpected spawn");
        queueMicrotask(() =>
          child.stdout.emit("data", Buffer.from(`${startupLine()}\n`)),
        );
        return child.process;
      }),
      () => verifiedClient(),
    );
    const events: Array<{ targetKey: string } | undefined> = [];
    owned.onDidChangeSession((event) => events.push(event));
    await owned.ensure(resolution());
    first.emitExit(1);
    await vi.waitFor(() => expect(events.at(-1)).toBeUndefined());
    await owned.ensure(resolution());
    second.exitOnSignal();
    owned.dispose();
    await vi.waitFor(() => expect(second.kill).toHaveBeenCalledOnce());
    expect(vscodeMocks.status.dispose).toHaveBeenCalledOnce();
  });
});

function managerWith(
  store: InspectConnectionStore,
  spawn: SpawnFn,
  clientFactory: () => InspectClient,
  options: { startupTimeoutMs?: number; stopTimeoutMs?: number } = {},
): InspectChildManager {
  return new InspectChildManager(binary(), store, {
    spawn,
    clientFactory,
    statusBar: vscodeMocks.status as unknown as StatusBarItem,
    ...options,
  });
}

function binary(): ResolvedBinary {
  return { path: "/custom/shore", source: "setting" };
}

function resolution(
  suffix = "",
  folderPath = "/repo",
): ResolvedTargetResolution {
  return {
    kind: "resolved",
    folder: workspaceFolder(
      folderPath,
      suffix ? `repo-${suffix}` : "repo",
    ) as WorkspaceFolder,
    target: {
      key: `store/context${suffix ? `-${suffix}` : ""}`,
      label: suffix ? `repo-${suffix}` : "repo",
      storeIdentity: "store",
      contextIdentity: suffix ? `context-${suffix}` : "context",
    },
    emptyInventory: false,
  };
}

function token(): string {
  return "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-";
}

function startup() {
  return {
    schema: "pointbreak.inspect-startup",
    version: 1,
    host: "127.0.0.1",
    port: 63831,
    token: token(),
  };
}

function startupLine(): string {
  return JSON.stringify(startup());
}

function verifiedClient(): InspectClient & {
  verify: ReturnType<typeof vi.fn>;
} {
  return {
    verify: vi.fn(async () => undefined),
  } as unknown as InspectClient & { verify: ReturnType<typeof vi.fn> };
}

function fakeStore(candidate?: {
  record: {
    targetKey: string;
    host: string;
    port: number;
    storeIdentity: string;
    contextIdentity: string;
  };
  token: string;
}): InspectConnectionStore & {
  load: ReturnType<typeof vi.fn>;
  remember: ReturnType<typeof vi.fn>;
  forget: ReturnType<typeof vi.fn>;
} {
  return {
    load: vi.fn(async () => candidate),
    remember: vi.fn(async () => undefined),
    forget: vi.fn(async () => undefined),
  } as unknown as InspectConnectionStore & {
    load: ReturnType<typeof vi.fn>;
    remember: ReturnType<typeof vi.fn>;
    forget: ReturnType<typeof vi.fn>;
  };
}

function fakeChild() {
  const process = new NodeEventEmitter() as ChildProcessWithoutNullStreams;
  const stdout = new NodeEventEmitter();
  const stderr = new NodeEventEmitter();
  const stdin = new NodeEventEmitter();
  Object.assign(process, {
    stdout,
    stderr,
    stdin,
    exitCode: null,
    signalCode: null,
  });
  const kill = vi.fn((signal?: NodeJS.Signals | number) => {
    if (signal === "SIGKILL") {
      queueMicrotask(() => emitExit(0));
    }
    return true;
  });
  process.kill = kill;
  const emitExit = (code: number | null) => {
    Object.defineProperty(process, "exitCode", {
      value: code,
      configurable: true,
    });
    process.emit("exit", code, null);
  };
  const exitOnSignal = () => {
    kill.mockImplementationOnce(() => {
      queueMicrotask(() => emitExit(0));
      return true;
    });
  };
  return { process, stdout, stderr, stdin, kill, emitExit, exitOnSignal };
}
