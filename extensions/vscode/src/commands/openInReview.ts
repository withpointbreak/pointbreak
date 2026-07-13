import {
  commands,
  env,
  Uri,
  type WorkspaceFolder,
  window,
  workspace,
} from "vscode";
import type { ResolvedBinary } from "../binary";
import type { PointbreakCli } from "../cli";
import {
  type ReviewServerRecord,
  type ReviewServerRegistry,
  reviewServerUrl,
} from "../reviewServerRegistry";
import { runningReviewUrl, startReviewTerminal } from "../reviewTerminal";
import { pickFolder, type TargetResolution } from "../targetResolver";

const RETRY_ATTEMPTS = 10;
const RETRY_DELAY_MS = 1_000;
const START_REVIEW_ACTION = "Start `shore inspect` here";
const OPEN_INTEGRATED_BROWSER_COMMAND = "workbench.action.browser.open";
const LOCAL_BROWSER_HOSTS = new Set([
  "localhost",
  "127.0.0.1",
  "[::1]",
  "0.0.0.0",
  "[::]",
]);

export interface ReviewNode {
  revisionId: string;
  targetKey: string;
  folder: WorkspaceFolder;
  lens?: "attention";
}

export interface ReviewIdentity {
  storeIdentity: string;
  contextIdentity: string;
}

export type ReviewProbeResult =
  | { kind: "match" }
  | { kind: "unavailable" }
  | { kind: "incompatible" }
  | { kind: "mismatch" };

type ReviewProbe = (
  baseUrl: string,
  identity: ReviewIdentity,
) => Promise<ReviewProbeResult>;

interface OpenInReviewDependencies {
  pick?: typeof pickFolder;
  probe?: ReviewProbe;
  registry?: ReviewServerRegistry;
  reviewUrl?: string;
  running?: typeof runningReviewUrl;
  sleep?: (milliseconds: number) => Promise<void>;
  start?: typeof startReviewTerminal;
}

interface RestoreReviewServersDependencies {
  onError?: (message: string) => void;
  probe?: ReviewProbe;
  sleep?: (milliseconds: number) => Promise<void>;
  start?: typeof startReviewTerminal;
}

export function reviewDeepLink(
  baseUrl: string,
  revisionId: string,
  lens?: "attention",
): string {
  const query = lens ? `?lens=${lens}` : "";
  return `${trimTrailingSlash(baseUrl)}/#/revision/${revisionId}${query}`;
}

export async function runOpenInReviewCommand(
  cli: PointbreakCli,
  binary: ResolvedBinary,
  resolutions: TargetResolution[],
  node?: ReviewNode,
  dependencies: OpenInReviewDependencies = {},
): Promise<void> {
  if (env.remoteName) {
    await window.showInformationMessage(
      "Open in Pointbreak Review is not available in remote workspaces yet.",
    );
    return;
  }

  const selection =
    node ??
    (await pickRevision(cli, resolutions, dependencies.pick ?? pickFolder));
  if (!selection) {
    return;
  }

  const resolution = await selectedResolution(selection, resolutions);
  if (!resolution) {
    return;
  }
  const identity = identityFor(resolution);

  const configuredUrl = normalizeOptionalUrl(
    dependencies.reviewUrl ?? configuredReviewUrl(selection.folder),
  );
  const probe = dependencies.probe ?? probeReview;
  if (configuredUrl) {
    const configuredResult = await probe(configuredUrl, identity);
    if (configuredResult.kind === "match") {
      await openRevision(configuredUrl, selection);
    } else {
      await window.showErrorMessage(
        configuredServerFailureMessage(configuredUrl, configuredResult),
      );
    }
    return;
  }

  const runningUrl = await knownRunningUrl(
    (dependencies.running ?? runningReviewUrl)(identityKey(identity)),
  );
  if (runningUrl && (await probe(runningUrl, identity)).kind === "match") {
    await rememberReviewServer(dependencies.registry, resolution, runningUrl);
    await openRevision(runningUrl, selection);
    return;
  }

  let preferredPort = 0;
  const remembered = dependencies.registry?.get(selection.targetKey);
  if (remembered) {
    const rememberedUrl = reviewServerUrl(remembered);
    const rememberedResult = await probe(rememberedUrl, identity);
    if (rememberedResult.kind === "match") {
      await openRevision(rememberedUrl, selection);
      return;
    }
    if (rememberedResult.kind === "unavailable") {
      preferredPort = remembered.port;
    }
  }

  const action = await window.showInformationMessage(
    "Pointbreak Review is not running for this repository.",
    START_REVIEW_ACTION,
  );
  if (action !== START_REVIEW_ACTION) {
    return;
  }

  let startedUrl: string;
  try {
    startedUrl = trimTrailingSlash(
      await (dependencies.start ?? startReviewTerminal)(
        binary,
        selection.folder,
        identityKey(identity),
        { port: preferredPort },
      ),
    );
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    await window.showErrorMessage(
      `Pointbreak could not start Review: ${detail}`,
    );
    return;
  }

  const result = await retryProbe(
    startedUrl,
    identity,
    probe,
    dependencies.sleep ?? delay,
  );
  if (result.kind === "match") {
    await rememberReviewServer(dependencies.registry, resolution, startedUrl);
    await openRevision(startedUrl, selection);
    return;
  }

  await window.showErrorMessage(startedServerFailureMessage(result));
}

