import { describe, expect, it } from "vitest";
import {
  formatChangeInspectorRoute,
  parseChangeInspectorRoute,
} from "../src/change-inspector-router";

describe("Change inspector routes", () => {
  it("round trips the two lenses, Change detail, and an exact contextual Revision", () => {
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
});
