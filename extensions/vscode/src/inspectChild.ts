import {
  type ChildProcessWithoutNullStreams,
  spawn as nodeSpawn,
} from "node:child_process";
import { isIP } from "node:net";
import {
  type Event,
  EventEmitter,
  StatusBarAlignment,
  type StatusBarItem,
  window,
} from "vscode";
import type { ResolvedBinary } from "./binary";
import { sanitizedEnv } from "./cli";
import { InspectClient } from "./inspectClient";
import type {
  InspectConnectionRecord,
  InspectConnectionStore,
} from "./inspectConnectionStore";
import type { ResolvedTargetResolution } from "./targetResolver";

const STARTUP_TIMEOUT_MS = 10_000;
const STOP_TIMEOUT_MS = 2_000;
const MAX_STARTUP_BYTES = 16 * 1024;
const STARTUP_SCHEMA = "pointbreak.inspect-startup";
const TOKEN_PATTERN = /^[A-Za-z0-9_-]{43,}$/;

export type SpawnFn = (
  file: string,
  args: string[],
  opts: { cwd: string; env: NodeJS.ProcessEnv },
) => ChildProcessWithoutNullStreams;

export interface InspectSession {
  readonly targetKey: string;
  readonly client: InspectClient;
}

interface InspectStartupDocument {
  readonly schema: typeof STARTUP_SCHEMA;
  readonly version: 1;
  readonly host: string;
  readonly port: number;
  readonly token: string;
}

interface OwnedRuntime {
  readonly session: InspectSession;
  readonly process: ChildProcessWithoutNullStreams;
  readonly exit: ProcessExit;
}

interface AttachedRuntime {
  readonly session: InspectSession;
}

type ManagerState =
  | { readonly kind: "idle" }
  | { readonly kind: "connecting"; readonly targetKey: string }
  | { readonly kind: "running-owned"; readonly runtime: OwnedRuntime }
  | { readonly kind: "running-attached"; readonly runtime: AttachedRuntime }
  | {
      readonly kind: "stopping";
      readonly runtime: OwnedRuntime | AttachedRuntime;
    };

interface InspectChildManagerOptions {
  readonly spawn?: SpawnFn;
  readonly clientFactory?: (origin: string, token: string) => InspectClient;
  readonly statusBar?: StatusBarItem;
  readonly startupTimeoutMs?: number;
  readonly stopTimeoutMs?: number;
}

interface ProcessExit {
  readonly promise: Promise<void>;
  readonly exited: () => boolean;
}

/** Owns the one lazy authenticated inspect connection for this VS Code window. */
export class InspectChildManager {
  private readonly sessionEmitter = new EventEmitter<
    { targetKey: string } | undefined
  >();
  readonly onDidChangeSession: Event<{ targetKey: string } | undefined> =
    this.sessionEmitter.event;

  private readonly spawn: SpawnFn;
  private readonly clientFactory: (
    origin: string,
    token: string,
  ) => InspectClient;
  private readonly statusBar: StatusBarItem;
  private readonly startupTimeoutMs: number;
  private readonly stopTimeoutMs: number;
  private state: ManagerState = { kind: "idle" };
  private operations: Promise<void> = Promise.resolve();
  private disposed = false;
  private sessionPublished = false;

  constructor(
    private readonly binary: ResolvedBinary,
    private readonly store: InspectConnectionStore,
    options: InspectChildManagerOptions = {},
  ) {
    this.spawn = options.spawn ?? defaultSpawn;
    this.clientFactory =
      options.clientFactory ??
      ((origin, token) => new InspectClient(origin, token));
    this.statusBar =
      options.statusBar ?? window.createStatusBarItem(StatusBarAlignment.Left);
    this.startupTimeoutMs = options.startupTimeoutMs ?? STARTUP_TIMEOUT_MS;
    this.stopTimeoutMs = options.stopTimeoutMs ?? STOP_TIMEOUT_MS;
    this.statusBar.command = "pointbreak.stopInspect";
    this.statusBar.hide();
  }

