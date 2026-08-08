import { describe, expect, it } from "vitest";
import {
  createChangeInspectorState,
  stageGeneration,
} from "../src/change-inspector-state";
import type {
  AttentionPage,
  ChangesPage,
  EventHistoryDocument,
  ReaderProfile,
} from "../src/change-protocol";
import { authorityCursor } from "./support/authority";

const AUTHORITY_HASH_FIELDS = [
  "journalRecordSetHash",
  "eventSetHash",
  "capabilitySetHash",
] as const;

const profile: ReaderProfile = {
  schema: "pointbreak.inspect-reader-profile",
  version: 1,
  availability: "ready",
  authorityCursor: authorityCursor(1),
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

function history(cursor = authorityCursor(1)): EventHistoryDocument {
  return {
    schema: "pointbreak.inspect-event-history",
    version: 1,
    authorityCursor: cursor,
    sourceChangeProjectionStamp: "sha256:generation",
    timelineProjectionStamp: "sha256:timeline",
    order: "desc",
    eventCount: cursor.eventCount,
    matchCount: 0,
    offset: 0,
    facets: {},
    completion: {
      eventTypes: [],
      trackIds: [],
      changeIds: [],
      revisionRefs: [],
      unresolvedRevisionIds: [],
    },
    diagnostics: [],
    queryNotices: [],
    entries: [],
  };
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
        { ...profile, authorityCursor: authorityCursor(2) },
      ),
    ).toThrow("changed during staging");

    const alternateHashes = authorityCursor(7);
    for (const field of AUTHORITY_HASH_FIELDS) {
      expect(() =>
        stageGeneration(
          profile,
          page("changes") as ChangesPage,
          page("attention") as AttentionPage,
          {
            ...profile,
            authorityCursor: {
              ...profile.authorityCursor,
              [field]: alternateHashes[field],
            },
          },
        ),
      ).toThrow("changed during staging");
    }
  });

  it("refuses history from a different authority generation", () => {
    expect(() =>
      stageGeneration(
        profile,
        page("changes") as ChangesPage,
        page("attention") as AttentionPage,
        profile,
        history(authorityCursor(2)),
      ),
    ).toThrow("changed during staging");

    const alternateHashes = authorityCursor(7);
    for (const field of AUTHORITY_HASH_FIELDS) {
      expect(() =>
        stageGeneration(
          profile,
          page("changes") as ChangesPage,
          page("attention") as AttentionPage,
          profile,
          history({
            ...profile.authorityCursor,
            [field]: alternateHashes[field],
          }),
        ),
      ).toThrow("changed during staging");
    }
  });
});
