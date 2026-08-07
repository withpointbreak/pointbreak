import { describe, expect, it, vi } from "vitest";
import type { WorkspaceFolder } from "vscode";
import { CHANGE_READER_DOCUMENTS } from "../src/changeProtocol";
import {
  ChangeTreeProvider,
  changeAttentionCount,
  deriveChangeTree,
  loadChangeDocuments,
} from "../src/changeView";
import type { PointbreakCli } from "../src/cli";
import type { TargetResolution } from "../src/targetResolver";
import { workspaceFolder } from "./helpers/vscodeMock";

const vscodeMocks = vi.hoisted(() => ({
  EventEmitter: class<T> {
    readonly event = vi.fn();
    fire = vi.fn<(value?: T) => void>();
    dispose = vi.fn();
  },
  ThemeIcon: class {
    constructor(readonly id: string) {}
  },
  TreeItem: class {
    contextValue?: string;
    description?: string;
    iconPath?: unknown;
    tooltip?: string;
    command?: unknown;
    constructor(
      readonly label: string,
      readonly collapsibleState?: number,
    ) {}
  },
}));

vi.mock("vscode", () => ({
  EventEmitter: vscodeMocks.EventEmitter,
  ThemeIcon: vscodeMocks.ThemeIcon,
  TreeItem: vscodeMocks.TreeItem,
  TreeItemCollapsibleState: { None: 0, Collapsed: 1, Expanded: 2 },
}));

describe("loadChangeDocuments", () => {
  it("stops after the typed profile while migration is incomplete", async () => {
    const cli = {
      changeProfile: vi.fn(async () => profile("migration_in_progress")),
      changeList: vi.fn(),
      changeAttention: vi.fn(),
    } as unknown as PointbreakCli;

    await expect(loadChangeDocuments(cli, "/repo")).resolves.toMatchObject({
      profile: { availability: "migration_in_progress" },
      changes: undefined,
      attention: undefined,
    });
    expect(cli.changeList).not.toHaveBeenCalled();
    expect(cli.changeAttention).not.toHaveBeenCalled();
  });

  it("publishes list and attention only as one projection generation", async () => {
    const cli = {
      changeProfile: vi.fn(async () => profile("ready")),
      changeList: vi.fn(async () => changes("sha256:one")),
      changeAttention: vi.fn(async () => attention("sha256:two")),
    } as unknown as PointbreakCli;

    await expect(loadChangeDocuments(cli, "/repo")).rejects.toThrow(
      /coherent generation/i,
    );
  });
});