  ensure(resolution: ResolvedTargetResolution): Promise<InspectSession> {
    return this.serialize(async () => {
      if (this.disposed) {
        throw new Error("Pointbreak inspect manager is no longer available.");
      }
      return this.ensureExclusive(resolution);
    });
  }

  stop(): Promise<void> {
    return this.serialize(() => this.stopExclusive());
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    void this.stop().finally(() => {
      this.sessionEmitter.dispose();
      this.statusBar.dispose();
    });
  }

  private async ensureExclusive(
    resolution: ResolvedTargetResolution,
  ): Promise<InspectSession> {
    const running = runningSession(this.state);
    if (running?.targetKey === resolution.target.key) {
      return running;
    }
    if (running || this.state.kind === "stopping") {
      await this.stopExclusive();
    }

    this.state = { kind: "connecting", targetKey: resolution.target.key };
    try {
      const attached = await this.attachRemembered(resolution);
      if (attached) {
        this.state = {
          kind: "running-attached",
          runtime: { session: attached },
        };
        this.publishSession(resolution, attached);
        return attached;
      }
      return await this.startOwned(resolution);
    } catch (error) {
      this.state = { kind: "idle" };
      this.clearSession();
      throw error;
    }
  }

  private async attachRemembered(
    resolution: ResolvedTargetResolution,
  ): Promise<InspectSession | undefined> {
    let candidate: Awaited<ReturnType<InspectConnectionStore["load"]>>;
    try {
      candidate = await this.store.load(resolution.target.key);
    } catch {
      await this.forget(resolution.target.key);
      return undefined;
    }
    if (!candidate) {
      return undefined;
    }
    if (
      !recordMatchesResolution(candidate.record, resolution) ||
      !isLoopbackIp(candidate.record.host) ||
      !TOKEN_PATTERN.test(candidate.token)
    ) {
      await this.forget(resolution.target.key);
      return undefined;
    }

    try {
      const client = this.clientFactory(
        inspectOrigin(candidate.record.host, candidate.record.port),
        candidate.token,
      );
      await client.verify(targetIdentity(resolution));
      return { targetKey: resolution.target.key, client };
    } catch {
      await this.forget(resolution.target.key);
      return undefined;
    }
  }

  private async startOwned(
    resolution: ResolvedTargetResolution,
  ): Promise<InspectSession> {
    let process: ChildProcessWithoutNullStreams;
    try {
      process = this.spawn(
        this.binary.path,
        ["inspect", "--port", "0", "--api-only", "--format", "json"],
        {
          cwd: resolution.folder.uri.fsPath,
          env: sanitizedEnv(),
        },
      );
    } catch {
      throw new Error("Pointbreak could not start the local Review service.");
    }

    const exit = trackExit(process);
    try {
      const startup = await readStartup(
        process,
        this.startupTimeoutMs,
        MAX_STARTUP_BYTES,
      );
      const client = this.clientFactory(
        inspectOrigin(startup.host, startup.port),
        startup.token,
      );
      await client.verify(targetIdentity(resolution));
      const session = { targetKey: resolution.target.key, client };
      const runtime = { session, process, exit };
      await this.remember(
        {
          targetKey: resolution.target.key,
          host: startup.host,
          port: startup.port,
          storeIdentity: resolution.target.storeIdentity,
          contextIdentity: resolution.target.contextIdentity,
        },
        startup.token,
      );
      if (exit.exited()) {
        await this.forget(resolution.target.key);
        throw startupFailure("exit");
      }
      this.state = { kind: "running-owned", runtime };
      this.publishSession(resolution, session);
      void exit.promise.then(() => this.handleUnexpectedExit(runtime));
      return session;
    } catch (error) {
      await terminateProcess(process, exit, this.stopTimeoutMs);
      await this.forget(resolution.target.key);
      throw error;
    }
  }

