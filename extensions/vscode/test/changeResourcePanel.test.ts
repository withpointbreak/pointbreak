import { describe, expect, it, vi } from "vitest";
import type { WorkspaceFolder } from "vscode";
import type { ChangeRevisionDoc } from "../src/changeProtocol";
import {
  ChangeResourcePanelManager,
  type ChangeRevisionPanelLocation,
  changeRevisionPanelKey,
  renderChangeRevisionHtml,
} from "../src/changeResourcePanel";
import { InspectClientError } from "../src/inspectClient";
import { workspaceFolder } from "./helpers/vscodeMock";

const vscodeMocks = vi.hoisted(() => ({
  createWebviewPanel: vi.fn(),
  showErrorMessage: vi.fn(),
}));

vi.mock("vscode", () => ({
  EventEmitter: class {
    readonly event = vi.fn();
    fire = vi.fn();
    dispose = vi.fn();
  },
  ViewColumn: { Active: -1 },
  window: {
    createWebviewPanel: vscodeMocks.createWebviewPanel,
    showErrorMessage: vscodeMocks.showErrorMessage,
  },
}));

describe("Change revision panel identity", () => {
  it("keys target, Change, Revision, artifact, and projection independently", () => {
    const original = location();
    const key = changeRevisionPanelKey(original, "sha256:resource");

    expect(
      changeRevisionPanelKey(
        {
          ...original,
          revision: {
            ...original.revision,
            objectArtifactContentHash: `sha256:${"b".repeat(64)}`,
          },
        },
        "sha256:resource",
      ),
    ).not.toBe(key);
    expect(
      changeRevisionPanelKey(
        {
          ...original,
          projectionStamp: `sha256:${"c".repeat(64)}`,
        },
        "sha256:resource",
      ),
    ).not.toBe(key);
    expect(changeRevisionPanelKey(original, "sha256:other")).not.toBe(key);
  });

  it("refuses typed migration state before creating a panel", async () => {
    vscodeMocks.createWebviewPanel.mockReset();
    vscodeMocks.showErrorMessage.mockReset();
    const client = {
      changeDetail: vi.fn(async () => {
        throw new InspectClientError("migration-in-progress");
      }),
      changeRevision: vi.fn(async () => document()),
    };
    const manager = new ChangeResourcePanelManager({
      ensure: vi.fn(async () => ({ client })),
    } as never);

    await manager.open(location());

    expect(vscodeMocks.createWebviewPanel).not.toHaveBeenCalled();
    expect(vscodeMocks.showErrorMessage).toHaveBeenCalledWith(
      "Pointbreak store migration is in progress; partial Change state is unavailable.",
    );
  });

  it("shows contextual currency, origin, availability, and exact captured bytes", () => {
    const html = renderChangeRevisionHtml(
      changeDetail(),
      document(),
      "location-key",
    );

    expect(html).toContain("change:sha256:one");
    expect(html).toContain("rev:sha256:origin");
    expect(html).toContain("superseded");
    expect(html).toContain("available");
    expect(html).toContain("declared_by");
    expect(html).toContain("&lt;captured&gt;");
    expect(html).not.toContain("<captured>");
  });
});

function location(): ChangeRevisionPanelLocation {
  return {
    resolution: {
      kind: "resolved",
      folder: workspaceFolder("/repo") as WorkspaceFolder,
      target: {
        key: "target",
        label: "repo",
        storeIdentity: "store:one",
        contextIdentity: "context:one",
      },
      emptyInventory: false,
    },
    changeId: "change:sha256:one",
    revision: {
      revisionId: "rev:sha256:one",
      objectArtifactContentHash: `sha256:${"a".repeat(64)}`,
    },
    projectionStamp: "sha256:projection",
  };
}

function changeDetail() {
  const revision = location().revision;
  return {
    schema: "pointbreak.review-change" as const,
    version: 1 as const,
    summary: {
      changeId: "change:sha256:one",
      declarationState: "authoritative",
      memberCount: 1,
      currentRevisionRefs: [revision],
      topology: "linear",
      lifecycle: "active",
      attentionSummary: "none",
      availabilitySummary: "available",
      diagnostics: [],
      projectionStamp: "sha256:projection",
    },
    relationClaims: [{ kind: "declared_by", eventId: "event:one" }],
    currentRevisionRefs: [revision],
    diagnostics: [],
    projectionStamp: "sha256:projection",
  };
}

function document(): ChangeRevisionDoc {
  const revision = location().revision;
  return {
    schema: "pointbreak.review-change-revision",
    version: 1,
    changeId: "change:sha256:one",
    revision,
    membershipSupport: [{ source: "declaration" }],
    revisionCurrency: "current",
    relationClassification: "replacement",
    exactRevisionDocument: {
      schema: "pointbreak.review-revision-resource",
      version: 1,
      resource: { revision, objectId: "obj:sha256:one" },
      projection: { includeBody: true },
      availability: "available",
      capturedDocumentHash: "sha256:document",
      capturedDocument: {
        schema: "pointbreak.review-revision",
        version: 3,
        revisionRef: revision,
        body: "<captured>",
      },
      diagnostics: [],
      cacheKey: "sha256:resource",
    },
    factPresentations: [
      {
        factId: "fact:one",
        family: "assessment",
        originRevision: {
          revisionId: "rev:sha256:origin",
          objectArtifactContentHash: "sha256:origin",
        },
        contextChangeId: "change:sha256:one",
        revisionCurrency: "superseded",
        familyState: "active",
        availability: "available",
      },
    ],
    associations: [],
    availability: "available",
    diagnostics: [],
    projectionStamp: "sha256:projection",
  };
}
