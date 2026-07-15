import { languages, window } from "vscode";
import type { PointbreakCli } from "../cli";
import type {
  HumanWriteContext,
  HumanWriteCoordinator,
} from "../humanWriteCoordinator";
import {
  buildProblemsSnapshot,
  type ProblemsSample,
  type ProblemsSnapshotOptions,
} from "../problemsSnapshot";
import {
  pickFolder,
  type ResolvedTargetResolution,
  type TargetResolution,
} from "../targetResolver";

export const PROBLEMS_OBSERVATION_TITLE = "VS Code Problems snapshot";
const RECORD_ACTION = "Record Problems";

export interface ProblemsCommandContext {
  readonly targetKey: string;
  readonly revisionId?: string;
}

export interface ProblemsWritePreview extends HumanWriteContext {
  readonly revisionId: string;
  readonly targetLabel: string;
  readonly body: string;
}

export interface RecordProblemsDependencies {
  readonly humanWrites?: HumanWriteCoordinator;
  readonly pickFolder: typeof pickFolder;
  readonly getDiagnostics: () => ProblemsSample;
  readonly buildSnapshot: (
    sample: ProblemsSample,
    options: ProblemsSnapshotOptions,
  ) => string;
  readonly now: () => string;
  readonly confirmWrite: (preview: ProblemsWritePreview) => Promise<boolean>;
  readonly showInformationMessage: (message: string) => Promise<unknown>;
  readonly showErrorMessage: (message: string) => Promise<unknown>;
}

export async function runRecordProblemsSnapshotCommand(
  cli: PointbreakCli,
  resolutions: TargetResolution[],
  context?: ProblemsCommandContext,
  overrides: Partial<RecordProblemsDependencies> = {},
): Promise<void> {
  const dependencies = { ...defaultDependencies(), ...overrides };
  const location = await resolveLocation(
    cli,
    resolutions,
    context,
    dependencies,
  );
  if (!location) return;
  const repo = location.resolution.folder.uri.fsPath;
  const targetLabel = location.resolution.target.label;
  const revisionId = location.revisionId;

  let body: string;
  try {
    const sample = dependencies.getDiagnostics();
    body = dependencies.buildSnapshot(sample, {
      repoRoot: repo,
      targetLabel,
      timestamp: dependencies.now(),
    });
  } catch {
    await dependencies.showErrorMessage(
      "Pointbreak could not build a target-bounded Problems snapshot.",
    );
    return;
  }

  const humanWrites = dependencies.humanWrites;
  if (!humanWrites) {
    await dependencies.showErrorMessage(
      "Pointbreak could not prepare the human write.",
    );
    return;
  }
  try {
    const result = await humanWrites.run({
      repo,
      resource: location.resolution.folder.uri,
      confirm: (writer) =>
        dependencies.confirmWrite({
          ...writer,
          revisionId,
          targetLabel,
          body,
        }),
      write: async ({ track }) => {
        const added = await cli.addObservation(repo, {
          revisionId,
          track,
          title: PROBLEMS_OBSERVATION_TITLE,
          target: { kind: "revision" },
          body,
          bodyContentType: "text/markdown",
        });
        if (added.revisionId !== revisionId) {
          throw new Error("Problems observation revision changed during write");
        }
        return added;
      },
    });
    if (!result) return;
  } catch {
    await dependencies.showErrorMessage(
      "Pointbreak could not record the Problems snapshot.",
    );
    return;
  }
  await dependencies.showInformationMessage("Problems snapshot recorded.");
}

interface ProblemsLocation {
  readonly resolution: ResolvedTargetResolution;
  readonly revisionId: string;
}

async function resolveLocation(
  cli: PointbreakCli,
  resolutions: TargetResolution[],
  context: ProblemsCommandContext | undefined,
  dependencies: Pick<
    RecordProblemsDependencies,
    "pickFolder" | "showErrorMessage"
  >,
): Promise<ProblemsLocation | undefined> {
  if (context) {
    const resolution = resolutions.find(
      (candidate): candidate is ResolvedTargetResolution =>
        candidate.kind === "resolved" &&
        candidate.target.key === context.targetKey,
    );
    if (!resolution || !context.revisionId?.trim()) {
      await dependencies.showErrorMessage(
        "Pointbreak could not identify an exact revision for this review context.",
      );
      return undefined;
    }
    return { resolution, revisionId: context.revisionId };
  }

  const resolution = await dependencies.pickFolder(resolutions);
  if (!resolution) return undefined;
  try {
    const revisions = await cli.revisionList(resolution.folder.uri.fsPath, {
      filter: "-is:superseded",
    });
    if (revisions.entries.length === 0) {
      await dependencies.showErrorMessage(
        "Pointbreak has no current revision. Capture current work before recording Problems.",
      );
      return undefined;
    }
    if (revisions.entries.length !== 1) {
      await dependencies.showErrorMessage(
        "Pointbreak found multiple current heads. Resolve competing heads from Attention before recording Problems.",
      );
      return undefined;
    }
    return { resolution, revisionId: revisions.entries[0].revisionId };
  } catch {
    await dependencies.showErrorMessage(
      "Pointbreak could not resolve a current revision for the Problems snapshot.",
    );
    return undefined;
  }
}

function defaultDependencies(): RecordProblemsDependencies {
  return {
    pickFolder,
    getDiagnostics: () => languages.getDiagnostics() as ProblemsSample,
    buildSnapshot: buildProblemsSnapshot,
    now: () => new Date().toISOString(),
    confirmWrite: async ({ actorId, track, revisionId, targetLabel, body }) =>
      (await window.showWarningMessage(
        `Problems snapshot preview for ${targetLabel}:\n\n${body}\nRevision: ${revisionId}\nWriter: ${actorId}\nTrack: ${track}`,
        { modal: true },
        RECORD_ACTION,
      )) === RECORD_ACTION,
    showInformationMessage: async (message) =>
      window.showInformationMessage(message),
    showErrorMessage: async (message) => window.showErrorMessage(message),
  };
}