describe("deriveChangeTree", () => {
  it("preserves the two-Change server change_id_asc order without re-sorting it", () => {
    const serverOrder = [
      changeSummary("sha256:one", "change:sha256:alpha", "in_progress"),
      changeSummary("sha256:one", "change:sha256:omega", "conflicted"),
    ];
    const loaded = new Map([
      [
        "target",
        {
          profile: profile("ready"),
          changes: changes("sha256:one", serverOrder),
          attention: attention("sha256:one", serverOrder),
        },
      ],
    ]);
    const sort = vi.spyOn(Array.prototype, "sort");

    try {
      const [section] = deriveChangeTree([resolved()], loaded, new Map());
      if (section.kind !== "changes-section")
        throw new Error("missing Changes");

      expect(section.children.map((change) => change.changeId)).toEqual(
        serverOrder.map((change) => change.changeId),
      );
      expect(sort).not.toHaveBeenCalled();
    } finally {
      sort.mockRestore();
    }
  });

  it("renders Change identity, topology, and every exact current Revision", () => {
    const loaded = new Map([
      [
        "target",
        {
          profile: profile("ready"),
          changes: changes("sha256:one"),
          attention: attention("sha256:one"),
        },
      ],
    ]);

    const nodes = deriveChangeTree([resolved()], loaded, new Map());

    expect(nodes).toMatchObject([
      {
        kind: "changes-section",
        children: [
          {
            kind: "change",
            changeId: "change:sha256:one",
            description: "replacement divergent · conflicted",
            children: [
              {
                kind: "change-revision",
                changeId: "change:sha256:one",
                revision: {
                  revisionId: "rev:sha256:one",
                  objectArtifactContentHash: `sha256:${"a".repeat(64)}`,
                },
                command: "pointbreak.openChangeRevision",
              },
              {
                kind: "change-revision",
                revision: { revisionId: "rev:sha256:two" },
              },
            ],
          },
        ],
      },
    ]);
    expect(changeAttentionCount(loaded)).toBe(1);
  });

  it("shows typed migration state without any semantic tree", () => {
    const nodes = deriveChangeTree(
      [resolved()],
      new Map([
        [
          "target",
          {
            profile: profile("migration_required"),
            changes: undefined,
            attention: undefined,
          },
        ],
      ]),
      new Map(),
    );

    expect(nodes).toMatchObject([
      {
        kind: "migration",
        availability: "migration_required",
      },
    ]);
  });

  it("emits only the Change context values contributed by the manifest", () => {
    const loaded = new Map([
      [
        "target",
        {
          profile: profile("ready"),
          changes: changes("sha256:one"),
          attention: attention("sha256:one"),
        },
      ],
    ]);
    const [section] = deriveChangeTree([resolved()], loaded, new Map());
    if (section.kind !== "changes-section") throw new Error("missing Changes");
    const change = section.children[0];
    const provider = new ChangeTreeProvider({} as PointbreakCli, [resolved()]);

    expect(provider.getTreeItem(change).contextValue).toBe("pointbreak.change");
    expect(provider.getTreeItem(change.children[0]).contextValue).toBe(
      "pointbreak.changeRevision",
    );
    expect(provider.getTreeItem(change).iconPath).toEqual(
      expect.objectContaining({ id: "warning" }),
    );
    provider.dispose();
  });
});

function profile(
  availability: "ready" | "migration_required" | "migration_in_progress",
) {
  return {
    schema: "pointbreak.inspect-reader-profile" as const,
    version: 1 as const,
    availability,
    minimumReaderProfile:
      availability === "ready" ? "review_change_revision_v1" : undefined,
    authorityCursor: { eventCount: 2 },
    documents: { ...CHANGE_READER_DOCUMENTS },
  };
}

function changeSummary(
  projectionStamp: string,
  changeId = "change:sha256:one",
  lifecycle = "conflicted",
) {
  return {
    changeId,
    declarationState: "authoritative",
    memberCount: 2,
    currentRevisionRefs: [
      {
        revisionId: "rev:sha256:one",
        objectArtifactContentHash: `sha256:${"a".repeat(64)}`,
      },
      {
        revisionId: "rev:sha256:two",
        objectArtifactContentHash: `sha256:${"b".repeat(64)}`,
      },
    ],
    topology: "replacement_divergent",
    lifecycle,
    attentionSummary: "conflicted",
    availabilitySummary: "available",
    diagnostics: [],
    projectionStamp,
  };
}

function changes(
  projectionStamp: string,
  summaries = [changeSummary(projectionStamp)],
) {
  return {
    schema: "pointbreak.review-change-list" as const,
    version: 1 as const,
    changes: summaries,
    diagnostics: [],
    projectionStamp,
  };
}

function attention(
  projectionStamp: string,
  summaries = changes(projectionStamp).changes,
) {
  return {
    schema: "pointbreak.attention-list" as const,
    version: 2 as const,
    changes: summaries,
    projectionStamp,
  };
}

function resolved(): TargetResolution {
  return {
    kind: "resolved",
    folder: workspaceFolder("/repo") as WorkspaceFolder,
    target: {
      key: "target",
      label: "repo",
      storeIdentity: "store:one",
      contextIdentity: "context:one",
    },
    emptyInventory: false,
  };
}