  private async stopExclusive(): Promise<void> {
    if (
      this.state.kind !== "running-owned" &&
      this.state.kind !== "running-attached"
    ) {
      this.state = { kind: "idle" };
      this.clearSession();
      return;
    }

    if (this.state.kind === "running-attached") {
      const runtime = this.state.runtime;
      this.state = { kind: "stopping", runtime };
      this.clearSession();
      this.state = { kind: "idle" };
      return;
    }

    const runtime = this.state.runtime;
    this.state = { kind: "stopping", runtime };
    this.clearSession();
    await terminateProcess(runtime.process, runtime.exit, this.stopTimeoutMs);
    await this.forget(runtime.session.targetKey);
    this.state = { kind: "idle" };
  }

  private handleUnexpectedExit(runtime: OwnedRuntime): Promise<void> {
    return this.serialize(async () => {
      if (
        this.state.kind !== "running-owned" ||
        this.state.runtime !== runtime
      ) {
        return;
      }
      this.state = { kind: "idle" };
      this.clearSession();
      await this.forget(runtime.session.targetKey);
    });
  }

  private publishSession(
    resolution: ResolvedTargetResolution,
    session: InspectSession,
  ): void {
    this.statusBar.text = `$(debug-stop) ${resolution.folder.name}`;
    this.statusBar.tooltip = `Stop Pointbreak Review for ${resolution.folder.name}`;
    this.statusBar.show();
    this.sessionPublished = true;
    this.sessionEmitter.fire({ targetKey: session.targetKey });
  }

  private clearSession(): void {
    this.statusBar.hide();
    if (!this.sessionPublished) {
      return;
    }
    this.sessionPublished = false;
    this.sessionEmitter.fire(undefined);
  }

  private async remember(
    record: InspectConnectionRecord,
    token: string,
  ): Promise<void> {
    try {
      await this.store.remember(record, token);
    } catch {
      // A live authenticated child remains usable when optional reconnect state fails.
    }
  }

  private async forget(targetKey: string): Promise<void> {
    try {
      await this.store.forget(targetKey);
    } catch {
      // Stale reconnect state is rejected by the next authenticated handshake.
    }
  }

  private serialize<T>(operation: () => Promise<T>): Promise<T> {
    const next = this.operations.then(operation, operation);
    this.operations = next.then(
      () => undefined,
      () => undefined,
    );
    return next;
  }
}

export function parseInspectStartupLine(line: string): InspectStartupDocument {
  if (line.includes("\n") || line.includes("\r")) {
    throw invalidStartup();
  }
  let value: unknown;
  try {
    value = JSON.parse(line);
  } catch {
    throw invalidStartup();
  }
  if (
    !isObject(value) ||
    value.schema !== STARTUP_SCHEMA ||
    value.version !== 1 ||
    typeof value.host !== "string" ||
    !isLoopbackIp(value.host) ||
    typeof value.port !== "number" ||
    !Number.isInteger(value.port) ||
    value.port <= 0 ||
    value.port > 65_535 ||
    typeof value.token !== "string" ||
    !TOKEN_PATTERN.test(value.token)
  ) {
    throw invalidStartup();
  }
  return {
    schema: STARTUP_SCHEMA,
    version: 1,
    host: value.host,
    port: value.port,
    token: value.token,
  };
}

