import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Task, WorkspaceFolder } from "vscode";

const vscodeMocks = vi.hoisted(() => ({ cancelDuringProgress: false }));

vi.mock("vscode", () => {
  const event = vi.fn(() => ({ dispose: vi.fn() }));
  return {
    ProgressLocation: { Notification: 15 },
    tasks: {
      executeTask: vi.fn(),
      fetchTasks: vi.fn(),
      onDidEndTask: event,
      onDidEndTaskProcess: event,
      onDidStartTaskProcess: event,
    },
    window: {
      showErrorMessage: vi.fn(),
      showInformationMessage: vi.fn(),
      showQuickPick: vi.fn(),
      showWarningMessage: vi.fn(),
      withProgress: vi.fn(
        async (
          _options: unknown,
          task: (
            progress: unknown,
            token: {
              onCancellationRequested(listener: () => void): {
                dispose(): void;
              };
            },
          ) => Promise<unknown>,
        ) =>
          task(
            {},
            {
              onCancellationRequested: vi.fn((listener: () => void) => {
                if (vscodeMocks.cancelDuringProgress) queueMicrotask(listener);
                return { dispose: vi.fn() };
              }),
            },
          ),
      ),
    },
  };
});

import type { AttentionItemNode } from "../src/attentionView";
import type {
  AttentionItem,
  RevisionListDoc,
  ValidationAddOptions,
  ValidationStatus,
} from "../src/cli";
import {
  proposeValidationStatus,
  runTaskAndRecordValidationCommand,
  validationConfirmation,
} from "../src/commands/runTaskAndRecordValidation";
import { HumanWriteCoordinator } from "../src/humanWriteCoordinator";
import type {
  ResolvedTargetResolution,
  TargetResolution,
} from "../src/targetResolver";
import type { TaskCandidate, TaskExecutionSummary } from "../src/taskRunner";
import { workspaceFolder } from "./helpers/vscodeMock";

const startedAt = "2026-07-15T20:00:00.000Z";
const completedAt = "2026-07-15T20:00:01.250Z";

beforeEach(() => {
  vscodeMocks.cancelDuringProgress = false;
});

describe("validation status proposal", () => {
  it.each([
    [summary({ exitCode: 0 }), "passed"],
    [summary({ exitCode: 2 }), "failed"],
    [summary({ processStarted: false }), "errored"],
    [
      summary({ terminationSource: "launch-error", processStarted: false }),
      "errored",
    ],
    [summary({ exitCode: undefined }), "errored"],
    [summary({ exitCode: 0, terminationSource: "cancelled" }), "skipped"],
  ] satisfies Array<
    [TaskExecutionSummary, ValidationStatus]
  >)("maps lifecycle facts to the advisory %s status", (execution, expected) => {
    expect(proposeValidationStatus(execution)).toBe(expected);
  });

  it("previews the editable status and warns that terminal cancellation may report zero", () => {
    const message = validationConfirmation({
      actorId: "actor:human:kevin",
      track: "human:local",
      revisionId: "rev:sha256:one",
      taskLabel: "workspace: test",
      checkName: "vscode-task:shell:workspace:test",
      execution: summary({ exitCode: 0 }),
      proposedStatus: "passed",
      status: "skipped",
      startedAt,
      completedAt,
    });

    expect(message).toContain("actor:human:kevin");
    expect(message).toContain("human:local");
    expect(message).toContain("rev:sha256:one");
    expect(message).toContain("workspace: test");
    expect(message).toContain("vscode-task:shell:workspace:test");
    expect(message).toContain("1,250 ms");
    expect(message).toContain("Exit code: 0");
    expect(message).toMatch(/selected status: skipped/i);
    expect(message).toMatch(/proposed status: passed/i);
    expect(message).toMatch(/terminal cancellation.+zero/i);
  });
});

