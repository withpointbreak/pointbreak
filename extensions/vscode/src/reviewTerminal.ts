import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
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
const MAX_PARSE_BUFFER = 4_096;
// Inspector output may acquire ANSI styling when run through a terminal.
// biome-ignore lint/suspicious/noControlCharactersInRegex: ESC is intentional here.
const ANSI_ESCAPE = /\u001b\[[0-?]*[ -/]*[@-~]/g;

interface RunningReview {
  terminal: Terminal;
  url: Promise<string>;
}

export interface StartReviewTerminalOptions {
  port?: number;
  reveal?: boolean;
}

const runningReviews = new Map<string, RunningReview>();

export function runningReviewUrl(
  targetKey: string,
): Promise<string> | undefined {
  return runningReviews.get(targetKey)?.url;
}

export function inspectInvocation(
  binary: ResolvedBinary,
  port = 0,
): {
  file: string;
  args: string[];
} {
  return { file: binary.path, args: ["inspect", "--port", String(port)] };
}

/** Incrementally extracts the loopback URL announced by `shore inspect`. */
export class ReviewUrlParser {
  private output = "";

  push(chunk: string): string | undefined {
    this.output = `${this.output}${chunk}`.slice(-MAX_PARSE_BUFFER);
    for (const line of this.output.replace(ANSI_ESCAPE, "").split(/\r?\n/)) {
      const candidate = line.match(/^\s*url:\s+(\S+)\s*$/)?.[1];
      if (!candidate) {
        continue;
      }
      try {
        const url = new URL(candidate);
        if (
          url.protocol === "http:" &&
          ["127.0.0.1", "localhost", "[::1]"].includes(url.hostname)
        ) {
          return trimTrailingSlash(url.toString());
        }
      } catch {
        // Ignore malformed output and keep waiting for a valid announcement.
      }
    }
    return undefined;
  }
}

/**
 * Start (or reveal) one inspector terminal for an exact store/context pair and
 * resolve with the ephemeral URL printed by the CLI.
 */
export function startReviewTerminal(
  binary: ResolvedBinary,
  folder: WorkspaceFolder,
  targetKey: string,
  options: StartReviewTerminalOptions = {},
): Promise<string> {
  const reveal = options.reveal ?? true;
  const existing = runningReviews.get(targetKey);
  if (existing) {
    if (reveal) {
      existing.terminal.show();
    }
    return existing.url;
  }

  let terminal: Terminal;
  const pty = new InspectPseudoterminal(
    binary,
    folder,
    options.port ?? 0,
    () => {
      if (runningReviews.get(targetKey)?.terminal === terminal) {
        runningReviews.delete(targetKey);
      }
    },
  );
  terminal = window.createTerminal({ name: REVIEW_TERMINAL_NAME, pty });
  const review = { terminal, url: pty.url };
  runningReviews.set(targetKey, review);
  if (reveal) {
    terminal.show();
  }
  return review.url;
}

class InspectPseudoterminal implements Pseudoterminal {
  private readonly writeEmitter = new EventEmitter<string>();
  readonly onDidWrite = this.writeEmitter.event;
  private readonly closeEmitter = new EventEmitter<number | undefined>();
  readonly onDidClose = this.closeEmitter.event;
  private parser = new ReviewUrlParser();
  private readonly resolveUrl: (url: string) => void;
  private readonly rejectUrl: (error: Error) => void;
  private process: ChildProcessWithoutNullStreams | undefined;
  private timeout: NodeJS.Timeout | undefined;
  private urlSettled = false;
  readonly url: Promise<string>;

  constructor(
    private readonly binary: ResolvedBinary,
    private readonly folder: WorkspaceFolder,
    private readonly preferredPort: number,
    private readonly onClose: () => void,
  ) {
    let resolveUrl!: (url: string) => void;
    let rejectUrl!: (error: Error) => void;
    this.url = new Promise<string>((resolve, reject) => {
      resolveUrl = resolve;
      rejectUrl = reject;
    });
    this.resolveUrl = resolveUrl;
    this.rejectUrl = rejectUrl;
  }

  open(): void {
    this.spawn(this.preferredPort);
  }

  private spawn(port: number): void {
    const invocation = inspectInvocation(this.binary, port);
    this.process = spawn(invocation.file, invocation.args, {
      cwd: this.folder.uri.fsPath,
      env: sanitizedEnv(),
      stdio: "pipe",
    });
    this.startTimeout();

    this.process.stdout.on("data", (chunk: Buffer) => {
      this.acceptOutput(chunk.toString());
    });
    this.process.stderr.on("data", (chunk: Buffer) => {
      this.acceptOutput(chunk.toString());
    });
    this.process.once("error", (error) => {
      this.writeEmitter.fire(
        `\r\nPointbreak could not start: ${error.message}\r\n`,
      );
      this.rejectPending(error);
      this.finish(1);
    });
    this.process.once("exit", (code) => {
      if (!this.urlSettled && port !== 0) {
        this.clearTimeout();
        this.parser = new ReviewUrlParser();
        this.writeEmitter.fire(
          `\r\nPort ${port} was unavailable; retrying on an ephemeral port.\r\n`,
        );
        this.spawn(0);
        return;
      }
      this.rejectPending(
        new Error(
          `shore inspect exited before announcing a Review URL (exit ${String(code ?? "unknown")})`,
        ),
      );
      this.finish(code ?? 0);
    });
  }

  private startTimeout(): void {
    this.clearTimeout();
    this.timeout = setTimeout(() => {
      this.rejectPending(
        new Error(
          "shore inspect did not announce a Review URL within 10 seconds",
        ),
      );
      this.process?.kill("SIGINT");
    }, URL_ANNOUNCEMENT_TIMEOUT_MS);
  }

  close(): void {
    this.rejectPending(
      new Error("Pointbreak Review terminal closed before announcing a URL"),
    );
    this.process?.kill("SIGINT");
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
    this.writeEmitter.fire(output.replace(/\r?\n/g, "\r\n"));
    const url = this.parser.push(output);
    if (url && !this.urlSettled) {
      this.urlSettled = true;
      this.clearTimeout();
      this.resolveUrl(url);
    }
  }

  private rejectPending(error: Error): void {
    if (this.urlSettled) {
      return;
    }
    this.urlSettled = true;
    this.clearTimeout();
    this.rejectUrl(error);
  }

  private finish(code: number): void {
    this.cleanup();
    this.closeEmitter.fire(code);
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
}

function trimTrailingSlash(value: string): string {
  return value.replace(/\/+$/, "");
}
