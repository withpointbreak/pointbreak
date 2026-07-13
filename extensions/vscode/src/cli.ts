import { execFile } from "node:child_process";
import type { ResolvedBinary } from "./binary";

export type ExecFn = (
  file: string,
  args: string[],
  opts: { cwd: string; env: NodeJS.ProcessEnv },
) => Promise<{ stdout: string; stderr: string; exitCode: number }>;

export interface DiagnosticDocument {
  schema: string;
  version: number;
  diagnostics?: unknown[];
}

export interface VersionDoc extends DiagnosticDocument {
  schema: "pointbreak.version";
  version: 1;
  cliVersion: string;
  documents: Record<string, number>;
}

export interface AttentionItem {
  id: string;
  tier: string;
  kind: string;
  revisionId?: string;
  title?: string;
  [field: string]: unknown;
}

export interface AttentionListDoc extends DiagnosticDocument {
  schema: "pointbreak.attention-list";
  version: 1;
  items: AttentionItem[];
}

export interface RevisionListDoc extends DiagnosticDocument {
  schema: "pointbreak.review-revision-list";
  version: 1;
  entries: Array<{
    revisionId: string;
    capturedAt: string;
    mergeStatus: string;
    [field: string]: unknown;
  }>;
  revisionCount: number;
  eventCount: number;
  eventSetHash: string;
}

export interface StoreStatusDoc extends DiagnosticDocument {
  schema: "pointbreak.store-status";
  version: 1;
  storeIdentity: string;
  contextIdentity: string;
  repositoryFamilyRef?: string;
  storeRef?: string;
  inventory: {
    eventCount: number;
    artifactCount: number;
    revisionObjects: unknown[];
    [field: string]: unknown;
  };
}

export interface CaptureDoc extends DiagnosticDocument {
  schema: "pointbreak.review-capture";
  version: 1;
  revision: { id: string };
}

export type CaptureChoice = "worktree" | "staged" | "unstaged";

export interface CaptureOptions {
  choice: CaptureChoice;
  includeUntracked: boolean;
  allowEmpty: boolean;
}

export const REQUIRED_DOCUMENTS: Record<string, number> = {
  "pointbreak.version": 1,
  "pointbreak.attention-list": 1,
  "pointbreak.review-revision-list": 1,
  "pointbreak.review-revision": 2,
  "pointbreak.review-capture": 1,
  "pointbreak.review-observation-add": 1,
  "pointbreak.review-snapshot": 1,
  "pointbreak.inspect-freshness": 1,
  "pointbreak.inspect-startup": 1,
  "pointbreak.store-status": 1,
};

// This extension targets the CLI minor that first provides the version handshake.
export const COMPATIBLE_CLI_RANGE = "0.6";

export type HandshakeResult =
  | { ok: true; cliVersion: string }
  | { ok: false; reason: string };

export class PointbreakCliError extends Error {
  constructor(
    message: string,
    readonly exitCode?: number,
    readonly stderr = "",
  ) {
    super(message);
    this.name = "PointbreakCliError";
  }
}

export function sanitizedEnv(
  base: NodeJS.ProcessEnv = process.env,
): NodeJS.ProcessEnv {
  const env = { ...base };
  delete env.SHORE_ACTOR_ID;
  delete env.SHORE_FORMAT;
  return env;
}

export function verifyHandshake(doc: VersionDoc): HandshakeResult {
  if (doc.schema !== "pointbreak.version" || doc.version !== 1) {
    return {
      ok: false,
      reason: "the CLI does not provide pointbreak.version version 1",
    };
  }

  if (
    typeof doc.cliVersion !== "string" ||
    typeof doc.documents !== "object" ||
    doc.documents === null
  ) {
    return {
      ok: false,
      reason: "the CLI returned an invalid pointbreak.version document",
    };
  }

  const minor = doc.cliVersion
    .match(/^(\d+)\.(\d+)(?:\.|$)/)
    ?.slice(1, 3)
    .join(".");
  if (minor !== COMPATIBLE_CLI_RANGE) {
    return {
      ok: false,
      reason: `CLI ${doc.cliVersion} is outside compatible minor ${COMPATIBLE_CLI_RANGE}`,
    };
  }

  for (const [schema, version] of Object.entries(REQUIRED_DOCUMENTS)) {
    if (doc.documents[schema] !== version) {
      return {
        ok: false,
        reason: `${schema} version ${String(doc.documents[schema] ?? "missing")} is incompatible; expected ${version}`,
      };
    }
  }

  return { ok: true, cliVersion: doc.cliVersion };
}

export class PointbreakCli {
  private handshake?: Promise<void>;