async function openRevision(
  baseUrl: string,
  selection: ReviewNode,
): Promise<void> {
  await openReviewUrl(
    reviewDeepLink(baseUrl, selection.revisionId, selection.lens),
  );
}

export async function restoreReviewServers(
  binary: ResolvedBinary,
  resolutions: TargetResolution[],
  registry: ReviewServerRegistry,
  dependencies: RestoreReviewServersDependencies = {},
): Promise<void> {
  if (env.remoteName) {
    return;
  }
  const probe = dependencies.probe ?? probeReview;
  const start = dependencies.start ?? startReviewTerminal;
  const sleep = dependencies.sleep ?? delay;
  await Promise.all(
    registry.entries().map(async (record) => {
      const resolution = restoredResolution(record, resolutions);
      if (
        !resolution ||
        normalizeOptionalUrl(configuredReviewUrl(resolution.folder))
      ) {
        return;
      }

      const identity = identityFor(resolution);
      const rememberedUrl = reviewServerUrl(record);
      const rememberedResult = await probe(rememberedUrl, identity);
      if (rememberedResult.kind === "match") {
        return;
      }

      const preferredPort =
        rememberedResult.kind === "unavailable" ? record.port : 0;
      let startedUrl: string;
      try {
        startedUrl = trimTrailingSlash(
          await start(binary, resolution.folder, record.targetKey, {
            port: preferredPort,
            reveal: false,
          }),
        );
      } catch (error) {
        dependencies.onError?.(
          restoreFailureMessage(resolution.folder.name, error),
        );
        return;
      }

      const result = await retryProbe(startedUrl, identity, probe, sleep);
      if (result.kind === "match") {
        await rememberReviewServer(registry, resolution, startedUrl);
        return;
      }
      dependencies.onError?.(
        `Pointbreak could not restore Review for ${resolution.folder.name}: ${startedServerFailureMessage(result)}`,
      );
    }),
  );
}

async function openReviewUrl(url: string): Promise<void> {
  const openLocalhostLinks = workspace
    .getConfiguration("workbench.browser")
    .get<boolean>("openLocalhostLinks", false);
  if (openLocalhostLinks && isLocalBrowserUrl(url)) {
    try {
      // The integrated browser has no dedicated extension API. Current VS Code
      // exposes this command; older supported hosts fall back to openExternal.
      await commands.executeCommand(OPEN_INTEGRATED_BROWSER_COMMAND, url);
      return;
    } catch {
      // Fall through for VS Code versions without the integrated browser.
    }
  }
  await env.openExternal(Uri.parse(url));
}

function isLocalBrowserUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return (
      (url.protocol === "http:" || url.protocol === "https:") &&
      LOCAL_BROWSER_HOSTS.has(url.hostname)
    );
  } catch {
    return false;
  }
}

async function knownRunningUrl(
  pending: Promise<string> | undefined,
): Promise<string | undefined> {
  try {
    return pending ? trimTrailingSlash(await pending) : undefined;
  } catch {
    return undefined;
  }
}

export async function probeReview(
  baseUrl: string,
  identity: ReviewIdentity,
): Promise<ReviewProbeResult> {
  let response: Response;
  try {
    response = await fetch(`${trimTrailingSlash(baseUrl)}/api/identity`, {
      method: "GET",
      signal: AbortSignal.timeout(1_000),
    });
  } catch {
    return { kind: "unavailable" };
  }
  if (!response.ok) {
    return { kind: "unavailable" };
  }

  let document: Partial<ReviewIdentity & { schema: string }>;
  try {
    document = (await response.json()) as Partial<
      ReviewIdentity & { schema: string }
    >;
  } catch {
    return { kind: "incompatible" };
  }
  if (
    document.schema !== "pointbreak.inspect-identity" ||
    typeof document.storeIdentity !== "string" ||
    typeof document.contextIdentity !== "string"
  ) {
    return { kind: "incompatible" };
  }
  if (
    document.storeIdentity !== identity.storeIdentity ||
    document.contextIdentity !== identity.contextIdentity
  ) {
    return { kind: "mismatch" };
  }
  return { kind: "match" };
}

type ResolvedTarget = TargetResolution & { kind: "resolved" };

async function selectedResolution(
  selection: ReviewNode,
  resolutions: TargetResolution[],
): Promise<ResolvedTarget | undefined> {
  const resolution = resolutions.find(
    (candidate) =>
      candidate.kind === "resolved" &&
      candidate.target.key === selection.targetKey,
  );
  if (resolution?.kind === "resolved") {
    return resolution;
  }
  await window.showErrorMessage(
    "Pointbreak could not identify this review target. Refresh the extension and try again.",
  );
  return undefined;
}