describe("runTaskAndRecordValidationCommand", () => {
  it("uses the sole current head and configured human track for a palette run", async () => {
    const harness = commandHarness();

    await runTaskAndRecordValidationCommand(
      harness.cli as never,
      resolutions(),
      undefined,
      harness.dependencies,
    );

    expect(harness.dependencies.pickFolder).toHaveBeenCalledOnce();
    expect(harness.cli.revisionList).toHaveBeenCalledWith("/repo", {
      filter: "-is:superseded",
    });
    expect(harness.dependencies.pickRevision).not.toHaveBeenCalled();
    expect(harness.dependencies.pickTask).toHaveBeenCalledWith(
      expect.objectContaining({ name: "repo" }),
      undefined,
    );
    expect(harness.dependencies.pickStatus).toHaveBeenCalledWith({
      proposedStatus: "passed",
      taskLabel: "workspace: test",
      checkName: "vscode-task:shell:workspace:test",
      execution: summary({ exitCode: 0 }),
    });
    expect(harness.dependencies.confirmWrite).toHaveBeenCalledWith(
      expect.objectContaining({
        actorId: "actor:git-email:human@example.com",
        track: "human:local",
        revisionId: "rev:sha256:head",
        proposedStatus: "passed",
        status: "passed",
      }),
    );
    expect(harness.cli.addValidation).toHaveBeenCalledWith("/repo", {
      revisionId: "rev:sha256:head",
      track: "human:local",
      checkName: "vscode-task:shell:workspace:test",
      status: "passed",
      command: "workspace: test",
      exitCode: 0,
      startedAt,
      completedAt,
      trigger: "manual",
      summary:
        "VS Code reported the selected root task in 1250 ms. Root exit code: 0. Completion source: task-end. The validation status was explicitly selected and confirmed by the user.",
    });
    expect(harness.refresh).toHaveBeenCalledOnce();
    expect(harness.dependencies.showInformationMessage).toHaveBeenCalledWith(
      "Validation recorded.",
    );
  });

  it("refuses zero revisions without selecting or running a task", async () => {
    const harness = commandHarness();
    harness.cli.revisionList.mockResolvedValueOnce(revisions());

    await runTaskAndRecordValidationCommand(
      harness.cli as never,
      resolutions(),
      undefined,
      harness.dependencies,
    );

    expect(harness.dependencies.showErrorMessage).toHaveBeenCalledWith(
      expect.stringMatching(/capture.+before recording validation/i),
    );
    expect(harness.dependencies.pickRevision).not.toHaveBeenCalled();
    expect(harness.dependencies.pickTask).not.toHaveBeenCalled();
    expect(harness.dependencies.taskRunner.run).not.toHaveBeenCalled();
    expect(harness.cli.addValidation).not.toHaveBeenCalled();
  });

  it("selects one exact revision when independent current revisions coexist", async () => {
    const harness = commandHarness();
    const document = revisions("rev:sha256:merged", "rev:sha256:open");
    document.entries[0].mergeStatus = "merged";
    harness.cli.revisionList.mockResolvedValueOnce(document);
    harness.dependencies.pickRevision.mockResolvedValueOnce(
      document.entries[1],
    );

    await runTaskAndRecordValidationCommand(
      harness.cli as never,
      resolutions(),
      undefined,
      harness.dependencies,
    );

    expect(harness.dependencies.pickRevision).toHaveBeenCalledWith(
      document.entries,
    );
    expect(harness.dependencies.showErrorMessage).not.toHaveBeenCalledWith(
      expect.stringMatching(/competing heads/i),
    );
    expect(harness.dependencies.confirmWrite).toHaveBeenCalledWith(
      expect.objectContaining({ revisionId: "rev:sha256:open" }),
    );
    expect(harness.cli.addValidation).toHaveBeenCalledWith(
      "/repo",
      expect.objectContaining({ revisionId: "rev:sha256:open" }),
    );
  });

  it("records and runs nothing when exact revision selection is cancelled", async () => {
    const harness = commandHarness();
    harness.cli.revisionList.mockResolvedValueOnce(
      revisions("rev:sha256:a", "rev:sha256:b"),
    );
    harness.dependencies.pickRevision.mockResolvedValueOnce(undefined);

    await runTaskAndRecordValidationCommand(
      harness.cli as never,
      resolutions(),
      undefined,
      harness.dependencies,
    );

    expect(harness.dependencies.pickRevision).toHaveBeenCalledOnce();
    expect(harness.dependencies.pickTask).not.toHaveBeenCalled();
    expect(harness.dependencies.taskRunner.run).not.toHaveBeenCalled();
    expect(harness.cli.identityWhoami).not.toHaveBeenCalled();
    expect(harness.cli.addValidation).not.toHaveBeenCalled();
  });

  it("reruns a failed item as a later pass with the identical non-human key", async () => {
    const harness = commandHarness({ configuredTrack: "human:local" });
    const node = failedValidationNode();

    await runTaskAndRecordValidationCommand(
      harness.cli as never,
      resolutions(),
      node,
      harness.dependencies,
    );

    expect(harness.cli.revisionList).not.toHaveBeenCalled();
    expect(harness.dependencies.pickTask).toHaveBeenCalledWith(
      expect.objectContaining({ name: node.folder.name }),
      node.item.kind === "failed_validation" ? node.item.checkName : "",
    );
    expect(harness.cli.addValidation).toHaveBeenCalledWith(
      "/repo",
      expect.objectContaining({
        revisionId: "rev:sha256:failed",
        track: "agent:ci",
        checkName: "vscode-task:shell:workspace:test",
        status: "passed",
      }),
    );
    expect(harness.dependencies.findAttentionItem).toHaveBeenCalledWith(
      "store/context",
      "failed_validation:validation:sha256:failed",
    );
    expect(harness.dependencies.showInformationMessage).toHaveBeenCalledWith(
      "Validation recorded. The failed-validation attention item cleared after refresh.",
    );
  });

  it("does not run when no task matches a failed item's exact check name", async () => {
    const harness = commandHarness();
    harness.dependencies.pickTask.mockResolvedValueOnce(undefined);

    await runTaskAndRecordValidationCommand(
      harness.cli as never,
      resolutions(),
      failedValidationNode(),
      harness.dependencies,
    );

    expect(harness.dependencies.pickTask).toHaveBeenCalledWith(
      expect.anything(),
      "vscode-task:shell:workspace:test",
    );
    expect(harness.dependencies.taskRunner.run).not.toHaveBeenCalled();
    expect(harness.cli.addValidation).not.toHaveBeenCalled();
  });

  it("allows an edited status but never reports skipped evidence as clearing", async () => {
    const harness = commandHarness();
    harness.dependencies.pickStatus.mockResolvedValueOnce("skipped");

    await runTaskAndRecordValidationCommand(
      harness.cli as never,
      resolutions(),
      failedValidationNode(),
      harness.dependencies,
    );

    expect(harness.dependencies.confirmWrite).toHaveBeenCalledWith(
      expect.objectContaining({
        proposedStatus: "passed",
        status: "skipped",
      }),
    );
    expect(harness.cli.addValidation).toHaveBeenCalledWith(
      "/repo",
      expect.objectContaining({ status: "skipped" }),
    );
    expect(
      harness.dependencies.showInformationMessage.mock.calls.flat().join(" "),
    ).not.toMatch(/item cleared/i);
  });

  it("propagates progress cancellation and proposes skipped evidence", async () => {
    const harness = commandHarness();
    vscodeMocks.cancelDuringProgress = true;
    harness.dependencies.taskRunner.run.mockImplementationOnce(
      async (_task, signal) => {
        await Promise.resolve();
        expect(signal?.aborted).toBe(true);
        return summary({ terminationSource: "cancelled" });
      },
    );

    await runTaskAndRecordValidationCommand(
      harness.cli as never,
      resolutions(),
      undefined,
      harness.dependencies,
    );

    expect(harness.dependencies.pickStatus).toHaveBeenCalledWith(
      expect.objectContaining({ proposedStatus: "skipped" }),
    );
    expect(harness.cli.addValidation).toHaveBeenCalledWith(
      "/repo",
      expect.objectContaining({ status: "skipped" }),
    );
  });

  it("reports a confirmed pass that remains after refresh", async () => {
    const harness = commandHarness();
    const node = failedValidationNode();
    harness.dependencies.findAttentionItem.mockReturnValueOnce(node.item);

    await runTaskAndRecordValidationCommand(
      harness.cli as never,
      resolutions(),
      node,
      harness.dependencies,
    );

    expect(harness.dependencies.showInformationMessage).toHaveBeenCalledWith(
      "Validation recorded. The failed-validation attention item remains after refresh.",
    );
  });

  it("records nothing when status selection or final confirmation is cancelled", async () => {
    const statusCancelled = commandHarness();
    statusCancelled.dependencies.pickStatus.mockResolvedValueOnce(undefined);

    await runTaskAndRecordValidationCommand(
      statusCancelled.cli as never,
      resolutions(),
      undefined,
      statusCancelled.dependencies,
    );

    expect(statusCancelled.cli.identityWhoami).not.toHaveBeenCalled();
    expect(statusCancelled.cli.addValidation).not.toHaveBeenCalled();
    expect(statusCancelled.refresh).not.toHaveBeenCalled();

    const confirmationCancelled = commandHarness();
    confirmationCancelled.dependencies.confirmWrite.mockResolvedValue(false);
    await runTaskAndRecordValidationCommand(
      confirmationCancelled.cli as never,
      resolutions(),
      undefined,
      confirmationCancelled.dependencies,
    );

    expect(confirmationCancelled.cli.addValidation).not.toHaveBeenCalled();
    expect(confirmationCancelled.refresh).not.toHaveBeenCalled();
  });
});

