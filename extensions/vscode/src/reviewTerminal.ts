import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { isIP } from "node:net";
import {
  EventEmitter,
  type Pseudoterminal,
  type Terminal,
  type WorkspaceFolder,
  window,
} from "vscode";
import type { ResolvedBinary } from "./binary";
import { sanitizedEnv } from "./cli";

const REVIEW_TERMINAL_NAME = "Pointbreak Review";
const URL_ANNOUNCEMENT_TIMEOUT_MS = 10_000;
const FORCE_KILL_TIMEOUT_MS = 2_000;
const MAX_PARSE_BUFFER = 4_096;
const TOKEN_PATTERN = /^[A-Za-z0-9_-]{43,}$/;
// Inspector output may acquire ANSI styling when run through a terminal.
// biome-ignore lint/suspicious/noControlCharactersInRegex: ESC is intentional here.
const ANSI_ESCAPE = /\u001b\[[0-?]*[ -/]*[@-~]/g;

export interface ReviewCapability {
  readonly origin: string;
  readonly token: string;
}

interface RunningReview {
  terminal: Terminal;
  capability: Promise<ReviewCapability>;
}

const runningReviews = new Map<string, RunningReview>();

export function runningReviewCapability(
  targetKey: string,
): Promise<ReviewCapability> | undefined {
  return runningReviews.get(targetKey)?.capability;
}

export function stopReviewTerminal(targetKey: string): void {
  const running = runningReviews.get(targetKey);
  if (!running) {
    return;
  }
  runningReviews.delete(targetKey);
  void running.capability.catch(() => undefined);
  running.terminal.dispose();
}

export function inspectInvocation(binary: ResolvedBinary): {
  file: string;
  args: string[];
} {
  return { file: binary.path, args: ["inspect", "--port", "0"] };
}

/** Incrementally extracts a loopback origin and bearer from inspect text output. */
export class ReviewUrlParser {
  private output = "";

  push(chunk: string): ReviewCapability | undefined {
    this.output = `${this.output}${chunk}`.slice(-MAX_PARSE_BUFFER);
    for (const line of this.output.replace(ANSI_ESCAPE, "").split(/\r?\n/)) {
      const candidate = line.match(/^\s*url:\s+(\S+)\s*$/)?.[1];
      if (!candidate) {
        continue;
      }
      const capability = parseReviewCapability(candidate);
      if (capability) {
        return capability;
      }
    }
    return undefined;
  }
}

/** Start or reveal the explicit browser Review process for one target. */
export function startReviewTerminal(
  binary: ResolvedBinary,
  folder: WorkspaceFolder,
  targetKey: string,
): Promise<ReviewCapability> {
  const existing = runningReviews.get(targetKey);
  if (existing) {
    existing.terminal.show();
    return existing.capability;
  }

  let terminal: Terminal;
  const pty = new InspectPseudoterminal(binary, folder, () => {
    if (runningReviews.get(targetKey)?.terminal === terminal) {
      runningReviews.delete(targetKey);
    }
  });
  terminal = window.createTerminal({ name: REVIEW_TERMINAL_NAME, pty });
  const running = { terminal, capability: pty.capability };
  runningReviews.set(targetKey, running);
  void pty.capability.catch(() => {
    if (runningReviews.get(targetKey) === running) {
      runningReviews.delete(targetKey);
      terminal.dispose();
    }
  });
  terminal.show();
  return pty.capability;
}

class InspectPseudoterminal implements Pseudoterminal {
  private readonly writeEmitter = new EventEmitter<string>();
  readonly onDidWrite = this.writeEmitter.event;
  private readonly closeEmitter = new EventEmitter<number | undefined>();
  readonly onDidClose = this.closeEmitter.event;
  private readonly parser = new ReviewUrlParser();
  private readonly resolveCapability: (capability: ReviewCapability) => void;
  private readonly rejectCapability: (error: Error) => void;
  private process: ChildProcessWithoutNullStreams | undefined;
  private timeout: NodeJS.Timeout | undefined;
  private forceKillTimeout: NodeJS.Timeout | undefined;
  private capabilitySettled = false;
  private finished = false;
  readonly capability: Promise<ReviewCapability>;

  constructor(
    private readonly binary: ResolvedBinary,
    private readonly folder: WorkspaceFolder,
    private readonly onClose: () => void,
  ) {
    let resolveCapability!: (capability: ReviewCapability) => void;
    let rejectCapability!: (error: Error) => void;
    this.capability = new Promise<ReviewCapability>((resolve, reject) => {
      resolveCapability = resolve;
      rejectCapability = reject;
    });
    this.resolveCapability = resolveCapability;
    this.rejectCapability = rejectCapability;
  }

  open(): void {
    const invocation = inspectInvocation(this.binary);
    this.process = spawn(invocation.file, invocation.args, {
      cwd: this.folder.uri.fsPath,
      env: sanitizedEnv(),
      stdio: "pipe",
      windowsHide: true,
    });
    this.startTimeout();

    this.process.stdout.on("data", (chunk: Buffer) => {
      this.acceptOutput(chunk.toString());
    });
    this.process.stderr.resume();
    this.process.once("error", () => {
      this.writeEmitter.fire("\r\nPointbreak could not start Review.\r\n");
      this.rejectPending(
        new Error("Pointbreak could not start the local Review service."),
      );
      this.finish(1);
    });
    this.process.once("exit", (code) => {
      this.rejectPending(
        new Error(
          "Pointbreak Review stopped before its browser capability was ready.",
        ),
      );
      this.finish(code ?? 0);
    });
  }

  close(): void {
    this.rejectPending(
      new Error(
        "Pointbreak Review stopped before its browser capability was ready.",
      ),
    );
    this.terminate();
    this.cleanup();
  }

  handleInput(data: string): void {
    if (data === "\u0003") {
      this.process?.kill("SIGINT");
      return;
    }
    this.process?.stdin.write(data);
  }

  private acceptOutput(output: string): void {
    // The text startup surface intentionally displays its one-time capability.
    this.writeEmitter.fire(output.replace(/\r?\n/g, "\r\n"));
    const capability = this.parser.push(output);
    if (capability && !this.capabilitySettled) {
      this.capabilitySettled = true;
      this.clearTimeout();
      this.resolveCapability(capability);
    }
  }

  private startTimeout(): void {
    this.clearTimeout();
    this.timeout = setTimeout(() => {
      this.rejectPending(
        new Error(
          "Pointbreak Review did not provide a browser capability in time.",
        ),
      );
      this.terminate();
    }, URL_ANNOUNCEMENT_TIMEOUT_MS);
  }

  private rejectPending(error: Error): void {
    if (this.capabilitySettled) {
      return;
    }
    this.capabilitySettled = true;
    this.clearTimeout();
    this.rejectCapability(error);
  }

  private finish(code: number): void {
    if (this.finished) {
      return;
    }
    this.finished = true;
    this.clearForceKillTimeout();
    this.cleanup();
    this.closeEmitter.fire(code);
  }

  private terminate(): void {
    const process = this.process;
    if (!process || process.exitCode !== null || process.signalCode !== null) {
      return;
    }
    process.kill("SIGINT");
    this.clearForceKillTimeout();
    this.forceKillTimeout = setTimeout(() => {
      if (process.exitCode === null && process.signalCode === null) {
        process.kill("SIGKILL");
      }
    }, FORCE_KILL_TIMEOUT_MS);
    this.forceKillTimeout.unref();
  }

  private cleanup(): void {
    this.clearTimeout();
    this.onClose();
  }

  private clearTimeout(): void {
    if (this.timeout) {
      clearTimeout(this.timeout);
      this.timeout = undefined;
    }
  }

  private clearForceKillTimeout(): void {
    if (this.forceKillTimeout) {
      clearTimeout(this.forceKillTimeout);
      this.forceKillTimeout = undefined;
    }
  }
}

function parseReviewCapability(value: string): ReviewCapability | undefined {
  try {
    const url = new URL(value);
    const host = url.hostname.replace(/^\[|\]$/g, "");
    if (
      url.protocol !== "http:" ||
      !isLoopbackIp(host) ||
      url.username ||
      url.password ||
      url.search
    ) {
      return undefined;
    }
    const queryStart = url.hash.indexOf("?");
    if (queryStart < 0) {
      return undefined;
    }
    const params = new URLSearchParams(url.hash.slice(queryStart + 1));
    const tokens = params.getAll("token");
    if (tokens.length !== 1 || !TOKEN_PATTERN.test(tokens[0] ?? "")) {
      return undefined;
    }
    return { origin: url.origin, token: tokens[0] };
  } catch {
    return undefined;
  }
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
