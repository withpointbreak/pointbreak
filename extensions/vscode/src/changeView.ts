import {
  type Disposable,
  type Event,
  EventEmitter,
  ThemeIcon,
  type TreeDataProvider,
  TreeItem,
  TreeItemCollapsibleState,
  type TreeView,
  type WorkspaceFolder,
} from "vscode";
import {
  type ChangeAttentionDoc,
  type ChangeListDoc,
  type ChangeSummaryV1,
  type ReaderProfileDoc,
  type RevisionRefV1,
  requireReadyReaderProfile,
} from "./changeProtocol";
import type { PointbreakCli } from "./cli";
import type {
  ResolvedTargetResolution,
  TargetResolution,
} from "./targetResolver";

export interface LoadedChangeDocuments {
  readonly profile: ReaderProfileDoc;
  readonly changes?: ChangeListDoc;
  readonly attention?: ChangeAttentionDoc;
}

interface NodeBase {
  readonly kind: string;
  readonly label: string;
  readonly targetKey?: string;
  readonly folder?: WorkspaceFolder;
}

export interface TargetNode extends NodeBase {
  readonly kind: "target";
  readonly targetKey: string;
  readonly folder: WorkspaceFolder;
  readonly children: ChangeTreeNode[];
}

export interface ChangesSectionNode extends NodeBase {
  readonly kind: "changes-section";
  readonly targetKey: string;
  readonly folder: WorkspaceFolder;
  readonly children: ChangeNode[];
}

export interface ChangeNode extends NodeBase {
  readonly kind: "change";
  readonly changeId: string;
  readonly targetKey: string;
  readonly folder: WorkspaceFolder;
  readonly description: string;
  readonly topology: string;
  readonly projectionStamp: string;
  readonly children: ChangeRevisionNode[];
}

export interface ChangeRevisionNode extends NodeBase {
  readonly kind: "change-revision";
  readonly changeId: string;
  readonly targetKey: string;
  readonly folder: WorkspaceFolder;
  readonly revision: RevisionRefV1;
  readonly projectionStamp: string;
  readonly command: "pointbreak.openChangeRevision";
}

export interface MigrationNode extends NodeBase {
  readonly kind: "migration";
  readonly targetKey: string;
  readonly folder: WorkspaceFolder;
  readonly availability: "migration_required" | "migration_in_progress";
}

export interface OnboardingNode extends NodeBase {
  readonly kind: "onboarding";
  readonly targetKey: string;
  readonly folder: WorkspaceFolder;
  readonly command: "pointbreak.capture";
}

export interface RepairNode extends NodeBase {
  readonly kind: "repair";
  readonly folder: WorkspaceFolder;
  readonly message: string;
}

export type ChangeTreeNode =
  | TargetNode
  | ChangesSectionNode
  | ChangeNode
  | ChangeRevisionNode
  | MigrationNode
  | OnboardingNode
  | RepairNode;

const activeProviders = new Set<ChangeTreeProvider>();

/**
 * Loads the cold Change reader as one profile-bound projection generation.
 * Semantic documents are never requested while migration is incomplete.
 */
export async function loadChangeDocuments(
  cli: PointbreakCli,
  repo: string,
): Promise<LoadedChangeDocuments> {
  const profile = await cli.changeProfile(repo);
  if (profile.availability !== "ready") {
    return { profile, changes: undefined, attention: undefined };
  }
  requireReadyReaderProfile(profile);
  const [changes, attention] = await Promise.all([
    cli.changeList(repo),
    cli.changeAttention(repo),
  ]);
  if (
    !changes.projectionStamp ||
    changes.projectionStamp !== attention.projectionStamp
  ) {
    throw new Error(
      "Pointbreak did not publish the Change list and attention view as one coherent generation.",
    );
  }
  return { profile, changes, attention };
}