function commandHarness(options: { configuredTrack?: string } = {}) {
  const cli = cliMock();
  const refresh = vi.fn(async () => undefined);
  const humanWrites = new HumanWriteCoordinator(cli as never, {
    resolveTrack: () => options.configuredTrack ?? "human:local",
    showDiagnostic: vi.fn(async () => undefined),
    refresh,
    showRefreshError: vi.fn(async () => undefined),
  });
  const task = rootTask();
  const candidate: TaskCandidate = {
    task,
    checkName: "vscode-task:shell:workspace:test",
  };
  const execution = summary({ exitCode: 0 });
  const pickStatus = vi.fn<
    (prompt: {
      proposedStatus: ValidationStatus;
      taskLabel: string;
      checkName: string;
      execution: TaskExecutionSummary;
    }) => Promise<ValidationStatus | undefined>
  >(
    async ({ proposedStatus }: { proposedStatus: ValidationStatus }) =>
      proposedStatus,
  );
  const now = vi
    .fn<() => string>()
    .mockReturnValueOnce(startedAt)
    .mockReturnValueOnce(completedAt);
  const taskRunner = {
    run: vi.fn<
      (task: Task, signal?: AbortSignal) => Promise<TaskExecutionSummary>
    >(async () => execution),
  };
  const pickRevision = vi.fn<
    (
      entries: readonly RevisionListDoc["entries"][number][],
    ) => Promise<RevisionListDoc["entries"][number] | undefined>
  >(async (entries) => entries[0]);
  const dependencies = {
    humanWrites,
    taskRunner,
    pickFolder: vi.fn(async () => resolved()),
    pickRevision,
    pickTask: vi.fn<
      (
        folder: WorkspaceFolder,
        requiredCheckName?: string,
      ) => Promise<TaskCandidate | undefined>
    >(async () => candidate),
    pickStatus,
    now,
    confirmWrite: vi.fn(async () => true),
    findAttentionItem: vi.fn<
      (targetKey: string, attentionId: string) => AttentionItem | undefined
    >(() => undefined),
    showInformationMessage: vi.fn(async () => undefined),
    showErrorMessage: vi.fn(async () => undefined),
  };
  return { cli, dependencies, refresh };
}

