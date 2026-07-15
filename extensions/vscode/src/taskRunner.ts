import {
  type Disposable,
  type Event,
  type QuickPickItem,
  type Task,
  type TaskExecution,
  type TaskProcessEndEvent,
  type TaskProcessStartEvent,
  tasks,
  type WorkspaceFolder,
  window,
} from "vscode";
import { analyzeTaskCandidates, type TaskCandidate } from "./taskIdentity";

export type { TaskCandidate } from "./taskIdentity";
export { analyzeTaskCandidates, deriveTaskCheckName } from "./taskIdentity";

export interface TaskExecutionSummary {
  readonly durationMs: number;
  readonly exitCode?: number;
  readonly processStarted: boolean;
  readonly terminationSource: "task-end" | "cancelled" | "launch-error";
}

interface TaskPickerDependencies {
  readonly fetchTasks: () => Thenable<Task[]>;
  readonly showQuickPick: (
    items: readonly QuickPickItem[],
    options: { readonly placeHolder: string },
  ) => Thenable<unknown>;
  readonly showErrorMessage: (message: string) => Thenable<unknown>;
}

interface TaskLifecycle {
  readonly executeTask: (task: Task) => Thenable<TaskExecution>;
  readonly onDidEndTask: Event<{ readonly execution: TaskExecution }>;
  readonly onDidStartTaskProcess: Event<TaskProcessStartEvent>;
  readonly onDidEndTaskProcess: Event<TaskProcessEndEvent>;
}

interface TaskPickItem extends QuickPickItem {
  readonly candidate: TaskCandidate;
}

type PendingEvent =
  | { readonly kind: "task-end"; readonly execution: TaskExecution }
  | { readonly kind: "process-start"; readonly execution: TaskExecution }
  | {
      readonly kind: "process-end";
      readonly execution: TaskExecution;
      readonly exitCode: number | undefined;
    };

const defaultPickerDependencies: TaskPickerDependencies = {
  fetchTasks: () => tasks.fetchTasks(),
  showQuickPick: (items, options) => window.showQuickPick(items, options),
  showErrorMessage: (message) => window.showErrorMessage(message),
};

const defaultLifecycle: TaskLifecycle = {
  executeTask: (task) => tasks.executeTask(task),
  onDidEndTask: tasks.onDidEndTask,
  onDidEndTaskProcess: tasks.onDidEndTaskProcess,
  onDidStartTaskProcess: tasks.onDidStartTaskProcess,
};

export async function pickRootTask(
  targetFolder: WorkspaceFolder,
  dependencies: TaskPickerDependencies = defaultPickerDependencies,
  requiredCheckName?: string,
): Promise<TaskCandidate | undefined> {
  const targetCandidates = analyzeTaskCandidates(
    await dependencies.fetchTasks(),
    targetFolder,
  );
  if (targetCandidates.length === 0) {
    await dependencies.showErrorMessage(
      `No VS Code tasks are scoped to ${targetFolder.name}. Define a folder-scoped task and try again.`,
    );
    return undefined;
  }
  const candidates = requiredCheckName
    ? targetCandidates.filter(
        (candidate) => candidate.checkName === requiredCheckName,
      )
    : targetCandidates;
  if (candidates.length === 0) {
    await dependencies.showErrorMessage(
      `No target-scoped task matches ${requiredCheckName}. Check the public task type, source, or name and try again.`,
    );
    return undefined;
  }

  const items: TaskPickItem[] = candidates.map((candidate) => ({
    label: candidate.task.name,
    description: candidate.task.source,
    detail: candidate.blockedReason ?? candidate.checkName,
    ...(requiredCheckName ? { picked: true } : {}),
    candidate,
  }));
  const picked = await dependencies.showQuickPick(items, {
    placeHolder: `Choose a task from ${targetFolder.name}`,
  });
  const candidate = items.find((item) => item === picked)?.candidate;
  if (!candidate) return undefined;
  if (candidate.blockedReason) {
    await dependencies.showErrorMessage(candidate.blockedReason);
    return undefined;
  }
  return candidate;
}

export class TaskRunner implements Disposable {
  private active: ActiveTaskRun | undefined;
  private disposed = false;

  constructor(
    private readonly lifecycle: TaskLifecycle = defaultLifecycle,
    private readonly now: () => number = Date.now,
  ) {}

