import type { Task, WorkspaceFolder } from "vscode";

export interface TaskCandidate {
  readonly task: Task;
  readonly checkName: string;
  readonly blockedReason?: string;
}

export function deriveTaskCheckName(task: Task): string {
  const components = [task.definition.type, task.source, task.name]
    .map(escapeCheckNameComponent)
    .join(":");
  return `vscode-task:${components}`;
}

function escapeCheckNameComponent(component: string): string {
  return component.replace(/%/g, "%25").replace(/:/g, "%3A");
}

export function analyzeTaskCandidates(
  availableTasks: readonly Task[],
  targetFolder: WorkspaceFolder,
): TaskCandidate[] {
  const candidates = availableTasks
    .filter((task) => taskBelongsToFolder(task, targetFolder))
    .map((task) => ({ task, checkName: deriveTaskCheckName(task) }));
  const counts = new Map<string, number>();
  for (const candidate of candidates) {
    counts.set(candidate.checkName, (counts.get(candidate.checkName) ?? 0) + 1);
  }

  return candidates.map((candidate) => {
    if (counts.get(candidate.checkName) === 1) return candidate;
    return {
      ...candidate,
      blockedReason:
        `Multiple target-scoped tasks derive ${candidate.checkName}. ` +
        "Rename or disambiguate the public task type, source, or name before running either candidate.",
    };
  });
}

function taskBelongsToFolder(
  task: Task,
  targetFolder: WorkspaceFolder,
): boolean {
  return (
    typeof task.scope === "object" &&
    task.scope.uri.toString() === targetFolder.uri.toString()
  );
}