export function deriveChangeTree(
  resolutions: TargetResolution[],
  loadedByTarget: ReadonlyMap<string, LoadedChangeDocuments>,
  errorsByTarget: ReadonlyMap<string, string>,
): ChangeTreeNode[] {
  const errors = resolutions
    .filter((resolution) => resolution.kind === "error")
    .map(repairNode);
  const sections = distinctResolvedTargets(resolutions).map((resolution) => ({
    resolution,
    children: deriveTargetChildren(
      resolution,
      loadedByTarget.get(resolution.target.key),
      errorsByTarget.get(resolution.target.key),
    ),
  }));

  if (sections.length === 1) {
    return [...sections[0].children, ...errors];
  }
  return [
    ...sections.map(
      ({ resolution, children }): TargetNode => ({
        kind: "target",
        label: resolution.folder.name,
        targetKey: resolution.target.key,
        folder: resolution.folder,
        children,
      }),
    ),
    ...errors,
  ];
}

export function changeAttentionCount(
  loadedByTarget: ReadonlyMap<string, LoadedChangeDocuments>,
): number {
  let count = 0;
  for (const loaded of loadedByTarget.values()) {
    count += loaded.attention?.changes.length ?? 0;
  }
  return count;
}

export async function refreshAfterWrite(): Promise<void> {
  await Promise.all([...activeProviders].map((provider) => provider.refresh()));
}

export class ChangeTreeProvider implements TreeDataProvider<ChangeTreeNode> {
  private readonly emitter = new EventEmitter<
    ChangeTreeNode | undefined | null
  >();
  readonly onDidChangeTreeData: Event<ChangeTreeNode | undefined | null> =
    this.emitter.event;
  private readonly visibilityEmitter = new EventEmitter<boolean>();
  readonly onDidChangeVisibility: Event<boolean> = this.visibilityEmitter.event;
  private readonly loadedByTarget = new Map<string, LoadedChangeDocuments>();
  private readonly loadErrors = new Map<string, string>();
  private treeView: TreeView<ChangeTreeNode> | undefined;
  private visible = false;

  constructor(
    private readonly cli: PointbreakCli,
    private readonly resolutions: TargetResolution[],
  ) {
    activeProviders.add(this);
  }

  attachTreeView(treeView: TreeView<ChangeTreeNode>): Disposable {
    this.treeView = treeView;
    this.updateBadge();
    const visibility = treeView.onDidChangeVisibility((event) => {
      void this.setVisible(event.visible);
    });
    void this.setVisible(treeView.visible);
    return visibility;
  }

  async setVisible(visible: boolean): Promise<void> {
    if (visible === this.visible) return;
    this.visible = visible;
    this.visibilityEmitter.fire(visible);
    if (visible) await this.refresh();
  }

  async refresh(): Promise<void> {
    await this.refreshResolutions(distinctResolvedTargets(this.resolutions));
  }

  async refreshTarget(targetKey: string, signal?: AbortSignal): Promise<void> {
    const resolution = distinctResolvedTargets(this.resolutions).find(
      (candidate) => candidate.target.key === targetKey,
    );
    if (resolution) await this.refreshResolutions([resolution], signal);
  }

  isVisible(): boolean {
    return this.visible;
  }

  getTreeItem(node: ChangeTreeNode): TreeItem {
    const collapsible =
      "children" in node
        ? TreeItemCollapsibleState.Expanded
        : TreeItemCollapsibleState.None;
    const item = new TreeItem(node.label, collapsible);
    if ("description" in node) item.description = node.description;
    if (node.kind === "repair" || node.kind === "migration") {
      item.iconPath = new ThemeIcon("warning");
      item.tooltip = node.kind === "repair" ? node.message : node.label;
    } else if (node.kind === "onboarding") {
      item.iconPath = new ThemeIcon("add");
    } else if (
      node.kind === "change" &&
      node.topology === "replacement_divergent"
    ) {
      item.iconPath = new ThemeIcon("warning");
    }
    if ("command" in node && node.command) {
      item.command = {
        command: node.command,
        title: node.label,
        arguments: [node],
      };
    }
    if (node.kind === "change") item.contextValue = "pointbreak.change";
    if (node.kind === "change-revision") {
      item.contextValue = "pointbreak.changeRevision";
    }
    return item;
  }

  getChildren(node?: ChangeTreeNode): ChangeTreeNode[] {
    if (node && "children" in node) return node.children;
    if (node) return [];
    return deriveChangeTree(
      this.resolutions,
      this.loadedByTarget,
      this.loadErrors,
    );
  }

  dispose(): void {
    this.visible = false;
    activeProviders.delete(this);
    this.emitter.dispose();
    this.visibilityEmitter.dispose();
  }

