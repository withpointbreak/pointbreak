import { describe, expect, it, vi } from "vitest";
import type {
  Event,
  Task,
  TaskExecution,
  TaskProcessEndEvent,
  TaskProcessStartEvent,
  WorkspaceFolder,
} from "vscode";

vi.mock("vscode", () => {
  const event = vi.fn(() => ({ dispose: vi.fn() }));
  return {
    tasks: {
      executeTask: vi.fn(),
      fetchTasks: vi.fn(),
      onDidEndTask: event,
      onDidEndTaskProcess: event,
      onDidStartTaskProcess: event,
    },
    window: {
      showErrorMessage: vi.fn(),
      showQuickPick: vi.fn(),
    },
  };
});

import {
  analyzeTaskCandidates,
  deriveTaskCheckName,
  pickRootTask,
  TaskRunner,
} from "../src/taskRunner";
import { workspaceFolder } from "./helpers/vscodeMock";

describe("task identity", () => {
  it("keeps ordinary public task labels readable in the stable key", () => {
    const task = fakeTask(
      "format-check - extensions/vscode",
      folder("/repo"),
      "npm",
      "npm",
    );

    expect(deriveTaskCheckName(task)).toBe(
      "vscode-task:npm:npm:format-check - extensions/vscode",
    );
  });

  it("escapes only the component delimiter and escape marker", () => {
    const task = fakeTask(
      "test:unit?",
      folder("/repo"),
      "npm tasks",
      "npm:script/β",
    );

    expect(deriveTaskCheckName(task)).toBe(
      "vscode-task:npm%3Ascript/β:npm tasks:test%3Aunit?",
    );

    const literalEscape = fakeTask(
      "test%3Aunit?",
      folder("/repo"),
      "npm tasks",
      "npm:script/β",
    );
    expect(deriveTaskCheckName(literalEscape)).toBe(
      "vscode-task:npm%3Ascript/β:npm tasks:test%253Aunit?",
    );
    expect(deriveTaskCheckName(literalEscape)).not.toBe(
      deriveTaskCheckName(task),
    );
  });

  it("keeps a root key stable across dependency edits and reruns", () => {
    const before = fakeTask("check", folder("/repo"), "workspace", "shell", {
      dependsOn: ["build"],
      path: "/private/first",
    });
    const after = fakeTask("check", folder("/repo"), "workspace", "shell", {
      dependsOn: ["build", "lint"],
      path: "/private/second",
      providerPayload: { changed: true },
    });

    expect(deriveTaskCheckName(before)).toBe(deriveTaskCheckName(after));
  });

  it("keeps only tasks explicitly scoped to the resolved target folder", () => {
    const target = folder("/repo", "repo");
    const tasks = [
      fakeTask("target", folder("/repo", "alias")),
      fakeTask("other", folder("/other")),
      fakeTask("workspace", 2),
      fakeTask("unscoped", undefined),
    ];

    expect(
      analyzeTaskCandidates(tasks, target).map(({ task }) => task.name),
    ).toEqual(["target"]);
  });

  it("blocks every duplicate key and leaves unrelated candidates eligible", () => {
    const target = folder("/repo");
    const candidates = analyzeTaskCandidates(
      [
        fakeTask("check", target, "workspace", "shell", {
          script: "first",
        }),
        fakeTask("check", target, "workspace", "shell", {
          script: "second",
        }),
        fakeTask("lint", target, "workspace", "shell"),
      ],
      target,
    );

    expect(candidates.map(({ blockedReason }) => blockedReason)).toEqual([
      expect.stringMatching(/rename or disambiguate/i),
      expect.stringMatching(/rename or disambiguate/i),
      undefined,
    ]);
    expect(candidates[0].checkName).toBe(candidates[1].checkName);
  });
});