  run(task: Task, signal?: AbortSignal): Promise<TaskExecutionSummary> {
    if (this.disposed) {
      return Promise.reject(taskRunnerDisposedError());
    }
    if (this.active) {
      return Promise.reject(
        new Error("Pointbreak is already running a selected VS Code task."),
      );
    }

    const active = new ActiveTaskRun(this.lifecycle, this.now, task, signal);
    this.active = active;
    return active.start().finally(() => {
      if (this.active === active) this.active = undefined;
    });
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.active?.dispose();
  }
}

class ActiveTaskRun {
  private readonly startedAt: number;
  private readonly completion: Promise<TaskExecutionSummary>;
  private readonly subscriptions: Disposable[] = [];
  private readonly pendingEvents: PendingEvent[] = [];
  private resolveCompletion!: (summary: TaskExecutionSummary) => void;
  private rejectCompletion!: (error: unknown) => void;
  private execution: TaskExecution | undefined;
  private exitCode: number | undefined;
  private processStarted = false;
  private cancellationRequested = false;
  private terminationRequested = false;
  private settled = false;

  constructor(
    private readonly lifecycle: TaskLifecycle,
    private readonly now: () => number,
    private readonly task: Task,
    private readonly signal?: AbortSignal,
  ) {
    this.startedAt = now();
    this.completion = new Promise((resolve, reject) => {
      this.resolveCompletion = resolve;
      this.rejectCompletion = reject;
    });
  }

  start(): Promise<TaskExecutionSummary> {
    this.subscriptions.push(
      this.lifecycle.onDidEndTask(({ execution }) =>
        this.receive({ kind: "task-end", execution }),
      ),
      this.lifecycle.onDidStartTaskProcess(({ execution }) =>
        this.receive({ kind: "process-start", execution }),
      ),
      this.lifecycle.onDidEndTaskProcess(({ execution, exitCode }) =>
        this.receive({ kind: "process-end", execution, exitCode }),
      ),
    );

    if (this.signal) {
      this.signal.addEventListener("abort", this.cancel, { once: true });
      if (this.signal.aborted) this.cancel();
    }
    void this.launch();
    return this.completion;
  }

  dispose(): void {
    if (this.settled) return;
    this.requestTermination();
    this.fail(taskRunnerDisposedError());
  }

  private readonly cancel = (): void => {
    if (this.settled || this.cancellationRequested) return;
    this.cancellationRequested = true;
    this.requestTermination();
  };

  private async launch(): Promise<void> {
    try {
      const execution = await this.lifecycle.executeTask(this.task);
      if (this.settled) {
        execution.terminate();
        return;
      }
      this.execution = execution;
      for (const event of this.pendingEvents.splice(0)) this.handle(event);
      if (!this.settled && this.cancellationRequested) {
        this.requestTermination();
      }
    } catch {
      this.finish("launch-error");
    }
  }

  private receive(event: PendingEvent): void {
    if (this.settled) return;
    if (!this.execution) {
      this.pendingEvents.push(event);
      return;
    }
    this.handle(event);
  }

  private handle(event: PendingEvent): void {
    if (event.execution !== this.execution) return;
    if (event.kind === "process-start") {
      this.processStarted = true;
      return;
    }
    if (event.kind === "process-end") {
      this.exitCode = event.exitCode;
      return;
    }
    this.finish();
  }

  private requestTermination(): void {
    if (!this.execution || this.terminationRequested) return;
    this.terminationRequested = true;
    this.execution.terminate();
  }

  private finish(
    terminationSource: TaskExecutionSummary["terminationSource"] = this
      .cancellationRequested
      ? "cancelled"
      : "task-end",
  ): void {
    if (this.settled) return;
    this.settled = true;
    const summary = {
      durationMs: Math.max(0, this.now() - this.startedAt),
      processStarted: this.processStarted,
      terminationSource,
      ...(this.exitCode === undefined ? {} : { exitCode: this.exitCode }),
    } satisfies TaskExecutionSummary;
    this.cleanup();
    this.resolveCompletion(summary);
  }

  private fail(error: unknown): void {
    if (this.settled) return;
    this.settled = true;
    this.cleanup();
    this.rejectCompletion(error);
  }

  private cleanup(): void {
    for (const subscription of this.subscriptions.splice(0)) {
      subscription.dispose();
    }
    this.signal?.removeEventListener("abort", this.cancel);
    this.pendingEvents.length = 0;
  }
}

function taskRunnerDisposedError(): Error {
  return new Error("Pointbreak task runner is no longer available.");
}