function readStartup(
  process: ChildProcessWithoutNullStreams,
  timeoutMs: number,
  maxBytes: number,
): Promise<InspectStartupDocument> {
  return new Promise((resolve, reject) => {
    let output = Buffer.alloc(0);
    let settled = false;
    let pendingResolve: NodeJS.Immediate | undefined;
    const timeout = setTimeout(
      () => settleReject(startupFailure("timeout")),
      timeoutMs,
    );

    const onData = (chunk: Buffer | string) => {
      const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
      output = Buffer.concat([output, bytes]);
      if (output.length > maxBytes) {
        settleReject(invalidStartup());
        return;
      }
      const newline = output.indexOf(0x0a);
      if (newline < 0) {
        return;
      }
      const line = output
        .subarray(0, newline)
        .toString("utf8")
        .replace(/\r$/, "");
      const remainder = output.subarray(newline + 1);
      if (remainder.length > 0) {
        settleReject(invalidStartup());
        return;
      }
      try {
        const document = parseInspectStartupLine(line);
        pendingResolve ??= setImmediate(() => settleResolve(document));
      } catch (error) {
        settleReject(error as Error);
      }
    };
    const onError = () => settleReject(startupFailure("start"));
    const onExit = () => settleReject(startupFailure("exit"));

    const cleanup = () => {
      clearTimeout(timeout);
      if (pendingResolve) {
        clearImmediate(pendingResolve);
      }
      process.stdout.removeListener("data", onData);
      process.removeListener("error", onError);
      process.removeListener("exit", onExit);
    };
    const settleResolve = (document: InspectStartupDocument) => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(document);
    };
    const settleReject = (error: Error) => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(error);
    };

    process.stdout.on("data", onData);
    process.once("error", onError);
    process.once("exit", onExit);
  });
}

function trackExit(process: ChildProcessWithoutNullStreams): ProcessExit {
  let didExit = process.exitCode !== null || process.signalCode !== null;
  const promise = didExit
    ? Promise.resolve()
    : new Promise<void>((resolve) => {
        process.once("exit", () => {
          didExit = true;
          resolve();
        });
      });
  return { promise, exited: () => didExit };
}

async function terminateProcess(
  process: ChildProcessWithoutNullStreams,
  exit: ProcessExit,
  timeoutMs: number,
): Promise<void> {
  if (exit.exited()) {
    return;
  }
  process.kill("SIGINT");
  if (await waitForExit(exit, timeoutMs)) {
    return;
  }
  process.kill("SIGKILL");
  await waitForExit(exit, timeoutMs);
}

function waitForExit(exit: ProcessExit, timeoutMs: number): Promise<boolean> {
  if (exit.exited()) {
    return Promise.resolve(true);
  }
  return new Promise((resolve) => {
    const timeout = setTimeout(() => resolve(false), timeoutMs);
    void exit.promise.then(() => {
      clearTimeout(timeout);
      resolve(true);
    });
  });
}

function runningSession(state: ManagerState): InspectSession | undefined {
  return state.kind === "running-owned" || state.kind === "running-attached"
    ? state.runtime.session
    : undefined;
}

function recordMatchesResolution(
  record: InspectConnectionRecord,
  resolution: ResolvedTargetResolution,
): boolean {
  return (
    record.targetKey === resolution.target.key &&
    record.storeIdentity === resolution.target.storeIdentity &&
    record.contextIdentity === resolution.target.contextIdentity
  );
}

function targetIdentity(resolution: ResolvedTargetResolution) {
  return {
    storeIdentity: resolution.target.storeIdentity,
    contextIdentity: resolution.target.contextIdentity,
  };
}

function inspectOrigin(host: string, port: number): string {
  return `http://${isIP(host) === 6 ? `[${host}]` : host}:${port}`;
}

function isLoopbackIp(host: string): boolean {
  const version = isIP(host);
  if (version === 4) {
    return host.split(".")[0] === "127";
  }
  if (version === 6) {
    const normalized = host.toLowerCase();
    return normalized === "::1" || normalized === "0:0:0:0:0:0:0:1";
  }
  return false;
}

function invalidStartup(): Error {
  return new Error(
    "Pointbreak inspect child returned an invalid startup document.",
  );
}

function startupFailure(kind: "timeout" | "start" | "exit"): Error {
  if (kind === "timeout") {
    return new Error("Pointbreak inspect child did not complete startup.");
  }
  if (kind === "exit") {
    return new Error(
      "Pointbreak inspect child exited before startup completed.",
    );
  }
  return new Error("Pointbreak could not start the local Review service.");
}

const defaultSpawn: SpawnFn = (file, args, opts) =>
  nodeSpawn(file, args, { ...opts, stdio: "pipe", windowsHide: true });

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
