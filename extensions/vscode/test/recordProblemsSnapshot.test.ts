import { beforeEach, describe, expect, it, vi } from "vitest";
import type { WorkspaceFolder } from "vscode";

vi.mock("vscode", () => ({
  commands: { executeCommand: vi.fn() },
  languages: { getDiagnostics: vi.fn(() => []) },
  window: {
    showErrorMessage: vi.fn(),
    showInformationMessage: vi.fn(),
    showWarningMessage: vi.fn(),
  },
}));

import type { ObservationOptions, RevisionListDoc } from "../src/cli";
import { runRecordProblemsSnapshotCommand } from "../src/commands/recordProblemsSnapshot";
import {
  type HumanWriteContext,
  HumanWriteCoordinator,
} from "../src/humanWriteCoordinator";
import type { ProblemsSample } from "../src/problemsSnapshot";
import type {
  ResolvedTargetResolution,
  TargetResolution,
} from "../src/targetResolver";
import { workspaceFolder } from "./helpers/vscodeMock";

const body = "# Problems snapshot\n\nExact body bytes.\n";
const timestamp = "2026-07-15T12:34:56.000Z";

beforeEach(() => {
  vi.clearAllMocks();
});

describe("runRecordProblemsSnapshotCommand", () => {
  it.each([
    "revision",
    "attention",
  ])("uses the explicit %s context revision without listing or guessing", async () => {
    const cli = cliMock();
    const dependencies = deps();

    await runRecordProblemsSnapshotCommand(
      cli as never,
      resolutions(),
      { targetKey: "store/context", revisionId: "rev:sha256:context" },
      dependencies,
    );

    expect(cli.revisionList).not.toHaveBeenCalled();
    expect(dependencies.getDiagnostics).toHaveBeenCalledOnce();
    expect(dependencies.buildSnapshot).toHaveBeenCalledOnce();
    expect(dependencies.buildSnapshot).toHaveBeenCalledWith([], {
      repoRoot: "/repo",
      targetLabel: "repo",
      timestamp,
    });
    expect(dependencies.confirmWrite).toHaveBeenCalledWith({
      actorId: "actor:git-email:human@example.com",
      track: "human:local",
      revisionId: "rev:sha256:context",
      targetLabel: "repo",
      body,
    });
    expect(cli.addObservation).toHaveBeenCalledWith("/repo", {
      revisionId: "rev:sha256:context",
      track: "human:local",
      title: "VS Code Problems snapshot",
      target: { kind: "revision" },
      body,
      bodyContentType: "text/markdown",
    });
    expect(dependencies.refresh).toHaveBeenCalledOnce();
  });

  it("uses the sole current head for a palette invocation", async () => {
    const cli = cliMock();
    cli.revisionList.mockResolvedValueOnce(revisions("rev:sha256:head"));
    const dependencies = deps();

    await runRecordProblemsSnapshotCommand(
      cli as never,
      resolutions(),
      undefined,
      dependencies,
    );

    expect(dependencies.pickFolder).toHaveBeenCalledOnce();
    expect(cli.revisionList).toHaveBeenCalledWith("/repo", {
      filter: "-is:superseded",
    });
    expect(cli.addObservation).toHaveBeenCalledWith(
      "/repo",
      expect.objectContaining({ revisionId: "rev:sha256:head" }),
    );
  });

  it.each([
    {
      name: "zero heads",
      ids: [] as string[],
      guidance: /capture.+before recording Problems/i,
    },
    {
      name: "multiple heads",
      ids: ["rev:sha256:a", "rev:sha256:b"],
      guidance: /multiple current heads.+Attention/i,
    },
  ])("refuses $name and routes the human without sampling", async ({
    ids,
    guidance,
  }) => {
    const cli = cliMock();
    cli.revisionList.mockResolvedValueOnce(revisions(...ids));
    const dependencies = deps();

    await runRecordProblemsSnapshotCommand(
      cli as never,
      resolutions(),
      undefined,
      dependencies,
    );

    expect(dependencies.showErrorMessage).toHaveBeenCalledWith(
      expect.stringMatching(guidance),
    );
    expect(dependencies.getDiagnostics).not.toHaveBeenCalled();
    expect(dependencies.buildSnapshot).not.toHaveBeenCalled();
    expect(cli.addObservation).not.toHaveBeenCalled();
  });

  it("builds once and reconfirms the same exact body when the actor changes", async () => {
    const cli = cliMock();
    cli.identityWhoami
      .mockResolvedValueOnce(identity("actor:human:old"))
      .mockResolvedValueOnce(identity("actor:human:new"))
      .mockResolvedValueOnce(identity("actor:human:new"));
    const refresh = vi.fn(async () => undefined);
    const humanWrites = new HumanWriteCoordinator(cli as never, {
      resolveTrack: () => "human:local",
      showDiagnostic: vi.fn(async () => undefined),
      refresh,
      showRefreshError: vi.fn(async () => undefined),
    });
    const dependencies = deps({ humanWrites, refresh });

    await runRecordProblemsSnapshotCommand(
      cli as never,
      resolutions(),
      { targetKey: "store/context", revisionId: "rev:sha256:context" },
      dependencies,
    );

    expect(dependencies.buildSnapshot).toHaveBeenCalledOnce();
    expect(dependencies.confirmWrite.mock.calls).toEqual([
      [
        expect.objectContaining({
          actorId: "actor:human:old",
          revisionId: "rev:sha256:context",
          body,
        }),
      ],
      [
        expect.objectContaining({
          actorId: "actor:human:new",
          revisionId: "rev:sha256:context",
          body,
        }),
      ],
    ]);
    expect(cli.addObservation).toHaveBeenCalledOnce();
    expect(refresh).toHaveBeenCalledOnce();
  });

  it("records an explicit zero-diagnostic body with the repeated caveat", async () => {
    const cli = cliMock();
    const dependencies = deps({ buildSnapshot: undefined });

    await runRecordProblemsSnapshotCommand(
      cli as never,
      resolutions(),
      { targetKey: "store/context", revisionId: "rev:sha256:context" },
      dependencies,
    );

    const options = cli.addObservation.mock.calls[0][1];
    const recordedBody = options.body ?? "";
    expect(recordedBody).toContain("No diagnostics were currently reported.");
    expect(recordedBody.match(/incomplete point-in-time view/g)).toHaveLength(
      2,
    );
  });

  it("cancels final confirmation without writing or refreshing", async () => {
    const cli = cliMock();
    const dependencies = deps();
    dependencies.confirmWrite.mockResolvedValueOnce(false);

    await runRecordProblemsSnapshotCommand(
      cli as never,
      resolutions(),
      { targetKey: "store/context", revisionId: "rev:sha256:context" },
      dependencies,
    );

    expect(cli.addObservation).not.toHaveBeenCalled();
    expect(dependencies.refresh).not.toHaveBeenCalled();
  });
});