describe("task selection", () => {
  it("fetches all tasks but displays only tasks scoped to the target folder", async () => {
    const target = folder("/repo");
    const targetTask = fakeTask("check", target);
    const showQuickPick = vi.fn(async (items: readonly unknown[]) => items[0]);
    const fetchTasks = vi.fn(async () => [
      fakeTask("other", folder("/other")),
      targetTask,
    ]);

    const selected = await pickRootTask(target, {
      fetchTasks,
      showErrorMessage: vi.fn(),
      showQuickPick,
    });

    expect(fetchTasks).toHaveBeenCalledOnce();
    expect(showQuickPick).toHaveBeenCalledOnce();
    expect(showQuickPick.mock.calls[0][0]).toHaveLength(1);
    expect(selected).toMatchObject({
      task: targetTask,
      checkName: deriveTaskCheckName(targetTask),
    });
  });

  it("rejects a selected collision with actionable public-label guidance", async () => {
    const target = folder("/repo");
    const duplicates = [
      fakeTask("check", target, "workspace", "shell", { script: "first" }),
      fakeTask("check", target, "workspace", "shell", { script: "second" }),
    ];
    const showErrorMessage = vi.fn();

    const selected = await pickRootTask(target, {
      fetchTasks: vi.fn(async () => duplicates),
      showErrorMessage,
      showQuickPick: vi.fn(async (items: readonly unknown[]) => items[1]),
    });

    expect(selected).toBeUndefined();
    expect(showErrorMessage).toHaveBeenCalledWith(
      expect.stringMatching(/type, source, or name/i),
    );
  });

  it("shows only an exact check-name match for a failed-validation rerun", async () => {
    const target = folder("/repo");
    const matching = fakeTask("check", target);
    const other = fakeTask("lint", target);
    const showQuickPick = vi.fn(async (items: readonly unknown[]) => items[0]);

    const selected = await pickRootTask(
      target,
      {
        fetchTasks: vi.fn(async () => [other, matching]),
        showErrorMessage: vi.fn(),
        showQuickPick,
      },
      deriveTaskCheckName(matching),
    );

    expect(showQuickPick.mock.calls[0][0]).toHaveLength(1);
    expect(showQuickPick.mock.calls[0][0][0]).toMatchObject({ picked: true });
    expect(selected?.task).toBe(matching);
  });

  it("rejects a failed-validation rerun with no exact task identity", async () => {
    const target = folder("/repo");
    const showErrorMessage = vi.fn();
    const showQuickPick = vi.fn();

    await expect(
      pickRootTask(
        target,
        {
          fetchTasks: vi.fn(async () => [fakeTask("other", target)]),
          showErrorMessage,
          showQuickPick,
        },
        "vscode-task:shell:workspace:missing",
      ),
    ).resolves.toBeUndefined();

    expect(showQuickPick).not.toHaveBeenCalled();
    expect(showErrorMessage).toHaveBeenCalledWith(
      expect.stringMatching(
        /no target-scoped task matches.+type, source, or name/i,
      ),
    );
  });
});