function identityFor(resolution: ResolvedTarget): ReviewIdentity {
  return {
    storeIdentity: resolution.target.storeIdentity,
    contextIdentity: resolution.target.contextIdentity,
  };
}

function restoredResolution(
  record: ReviewServerRecord,
  resolutions: TargetResolution[],
): ResolvedTarget | undefined {
  const matches = resolutions.filter(
    (resolution): resolution is ResolvedTarget =>
      resolution.kind === "resolved" &&
      resolution.target.key === record.targetKey &&
      resolution.target.storeIdentity === record.storeIdentity &&
      resolution.target.contextIdentity === record.contextIdentity,
  );
  return (
    matches.find(
      (resolution) => resolution.folder.uri.toString() === record.folderUri,
    ) ?? matches[0]
  );
}

async function rememberReviewServer(
  registry: ReviewServerRegistry | undefined,
  resolution: ResolvedTarget,
  baseUrl: string,
): Promise<void> {
  const port = reviewServerPort(baseUrl);
  if (!registry || !port) {
    return;
  }
  try {
    await registry.remember({
      targetKey: resolution.target.key,
      storeIdentity: resolution.target.storeIdentity,
      contextIdentity: resolution.target.contextIdentity,
      folderUri: resolution.folder.uri.toString(),
      port,
    });
  } catch {
    // Persistence is a convenience; a healthy server should still open.
  }
}

function reviewServerPort(baseUrl: string): number | undefined {
  try {
    const port = Number(new URL(baseUrl).port);
    return Number.isInteger(port) && port > 0 && port <= 65_535
      ? port
      : undefined;
  } catch {
    return undefined;
  }
}

function configuredReviewUrl(folder: WorkspaceFolder): string {
  return workspace
    .getConfiguration("pointbreak", folder.uri)
    .get<string>("reviewUrl", "");
}

function normalizeOptionalUrl(value: string): string {
  const trimmed = value.trim();
  return trimmed ? trimTrailingSlash(trimmed) : "";
}

function identityKey(identity: ReviewIdentity): string {
  return `${identity.storeIdentity}/${identity.contextIdentity}`;
}

async function pickRevision(
  cli: PointbreakCli,
  resolutions: TargetResolution[],
  pick: typeof pickFolder,
): Promise<ReviewNode | undefined> {
  const resolution = await pick(resolutions);
  if (!resolution) {
    return undefined;
  }
  try {
    const revisions = await cli.revisionList(resolution.folder.uri.fsPath);
    const items = revisions.entries.slice(0, 20).map((entry) => ({
      label: shortRevisionId(entry.revisionId),
      description: entry.mergeStatus,
      detail: entry.capturedAt,
      revisionId: entry.revisionId,
    }));
    if (items.length === 0) {
      await window.showInformationMessage(
        "Pointbreak has no captured revisions in this target yet.",
      );
      return undefined;
    }
    const picked = await window.showQuickPick(items, {
      placeHolder: "Choose a revision to open in Pointbreak Review",
    });
    return picked
      ? {
          revisionId: picked.revisionId,
          targetKey: resolution.target.key,
          folder: resolution.folder,
        }
      : undefined;
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    await window.showErrorMessage(
      `Pointbreak could not list revisions: ${detail}`,
    );
    return undefined;
  }
}

async function retryProbe(
  baseUrl: string,
  identity: ReviewIdentity,
  probe: ReviewProbe,
  sleep: (milliseconds: number) => Promise<void>,
): Promise<ReviewProbeResult> {
  for (let attempt = 0; attempt < RETRY_ATTEMPTS; attempt += 1) {
    await sleep(RETRY_DELAY_MS);
    const result = await probe(baseUrl, identity);
    if (result.kind !== "unavailable") {
      return result;
    }
  }
  return { kind: "unavailable" };
}

function startedServerFailureMessage(result: ReviewProbeResult): string {
  if (result.kind === "incompatible") {
    return "The Pointbreak Review terminal started, but the shore CLI it launched is incompatible with this extension. Rebuild or update the extension bundle so its JavaScript and CLI versions match.";
  }
  if (result.kind === "mismatch") {
    return "The Pointbreak Review terminal started, but its server belongs to a different repository.";
  }
  return "The Pointbreak Review terminal started, but its server did not become available.";
}

function configuredServerFailureMessage(
  baseUrl: string,
  result: ReviewProbeResult,
): string {
  if (result.kind === "incompatible") {
    return `Configured Pointbreak Review at ${baseUrl} is incompatible with this extension.`;
  }
  if (result.kind === "mismatch") {
    return `Configured Pointbreak Review at ${baseUrl} serves a different repository.`;
  }
  return `Configured Pointbreak Review at ${baseUrl} is unavailable.`;
}

function restoreFailureMessage(folderName: string, error: unknown): string {
  const detail = error instanceof Error ? error.message : String(error);
  return `Pointbreak could not restore Review for ${folderName}: ${detail}`;
}

function trimTrailingSlash(value: string): string {
  return value.replace(/\/+$/, "");
}

function shortRevisionId(revisionId: string): string {
  return revisionId.split(":").at(-1)?.slice(0, 12) ?? revisionId;
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