function cliMock() {
  return {
    identityWhoami: vi.fn(async () =>
      identity("actor:git-email:human@example.com"),
    ),
    revisionList: vi.fn(async () => revisions("rev:sha256:head")),
    addObservation: vi.fn(
      async (_repo: string, options: ObservationOptions) => ({
        schema: "pointbreak.review-observation-add",
        version: 1,
        revisionId: options.revisionId,
        observationId: "obs:sha256:problems",
        eventId: "evt:sha256:problems",
        trackId: "human:local",
        target: { kind: "revision", revisionId: options.revisionId },
        diagnostics: [],
      }),
    ),
  };
}

function deps(
  overrides: {
    humanWrites?: HumanWriteCoordinator;
    refresh?: () => Promise<void>;
    buildSnapshot?: undefined;
  } = {},
) {
  const refresh = overrides.refresh ?? vi.fn(async () => undefined);
  const confirmWrite = vi.fn(async (_context: HumanWriteContext) => true);
  const humanWrites =
    overrides.humanWrites ??
    ({
      run: vi.fn(async (request) => {
        const context = {
          actorId: "actor:git-email:human@example.com",
          track: "human:local",
        };
        if (!(await request.confirm(context))) return undefined;
        const document = await request.write(context);
        await refresh();
        return { document, refreshed: true };
      }),
    } as unknown as HumanWriteCoordinator);
  const dependencies = {
    humanWrites,
    pickFolder: vi.fn(async () => resolved()),
    getDiagnostics: vi.fn(() => [] as unknown as ProblemsSample),
    buildSnapshot: vi.fn(() => body),
    now: vi.fn(() => timestamp),
    confirmWrite,
    showInformationMessage: vi.fn(async () => undefined),
    showErrorMessage: vi.fn(async () => undefined),
    refresh,
  };
  if (overrides.buildSnapshot === undefined && "buildSnapshot" in overrides) {
    Reflect.deleteProperty(dependencies, "buildSnapshot");
  }
  return dependencies;
}

function identity(actorId: string) {
  return {
    schema: "pointbreak.identity-whoami" as const,
    version: 1 as const,
    actorId,
    diagnostics: [],
  };
}

function revisions(...revisionIds: string[]): RevisionListDoc {
  return {
    schema: "pointbreak.review-revision-list",
    version: 1,
    entries: revisionIds.map((revisionId) => ({
      revisionId,
      capturedAt: timestamp,
      mergeStatus: "open",
    })),
    revisionCount: revisionIds.length,
    eventCount: revisionIds.length,
    eventSetHash: "sha256:events",
    diagnostics: [],
  };
}

function resolutions(): TargetResolution[] {
  return [resolved()];
}

function resolved(): ResolvedTargetResolution {
  return {
    kind: "resolved",
    folder: workspaceFolder("/repo", "repo") as WorkspaceFolder,
    target: {
      key: "store/context",
      label: "repo",
      storeIdentity: "store",
      contextIdentity: "context",
    },
    emptyInventory: false,
  };
}