  private async refreshResolutions(
    resolutions: ResolvedTargetResolution[],
    signal?: AbortSignal,
  ): Promise<void> {
    await Promise.all(
      resolutions.map(async (resolution) => {
        const key = resolution.target.key;
        if (resolution.emptyInventory) {
          this.loadedByTarget.delete(key);
          this.loadErrors.delete(key);
          return;
        }
        try {
          const loaded = await loadChangeDocuments(
            this.cli,
            resolution.folder.uri.fsPath,
          );
          if (signal?.aborted) return;
          this.loadedByTarget.set(key, loaded);
          this.loadErrors.delete(key);
        } catch (error) {
          if (signal?.aborted) return;
          this.loadedByTarget.delete(key);
          this.loadErrors.set(key, errorMessage(error));
        }
      }),
    );
    if (signal?.aborted) return;
    this.updateBadge();
    this.emitter.fire(undefined);
  }

  private updateBadge(): void {
    if (!this.treeView) return;
    const value = changeAttentionCount(this.loadedByTarget);
    this.treeView.badge = {
      value,
      tooltip: `${value} Pointbreak Change${value === 1 ? "" : "s"} requiring attention`,
    };
  }
}

function distinctResolvedTargets(
  resolutions: TargetResolution[],
): ResolvedTargetResolution[] {
  const distinct = new Map<string, ResolvedTargetResolution>();
  for (const resolution of resolutions) {
    if (
      resolution.kind === "resolved" &&
      !distinct.has(resolution.target.key)
    ) {
      distinct.set(resolution.target.key, resolution);
    }
  }
  return [...distinct.values()];
}

function deriveTargetChildren(
  resolution: ResolvedTargetResolution,
  loaded: LoadedChangeDocuments | undefined,
  loadError: string | undefined,
): ChangeTreeNode[] {
  if (loadError) {
    return [
      {
        kind: "repair",
        label: resolution.folder.name,
        folder: resolution.folder,
        message: loadError,
      },
    ];
  }
  if (resolution.emptyInventory) {
    return [
      {
        kind: "onboarding",
        label: "Capture your current work",
        targetKey: resolution.target.key,
        folder: resolution.folder,
        command: "pointbreak.capture",
      },
    ];
  }
  if (!loaded) return [];
  if (loaded.profile.availability !== "ready") {
    return [
      {
        kind: "migration",
        label:
          loaded.profile.availability === "migration_required"
            ? "Store migration required"
            : "Store migration in progress",
        targetKey: resolution.target.key,
        folder: resolution.folder,
        availability: loaded.profile.availability,
      },
    ];
  }

  const projectionStamp = loaded.changes?.projectionStamp;
  if (
    !projectionStamp ||
    projectionStamp !== loaded.attention?.projectionStamp
  ) {
    return [
      {
        kind: "repair",
        label: resolution.folder.name,
        folder: resolution.folder,
        message:
          "Pointbreak Change documents do not share one projection generation.",
      },
    ];
  }
  return [
    {
      kind: "changes-section",
      label: "Changes",
      targetKey: resolution.target.key,
      folder: resolution.folder,
      children: loaded.changes.changes.map((change) =>
        changeNode(resolution, change),
      ),
    },
  ];
}

function changeNode(
  resolution: ResolvedTargetResolution,
  change: ChangeSummaryV1,
): ChangeNode {
  return {
    kind: "change",
    label: change.changeId,
    changeId: change.changeId,
    targetKey: resolution.target.key,
    folder: resolution.folder,
    description: `${humanize(change.topology)} · ${humanize(change.lifecycle)}`,
    topology: change.topology,
    projectionStamp: change.projectionStamp,
    children: change.currentRevisionRefs.map((revision) => ({
      kind: "change-revision",
      label: revision.revisionId,
      changeId: change.changeId,
      targetKey: resolution.target.key,
      folder: resolution.folder,
      revision,
      projectionStamp: change.projectionStamp,
      command: "pointbreak.openChangeRevision",
    })),
  };
}

function repairNode(
  resolution: TargetResolution & { kind: "error" },
): RepairNode {
  return {
    kind: "repair",
    label: resolution.folder.name,
    message: resolution.message,
    folder: resolution.folder,
  };
}

function humanize(value: string): string {
  return value.replaceAll("_", " ");
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