  constructor(
    private readonly binary: ResolvedBinary,
    private readonly exec: ExecFn = defaultExec,
  ) {}

  async version(repo: string): Promise<VersionDoc> {
    const doc = await this.readVersion(repo);
    this.handshake = Promise.resolve();
    return doc;
  }

  async attentionList(repo: string): Promise<AttentionListDoc> {
    return this.runDocument(
      repo,
      ["attention", "list"],
      "pointbreak.attention-list",
    );
  }

  async revisionList(repo: string): Promise<RevisionListDoc> {
    return this.runDocument(
      repo,
      ["revision", "list"],
      "pointbreak.review-revision-list",
    );
  }

  async storeStatus(repo: string): Promise<StoreStatusDoc> {
    return this.runDocument(
      repo,
      ["store", "status"],
      "pointbreak.store-status",
    );
  }

  async capture(repo: string, opts: CaptureOptions): Promise<CaptureDoc> {
    return this.runDocument(
      repo,
      captureArgs(opts),
      "pointbreak.review-capture",
    );
  }

  private async ensureHandshake(repo: string): Promise<void> {
    this.handshake ??= this.readVersion(repo).then(() => undefined);
    return this.handshake;
  }

  private async readVersion(repo: string): Promise<VersionDoc> {
    let doc: VersionDoc;
    try {
      doc = await this.executeDocument(repo, ["version"], "pointbreak.version");
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      throw new PointbreakCliError(
        `Pointbreak CLI is too old or incompatible: shore version failed (${detail})`,
        error instanceof PointbreakCliError ? error.exitCode : undefined,
        error instanceof PointbreakCliError ? error.stderr : "",
      );
    }

    const result = verifyHandshake(doc);
    if (!result.ok) {
      throw new PointbreakCliError(
        `${result.reason}. Update the extension or the Pointbreak CLI.`,
      );
    }
    return doc;
  }

  private async runDocument<T extends DiagnosticDocument>(
    repo: string,
    args: string[],
    schema: string,
  ): Promise<T> {
    await this.ensureHandshake(repo);
    return this.executeDocument(repo, args, schema);
  }

  private async executeDocument<T extends DiagnosticDocument>(
    repo: string,
    args: string[],
    schema: string,
  ): Promise<T> {
    const result = await this.exec(this.binary.path, args, {
      cwd: repo,
      env: sanitizedEnv(),
    });
    if (result.exitCode !== 0) {
      const detail = result.stderr.trim() || "no error output";
      throw new PointbreakCliError(
        `shore ${args.join(" ")} failed: ${detail}`,
        result.exitCode,
        result.stderr,
      );
    }

    let parsed: unknown;
    try {
      parsed = JSON.parse(result.stdout);
    } catch {
      throw new PointbreakCliError(
        `shore ${args.join(" ")} returned invalid JSON`,
      );
    }
    if (!isDocument(parsed)) {
      throw new PointbreakCliError(
        `shore ${args.join(" ")} returned an invalid document`,
      );
    }

    const expectedVersion = REQUIRED_DOCUMENTS[schema];
    if (parsed.schema !== schema || parsed.version !== expectedVersion) {
      throw new PointbreakCliError(
        `Expected ${schema} version ${expectedVersion}, received ${parsed.schema} version ${parsed.version}`,
      );
    }
    return parsed as T;
  }
}

export function captureArgs(opts: CaptureOptions): string[] {
  if (opts.choice === "staged" && opts.includeUntracked) {
    throw new Error("Staged capture cannot include untracked files.");
  }

  const args = ["capture"];
  if (opts.choice === "staged") {
    args.push("--staged");
  } else if (opts.choice === "unstaged") {
    args.push("--unstaged");
  }
  if (opts.includeUntracked) {
    args.push("--include-untracked");
  }
  if (opts.allowEmpty) {
    args.push("--allow-empty");
  }
  return args;
}

const defaultExec: ExecFn = (file, args, opts) =>
  new Promise((resolve) => {
    execFile(
      file,
      args,
      {
        cwd: opts.cwd,
        env: opts.env,
        encoding: "utf8",
        maxBuffer: 1024 * 1024,
        timeout: 30_000,
        windowsHide: true,
      },
      (error, stdout, stderr) => {
        resolve({
          stdout,
          stderr,
          exitCode: commandExitCode(error),
        });
      },
    );
  });

function isDocument(value: unknown): value is DiagnosticDocument {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.schema === "string" &&
    typeof candidate.version === "number"
  );
}

function commandExitCode(error: { code?: string | number } | null): number {
  if (!error) {
    return 0;
  }
  return typeof error.code === "number" ? error.code : 1;
}
