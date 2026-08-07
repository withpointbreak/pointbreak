import { describe, expect, it } from "vitest";
import {
  createChangeInspectorState,
  stageGeneration,
} from "../src/change-inspector-state";
import type {
  AttentionPage,
  ChangesPage,
  ReaderProfile,
} from "../src/change-protocol";

const profile: ReaderProfile = {
  schema: "pointbreak.inspect-reader-profile",
  version: 1,
  availability: "ready",
  authorityCursor: { eventCount: 1 },
  commitGraphStamp: "sha256:stamp",
  minimumReaderProfile: "review_change_revision_v1",
  documents: {},
};

function page(lens: "changes" | "attention", hash = "sha256:artifact") {
  return {
    schema:
      lens === "changes"
        ? "pointbreak.inspect-changes-page"
        : "pointbreak.inspect-attention",
    version: lens === "changes" ? 1 : 2,
    projectionStamp: "sha256:generation",
    next: null,
    changes: [
      {
        changeId: "change:sha256:one",
        topology: "initial",
        lifecycle: "in_progress",
        attentionSummary: "in_progress",
        availabilitySummary: "available",
        currentRevisionRefs: [
          {
            revisionId: "revision:sha256:two",
            objectArtifactContentHash: hash,
          },
        ],
        projectionStamp: "sha256:generation",
      },
    ],
  } as ChangesPage | AttentionPage;
}

describe("Change inspector state", () => {
  it("publishes atomically without inferring exact membership from a bounded page", () => {
    const state = createChangeInspectorState({
      kind: "revision",
      changeId: "change:sha256:one",
      revision: {
        revisionId: "revision:sha256:two",
        objectArtifactContentHash: "sha256:artifact",
      },
      query: {},
    });
    const first = stageGeneration(
      profile,
      page("changes") as ChangesPage,
      page("attention") as AttentionPage,
      profile,
    );
    state.publish(first);
    expect(state.snapshot().route.kind).toBe("revision");
    const replaced = stageGeneration(
      profile,
      page("changes", "sha256:changed") as ChangesPage,
      page("attention", "sha256:changed") as AttentionPage,
      profile,
    );
    state.publish(replaced);
    expect(state.snapshot().route.kind).toBe("revision");
    expect(state.snapshot().selected).toBeNull();
    expect(state.snapshot().diagnostic).toBeNull();
  });

  it("refuses to stage a mixed stamp or changed profile postflight", () => {
    expect(() =>
      stageGeneration(
        profile,
        page("changes") as ChangesPage,
        {
          ...page("attention"),
          projectionStamp: "sha256:other",
        } as AttentionPage,
        profile,
      ),
    ).toThrow("coherent");
    expect(() =>
      stageGeneration(
        profile,
        page("changes") as ChangesPage,
        page("attention") as AttentionPage,
        { ...profile, authorityCursor: { eventCount: 2 } },
      ),
    ).toThrow("changed during staging");
  });
});
