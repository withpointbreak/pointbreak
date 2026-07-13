import type { QuickPickItem, WorkspaceFolder } from "vscode";
import { window } from "vscode";
import {
  type PointbreakCli,
  PointbreakCliError,
  type StoreStatusDoc,
} from "./cli";

export interface ReviewTarget {
  key: string;
  label: string;
  storeIdentity: string;
  contextIdentity: string;
}

export type TargetResolution =
  | {
      kind: "resolved";
      folder: WorkspaceFolder;
      target: ReviewTarget;
      emptyInventory: boolean;
    }
  | { kind: "error"; folder: WorkspaceFolder; message: string };

export type ResolvedTargetResolution = Extract<
  TargetResolution,
  { kind: "resolved" }
>;

type ResolvedTarget = ResolvedTargetResolution;

interface FolderPickItem extends QuickPickItem {
  resolution: ResolvedTarget;
}

export async function resolveTargets(
  cli: PointbreakCli,
  folders: readonly WorkspaceFolder[],
): Promise<TargetResolution[]> {
  return Promise.all(
    folders.map(async (folder): Promise<TargetResolution> => {
      try {
        const status = await cli.storeStatus(folder.uri.fsPath);
        return {
          kind: "resolved",
          folder,
          target: reviewTargetFromStatus(status),
          emptyInventory: status.inventory.revisionObjects.length === 0,
        };
      } catch (error) {
        return {
          kind: "error",
          folder,
          message: `Pointbreak could not use ${folder.name}: ${errorMessage(error)}`,
        };
      }
    }),
  );
}

export function reviewTargetFromStatus(status: StoreStatusDoc): ReviewTarget {
  if (!status.storeIdentity || !status.contextIdentity) {
    throw new Error(
      "store status did not include its store and context identities",
    );
  }
  return {
    key: `${status.storeIdentity}/${status.contextIdentity}`,
    label: status.repositoryFamilyRef ?? status.storeRef ?? "Pointbreak",
    storeIdentity: status.storeIdentity,
    contextIdentity: status.contextIdentity,
  };
}

export async function pickFolder(
  resolutions: TargetResolution[],
): Promise<ResolvedTarget | undefined> {
  const distinct = new Map<string, ResolvedTarget>();
  for (const resolution of resolutions) {
    if (
      resolution.kind === "resolved" &&
      !distinct.has(resolution.target.key)
    ) {
      distinct.set(resolution.target.key, resolution);
    }
  }

  const candidates = [...distinct.values()];
  if (candidates.length === 0) {
    await window.showErrorMessage(
      "Pointbreak could not resolve a review target in this workspace. Open a Git worktree and try again.",
    );
    return undefined;
  }
  if (candidates.length === 1) {
    return candidates[0];
  }

  const items: FolderPickItem[] = candidates.map((resolution) => ({
    label: resolution.folder.name,
    description: resolution.target.label,
    resolution,
  }));
  const picked = await window.showQuickPick(items, {
    placeHolder: "Choose the Pointbreak review target",
  });
  return picked?.resolution;
}

function errorMessage(error: unknown): string {
  if (error instanceof PointbreakCliError && error.stderr.trim()) {
    return error.stderr.trim();
  }
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}