describe("root task lifecycle", () => {
  it("ignores child and unrelated event noise and summarizes only the exact execution", async () => {
    const rootTask = fakeTask("root", folder("/repo"));
    const childTask = fakeTask("dependency", folder("/repo"));
    const root = execution(rootTask);
    const child = execution(childTask);
    const unrelated = execution(rootTask);
    const harness = taskHarness(root, [100, 145]);
    const result = harness.runner.run(rootTask);
    await settled();

    harness.processStart.fire({ execution: child, processId: 10 });
    harness.processEnd.fire({ execution: child, exitCode: 9 });
    harness.taskEnd.fire({ execution: child });
    harness.processStart.fire({ execution: unrelated, processId: 11 });
    harness.processEnd.fire({ execution: unrelated, exitCode: 8 });
    harness.taskEnd.fire({ execution: unrelated });
    await expectPending(result);

    harness.processStart.fire({ execution: root, processId: 12 });
    harness.processEnd.fire({ execution: root, exitCode: 0 });
    harness.taskEnd.fire({ execution: root });

    await expect(result).resolves.toEqual({
      durationMs: 45,
      exitCode: 0,
      processStarted: true,
      terminationSource: "task-end",
    });
    expect(harness.listenerCounts()).toEqual([0, 0, 0]);
  });

  it("uses the matching task-end event as authoritative for a processless root", async () => {
    const rootTask = fakeTask("root", folder("/repo"));
    const root = execution(rootTask);
    const harness = taskHarness(root, [20, 27]);
    const result = harness.runner.run(rootTask);
    await settled();

    harness.taskEnd.fire({ execution: root });

    await expect(result).resolves.toEqual({
      durationMs: 7,
      processStarted: false,
      terminationSource: "task-end",
    });
    expect(harness.listenerCounts()).toEqual([0, 0, 0]);
  });

  it("terminates on cancellation, waits for root end, and reports that source", async () => {
    const rootTask = fakeTask("root", folder("/repo"));
    const root = execution(rootTask);
    const harness = taskHarness(root, [30, 39]);
    const cancellation = new AbortController();
    const result = harness.runner.run(rootTask, cancellation.signal);
    await settled();

    cancellation.abort();
    expect(root.terminate).toHaveBeenCalledOnce();
    await expectPending(result);
    harness.taskEnd.fire({ execution: root });

    await expect(result).resolves.toMatchObject({
      durationMs: 9,
      terminationSource: "cancelled",
    });
    expect(harness.listenerCounts()).toEqual([0, 0, 0]);
  });

  it("summarizes a launch error and cleans every listener", async () => {
    const rootTask = fakeTask("root", folder("/repo"));
    const harness = taskHarness(new Error("task launch failed"), [40, 46]);

    await expect(harness.runner.run(rootTask)).resolves.toEqual({
      durationMs: 6,
      processStarted: false,
      terminationSource: "launch-error",
    });
    expect(harness.listenerCounts()).toEqual([0, 0, 0]);
  });

  it("terminates active work and cleans listeners during extension teardown", async () => {
    const rootTask = fakeTask("root", folder("/repo"));
    const root = execution(rootTask);
    const harness = taskHarness(root, [50]);
    const result = harness.runner.run(rootTask);
    await settled();

    harness.runner.dispose();

    expect(root.terminate).toHaveBeenCalledOnce();
    await expect(result).rejects.toThrow(/no longer available/i);
    expect(harness.listenerCounts()).toEqual([0, 0, 0]);
  });
});

function fakeTask(
  name: string,
  scope: WorkspaceFolder | number | undefined,
  source = "workspace",
  type = "shell",
  extraDefinition: Record<string, unknown> = {},
): Task {
  return {
    definition: { type, ...extraDefinition },
    name,
    scope,
    source,
  } as Task;
}

function folder(path: string, name = path): WorkspaceFolder {
  return workspaceFolder(path, name) as WorkspaceFolder;
}

function execution(task: Task) {
  return { task, terminate: vi.fn<() => void>() };
}

function taskHarness(executionOrError: TaskExecution | Error, times: number[]) {
  const taskEnd = event<{ execution: TaskExecution }>();
  const processStart = event<TaskProcessStartEvent>();
  const processEnd = event<TaskProcessEndEvent>();
  const executeTask =
    executionOrError instanceof Error
      ? vi.fn(async () => {
          throw executionOrError;
        })
      : vi.fn(async () => executionOrError);
  const now = vi.fn();
  for (const time of times) now.mockReturnValueOnce(time);
  const runner = new TaskRunner(
    {
      executeTask,
      onDidEndTask: taskEnd.subscribe,
      onDidEndTaskProcess: processEnd.subscribe,
      onDidStartTaskProcess: processStart.subscribe,
    },
    now,
  );

  return {
    listenerCounts: () => [
      taskEnd.listenerCount(),
      processStart.listenerCount(),
      processEnd.listenerCount(),
    ],
    processEnd,
    processStart,
    runner,
    taskEnd,
  };
}

function event<T>() {
  const listeners = new Set<(value: T) => unknown>();
  return {
    fire(value: T): void {
      for (const listener of listeners) listener(value);
    },
    listenerCount: () => listeners.size,
    subscribe: ((listener: (value: T) => unknown) => {
      listeners.add(listener);
      return { dispose: () => listeners.delete(listener) };
    }) as Event<T>,
  };
}

async function expectPending(promise: Promise<unknown>): Promise<void> {
  const sentinel = Symbol("pending");
  await expect(
    Promise.race([promise, Promise.resolve(sentinel)]),
  ).resolves.toBe(sentinel);
}

async function settled(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}
