import { describe, expect, it } from "vitest";
import {
  eventAnnotatedDiffRoute,
  formatChangeInspectorRoute,
  parseChangeInspectorRoute,
  queryForExactNavigation,
  showChangeInTimelineRoute,
  showRevisionInTimelineRoute,
  timelineEventRoute,
} from "../src/change-inspector-router";

describe("Change inspector routes", () => {
  it("uses Timeline for an empty hash and round trips its bounded query", () => {
    expect(parseChangeInspectorRoute("")).toEqual({
      kind: "timeline",
      historyQuery: {},
    });
    const timeline = parseChangeInspectorRoute(
      "#/timeline?q=release&type=validation_check_recorded&order=asc&limit=25",
    );
    expect(timeline).toEqual({
      kind: "timeline",
      historyQuery: {
        q: "release",
        type: "validation_check_recorded",
        order: "asc",
        limit: 25,
      },
    });
    if (timeline.kind === "invalid") throw new Error(timeline.message);
    expect(formatChangeInspectorRoute(timeline)).toBe(
      "#/timeline?limit=25&q=release&type=validation_check_recorded&order=asc",
    );
  });

  it("keeps Timeline continuation position exclusive and exact revision-scoped", () => {
    expect(
      parseChangeInspectorRoute(
        "#/timeline?at=evt%3Asha256%3Aone&after=opaque",
      ),
    ).toEqual({
      kind: "invalid",
      message: "Timeline at and after cannot be combined.",
    });
    expect(
      parseChangeInspectorRoute("#/timeline?revision=rev%3Asha256%3Aone"),
    ).toEqual({
      kind: "invalid",
      message: "Timeline revision requires artifactHash.",
    });
  });

  it("keeps partial identity queries on the Timeline instead of constructing exact routes", () => {
    const route = parseChangeInspectorRoute(
      "#/timeline?q=revision%3A01234567+rev%3A01234567+change%3Afedcba98",
    );
    expect(route).toEqual({
      kind: "timeline",
      historyQuery: {
        q: "revision:01234567 rev:01234567 change:fedcba98",
      },
    });
    if (route.kind !== "timeline") throw new Error("expected Timeline route");
    expect(formatChangeInspectorRoute(route)).toBe(
      "#/timeline?q=revision%3A01234567+rev%3A01234567+change%3Afedcba98",
    );

    // Only the dedicated selector fields can carry an exact Revision identity;
    // a partial q clause must never synthesize an artifact hash or a detail route.
    expect(route.kind).toBe("timeline");
    expect(route.historyQuery.revision).toBeUndefined();
    expect(route.historyQuery.artifactHash).toBeUndefined();
  });

  it("round trips the two Change lenses, Change detail, and an exact contextual Revision", () => {
    expect(
      parseChangeInspectorRoute("#/changes?q=ready&lifecycle=in_progress"),
    ).toEqual({
      kind: "lens",
      lens: "changes",
      query: { q: "ready", lifecycle: "in_progress" },
    });
    expect(parseChangeInspectorRoute("#/attention")).toEqual({
      kind: "lens",
      lens: "attention",
      query: {},
    });
    const exact = {
      kind: "revision" as const,
      changeId: "change:sha256:one",
      revision: {
        revisionId: "revision:sha256:two",
        objectArtifactContentHash: "sha256:artifact",
      },
      query: { q: "release" },
    };
    expect(
      parseChangeInspectorRoute(formatChangeInspectorRoute(exact)),
    ).toEqual(exact);
  });

  it("round trips exact captured resources, associations, and ordered interdiffs", () => {
    const revision = {
      revisionId: "revision:sha256:two",
      objectArtifactContentHash: "sha256:artifact-two",
    };
    const resource = {
      kind: "resource" as const,
      changeId: "change:sha256:one",
      revision,
      query: {},
      focus: { factId: "obs:sha256:one", filePath: "src/lib.rs" },
    };
    const association = { ...resource, kind: "association" as const };
    const interdiff = {
      kind: "interdiff" as const,
      changeId: "change:sha256:one",
      from: revision,
      to: {
        revisionId: "revision:sha256:three",
        objectArtifactContentHash: "sha256:artifact-three",
      },
      query: { q: "exact" },
    };
    expect(
      parseChangeInspectorRoute(formatChangeInspectorRoute(resource)),
    ).toEqual(resource);
    expect(
      parseChangeInspectorRoute(formatChangeInspectorRoute(association)),
    ).toEqual(association);
    expect(
      parseChangeInspectorRoute(formatChangeInspectorRoute(interdiff)),
    ).toEqual(interdiff);
    expect(formatChangeInspectorRoute(interdiff)).not.toEqual(
      formatChangeInspectorRoute({
        ...interdiff,
        from: interdiff.to,
        to: interdiff.from,
      }),
    );
  });

  it("makes the annotated diff a contextual exact Revision route", () => {
    const diff = {
      kind: "diff" as const,
      changeId: "change:sha256:one",
      revision: {
        revisionId: "revision:sha256:two",
        objectArtifactContentHash: "sha256:artifact-two",
      },
      query: {},
      focus: {
        filePath: "src/lib.rs",
        factId: "obs:sha256:one",
        fileQuery: "path:lib has:facts",
      },
    };
    const formatted = formatChangeInspectorRoute(diff);
    expect(formatted).toBe(
      "#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Atwo/diff?artifactHash=sha256%3Aartifact-two&fact=obs%3Asha256%3Aone&file=src%2Flib.rs&fq=path%3Alib+has%3Afacts",
    );
    expect(parseChangeInspectorRoute(formatted)).toEqual(diff);
    expect(
      parseChangeInspectorRoute(
        "#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Atwo/resource?artifactHash=sha256%3Aartifact-two&fq=path%3Alib",
      ),
    ).toEqual({
      kind: "invalid",
      message:
        "Exact route focus requires at most one non-empty fact and file.",
    });
  });

  it("constructs an event diff route only from one explicit fully exact context", () => {
    const context = {
      changeIds: ["change:sha256:one"],
      revisionRefs: [
        {
          revisionId: "revision:sha256:two",
          objectArtifactContentHash: "sha256:artifact-two",
        },
      ],
      unresolvedRevisionIds: [],
    };
    expect(eventAnnotatedDiffRoute(context)).toEqual({
      kind: "diff",
      changeId: "change:sha256:one",
      revision: context.revisionRefs[0],
      query: {},
    });
    for (const refused of [
      { ...context, changeIds: [] },
      { ...context, revisionRefs: [] },
      {
        ...context,
        unresolvedRevisionIds: ["revision:sha256:unresolved"],
      },
      {
        ...context,
        changeIds: ["change:sha256:one", "change:sha256:two"],
      },
      {
        ...context,
        revisionRefs: [
          ...context.revisionRefs,
          {
            revisionId: "revision:sha256:three",
            objectArtifactContentHash: "sha256:artifact-three",
          },
        ],
      },
    ]) {
      expect(eventAnnotatedDiffRoute(refused)).toBeNull();
    }
  });

  it("constructs fresh canonical Timeline scopes for a Change and exact Revision", () => {
    const revision = {
      revisionId: "revision:sha256:two",
      objectArtifactContentHash: "sha256:artifact-two",
    };
    expect(showChangeInTimelineRoute("change:sha256:one")).toEqual({
      kind: "timeline",
      historyQuery: { change: "change:sha256:one" },
    });
    expect(showRevisionInTimelineRoute("change:sha256:one", revision)).toEqual({
      kind: "timeline",
      historyQuery: {
        change: "change:sha256:one",
        revision: "revision:sha256:two",
        artifactHash: "sha256:artifact-two",
      },
    });
  });

  it("constructs an event route without competing Timeline position", () => {
    expect(
      timelineEventRoute("evt:sha256:one", {
        q: "review",
        type: "review_note_imported",
        track: "reviewer",
        change: "change:sha256:one",
        revision: "revision:sha256:one",
        artifactHash: "sha256:artifact-one",
        limit: 25,
        order: "asc",
        after: "opaque-continuation",
        at: "evt:sha256:locator",
      }),
    ).toEqual({
      kind: "event",
      eventId: "evt:sha256:one",
      historyQuery: {
        q: "review",
        type: "review_note_imported",
        track: "reviewer",
        change: "change:sha256:one",
        revision: "revision:sha256:one",
        artifactHash: "sha256:artifact-one",
        limit: 25,
        order: "asc",
      },
      query: {},
    });
  });

  it("round trips an exact event route and rejects a competing anchor", () => {
    const route = parseChangeInspectorRoute(
      "#/timeline/events/evt%3Asha256%3Aone?q=release&type=validation_check_recorded",
    );
    expect(route).toEqual({
      kind: "event",
      eventId: "evt:sha256:one",
      historyQuery: { q: "release", type: "validation_check_recorded" },
      query: {},
    });
    if (route.kind === "invalid") throw new Error(route.message);
    expect(formatChangeInspectorRoute(route)).toBe(
      "#/timeline/events/evt%3Asha256%3Aone?q=release&type=validation_check_recorded",
    );
    expect(
      parseChangeInspectorRoute(
        "#/timeline/events/evt%3Asha256%3Aone?at=evt%3Asha256%3Atwo",
      ),
    ).toEqual({
      kind: "invalid",
      message: "Event routes select their anchor from the event ID.",
    });
    expect(parseChangeInspectorRoute("#/timeline/events")).toEqual({
      kind: "invalid",
      message: "Unknown Change Inspector route.",
    });
  });

  it("rejects a Revision route with no artifact hash instead of retargeting it", () => {
    expect(
      parseChangeInspectorRoute(
        "#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Atwo",
      ),
    ).toEqual({
      kind: "invalid",
      message: "Exact Revision routes require artifactHash.",
    });
  });

  it("rejects unrecognized, duplicate, and context-invalid route query members", () => {
    expect(parseChangeInspectorRoute("#/changes?unknown=value")).toEqual({
      kind: "invalid",
      message: "Unknown unknown route query.",
    });
    expect(parseChangeInspectorRoute("#/changes?q=one&q=two")).toEqual({
      kind: "invalid",
      message: "Duplicate q route query.",
    });
    expect(parseChangeInspectorRoute("#/changes?order=activity")).toEqual({
      kind: "invalid",
      message: "Invalid order route query.",
    });
    expect(
      parseChangeInspectorRoute("#/attention?artifactHash=sha256:one"),
    ).toEqual({
      kind: "invalid",
      message: "artifactHash is only valid on an exact Revision route.",
    });
    expect(
      parseChangeInspectorRoute(
        "#/changes/change%3Asha256%3Aone?artifactHash=sha256:one",
      ),
    ).toEqual({
      kind: "invalid",
      message: "artifactHash is only valid on an exact Revision route.",
    });
    expect(
      parseChangeInspectorRoute(
        "#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Atwo?artifactHash=sha256:one&artifactHash=sha256:two",
      ),
    ).toEqual({
      kind: "invalid",
      message: "Exact Revision routes require exactly one artifactHash.",
    });
    expect(
      parseChangeInspectorRoute(
        "#/changes/change%3Asha256%3Aone/interdiff/revision%3Asha256%3Aone/revision%3Asha256%3Atwo?fromArtifactHash=sha256:one",
      ),
    ).toEqual({
      kind: "invalid",
      message:
        "Interdiff routes require exactly one artifact hash for each endpoint.",
    });
    for (const hash of ["#/changes?q=%ZZ", "#/changes?q=%E0%A4%A"]) {
      expect(parseChangeInspectorRoute(hash)).toEqual({
        kind: "invalid",
        message: "Malformed route query encoding.",
      });
    }
  });

  it("preserves server query and page input in the fragment", () => {
    const route = parseChangeInspectorRoute(
      "#/attention?after=opaque&q=release&topology=parallel_current&lifecycle=in_progress&attention=conflicted&availability=incomplete&limit=20",
    );
    expect(route).toMatchObject({
      kind: "lens",
      lens: "attention",
      query: {
        after: "opaque",
        q: "release",
        topology: "parallel_current",
        lifecycle: "in_progress",
        attention: "conflicted",
        availability: "incomplete",
        limit: 20,
      },
    });
    expect(
      formatChangeInspectorRoute(
        route as Exclude<typeof route, { kind: "invalid" }>,
      ),
    ).toContain("after=opaque");
  });

  it("retains filters but drops a lens-bound Attention continuation before exact navigation", () => {
    const attention = parseChangeInspectorRoute(
      "#/attention?q=release&after=attention-page&limit=20&order=change_id_asc",
    );
    if (attention.kind === "invalid") throw new Error(attention.message);
    expect(queryForExactNavigation(attention)).toEqual({
      q: "release",
      limit: 20,
      order: "change_id_asc",
    });

    const changes = parseChangeInspectorRoute(
      "#/changes?after=changes-page&limit=20&order=change_id_asc",
    );
    if (changes.kind === "invalid" || changes.kind === "timeline")
      throw new Error("expected Changes route");
    expect(queryForExactNavigation(changes)).toEqual(changes.query);
  });
});