function cliMock() {
  return {
    identityWhoami: vi.fn(async () => ({
      schema: "pointbreak.identity-whoami",
      version: 1,
      actorId: "actor:git-email:human@example.com",
      diagnostics: [],
    })),
    revisionList: vi.fn(async () => revisions("rev:sha256:head")),
    addValidation: vi.fn(
      async (_repo: string, options: ValidationAddOptions) => ({
        schema: "pointbreak.review-validation-add",
        version: 1,
        revisionId: options.revisionId,
        validationCheckId: "validation:sha256:new",
        eventId: "evt:sha256:new",
        trackId: options.track,
        target: { kind: "revision", revisionId: options.revisionId },
        status: options.status,
        diagnostics: [],
      }),
    ),
  };
}

function summary(
  overrides: Partial<TaskExecutionSummary> = {},
): TaskExecutionSummary {
  return {
    durationMs: 1250,
    exitCode: 0,
    processStarted: true,
    terminationSource: "task-end",
    ...overrides,
  };
}

function rootTask(): Task {
  return {
    definition: { type: "shell" },
    name: "test",
    source: "workspace",
    scope: resolved().folder,
  } as Task;
}

function failedValidationNode(): AttentionItemNode {
  return {
    kind: "attention-item",
    label: "failed_validation",
    targetKey: "store/context",
    folder: resolved().folder,
    description: "primary",
    revisionId: "rev:sha256:failed",
    attentionId: "failed_validation:validation:sha256:failed",
    item: {
      id: "failed_validation:validation:sha256:failed",
      kind: "failed_validation",
      tier: "primary",
      revisionId: "rev:sha256:failed",
      freshness: { state: "current" },
      observedAt: startedAt,
      validationCheckId: "validation:sha256:failed",
      checkName: "vscode-task:shell:workspace:test",
      status: "failed",
      trackId: "agent:ci",
      recordedBy: "actor:agent:ci",
    },
    lens: "attention",
    command: "pointbreak.openAnnotatedDiff",
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

function revisions(...revisionIds: string[]): RevisionListDoc {
  return {
    schema: "pointbreak.review-revision-list",
    version: 1,
    entries: revisionIds.map((revisionId) => ({
      revisionId,
      capturedAt: startedAt,
      mergeStatus: "open",
    })),
    revisionCount: revisionIds.length,
    eventCount: revisionIds.length,
    eventSetHash: "sha256:events",
    diagnostics: [],
  };
}
