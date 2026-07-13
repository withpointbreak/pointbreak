import { beforeEach, describe, expect, it, vi } from "vitest";
import type { WorkspaceFolder } from "vscode";
import type { PointbreakCli, StoreStatusDoc } from "../src/cli";
import {
  pickFolder,
  resolveTargets,
  reviewTargetFromStatus,
} from "../src/targetResolver";
import { workspaceFolder } from "./helpers/vscodeMock";

const vscodeMocks = vi.hoisted(() => ({
  showErrorMessage: vi.fn(),
  showQuickPick: vi.fn(),
}));

vi.mock("vscode", () => ({ window: vscodeMocks }));

beforeEach(() => {
  vscodeMocks.showErrorMessage.mockReset();
  vscodeMocks.showQuickPick.mockReset();
});

describe("resolveTargets", () => {
  it("resolves captured and empty repositories with an onboarding flag", async () => {
    const cli = cliReturning({
      "/a": status("store:a", "context:a", 1),
      "/b": status("store:b", "context:b", 0),
    });

    const resolutions = await resolveTargets(cli, folders("/a", "/b"));

    expect(
      resolutions.every((resolution) => resolution.kind === "resolved"),
    ).toBe(true);
    expect(
      resolutions.find((resolution) => resolution.folder.uri.fsPath === "/b"),
    ).toMatchObject({ kind: "resolved", emptyInventory: true });
  });

  it("maps a non-git folder to an actionable error", async () => {
    const cli = cliFailing("not a git worktree");

    const resolutions = await resolveTargets(cli, folders("/outside"));

    expect(resolutions[0]).toMatchObject({
      kind: "error",
      message: expect.stringMatching(/not a git worktree/),
    });
  });

  it("maps nested folders with one store and context to the same target key", async () => {
    const shared = status("store:shared", "context:shared", 1);
    const resolutions = await resolveTargets(
      cliReturning({ "/repo": shared, "/repo/packages/app": shared }),
      folders("/repo", "/repo/packages/app"),
    );

    expect(resolutions).toHaveLength(2);
    expect(resolutions.map(resolvedKey)).toEqual([
      "store:shared/context:shared",
      "store:shared/context:shared",
    ]);
  });

  it("keeps worktrees from one repository family distinct", async () => {
    const resolutions = await resolveTargets(
      cliReturning({
        "/worktree-a": status("store:shared", "context:a", 1),
        "/worktree-b": status("store:shared", "context:b", 1),
      }),
      folders("/worktree-a", "/worktree-b"),
    );

    expect(resolutions.map(resolvedKey)).toEqual([
      "store:shared/context:a",
      "store:shared/context:b",
    ]);
  });
});

describe("pickFolder", () => {
  it("returns the first equivalent folder without prompting", async () => {
    const shared = status("store:shared", "context:shared", 1);
    const resolutions = await resolveTargets(
      cliReturning({ "/repo": shared, "/repo/nested": shared }),
      folders("/repo", "/repo/nested"),
    );

    const picked = await pickFolder(resolutions);

    expect(picked?.folder.uri.fsPath).toBe("/repo");
    expect(vscodeMocks.showQuickPick).not.toHaveBeenCalled();
  });

  it("prompts when more than one distinct target exists", async () => {
    const resolutions = await resolveTargets(
      cliReturning({
        "/worktree-a": status("store:shared", "context:a", 1),
        "/worktree-b": status("store:shared", "context:b", 1),
      }),
      folders("/worktree-a", "/worktree-b"),
    );
    vscodeMocks.showQuickPick.mockImplementation(async (items) => items[1]);

    const picked = await pickFolder(resolutions);

    expect(vscodeMocks.showQuickPick).toHaveBeenCalledOnce();
    expect(vscodeMocks.showQuickPick.mock.calls[0][0]).toHaveLength(2);
    expect(picked?.folder.uri.fsPath).toBe("/worktree-b");
  });

  it("shows an actionable message when no folder resolves", async () => {
    const resolutions = await resolveTargets(
      cliFailing("permission denied"),
      folders("/blocked"),
    );

    await expect(pickFolder(resolutions)).resolves.toBeUndefined();
    expect(vscodeMocks.showErrorMessage).toHaveBeenCalledOnce();
  });
});

it("derives a review target from both opaque identities", () => {
  expect(reviewTargetFromStatus(status("store:one", "context:two", 0))).toEqual(
    {
      key: "store:one/context:two",
      label: "pointbreak",
    },
  );
});

function status(
  storeIdentity: string,
  contextIdentity: string,
  revisionCount: number,
): StoreStatusDoc {
  return {
    schema: "pointbreak.store-status",
    version: 1,
    storeIdentity,
    contextIdentity,
    repositoryFamilyRef: "pointbreak",
    inventory: {
      eventCount: revisionCount,
      artifactCount: revisionCount,
      revisionObjects: Array.from({ length: revisionCount }),
    },
    diagnostics: [],
  };
}

function folders(...paths: string[]): WorkspaceFolder[] {
  return paths.map(
    (folderPath) =>
      workspaceFolder(
        folderPath,
        folderPath.split("/").at(-1),
      ) as WorkspaceFolder,
  );
}

function cliReturning(statuses: Record<string, StoreStatusDoc>): PointbreakCli {
  return {
    storeStatus: vi.fn(async (repo: string) => statuses[repo]),
  } as unknown as PointbreakCli;
}

function cliFailing(message: string): PointbreakCli {
  return {
    storeStatus: vi.fn(async () => {
      throw new Error(message);
    }),
  } as unknown as PointbreakCli;
}

function resolvedKey(
  resolution: Awaited<ReturnType<typeof resolveTargets>>[number],
): string {
  if (resolution.kind === "error") {
    throw new Error(resolution.message);
  }
  return resolution.target.key;
}
