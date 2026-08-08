import { afterEach, describe, expect, it, vi } from "vitest";
import { loadChangeInspectorReading } from "../src/change-inspector-reading";
import type { ChangeInspectorRoute } from "../src/change-inspector-router";

const changeId = "change:sha256:reading";
const revision = {
  revisionId: "rev:sha256:reading",
  objectArtifactContentHash: "sha256:artifact-reading",
};
const otherRevision = {
  revisionId: "rev:sha256:other",
  objectArtifactContentHash: "sha256:artifact-other",
};
const stamp = "sha256:generation";

function resource(reference = revision) {
  return {
    schema: "pointbreak.review-revision-resource",
    version: 1,
    projectionStamp: stamp,
    resource: { revision: reference, objectId: "obj:sha256:reading" },
    projection: { includeBody: true },
    availability: "available",
    capturedDocumentHash: "sha256:document",
    capturedDocument: {
      schema: "pointbreak.review-snapshot",
      version: 1,
      contentHash: reference.objectArtifactContentHash,
      snapshot: {
        review_id: "review:sha256:reading",
        object_id: "obj:sha256:reading",
        files: [],
      },
    },
    diagnostics: [],
    cacheKey: "sha256:resource",
  };
}

function contextualDetail() {
  return {
    schema: "pointbreak.review-change-revision",
    version: 1,
    changeId,
    revision,
    membershipSupport: [],
    revisionCurrency: "current",
    relationClassification: "current",
    availability: "available",
    exactRevisionDocument: resource(),
    factPresentations: [],
    factPorts: [],
    associations: [],
    diagnostics: [],
    projectionStamp: stamp,
  };
}

function exactRoute(): Extract<ChangeInspectorRoute, { kind: "revision" }> {
  return { kind: "revision", changeId, revision, query: {} };
}

afterEach(() => vi.restoreAllMocks());

describe("exact Inspector reading controller", () => {
  it("accepts contextual detail only when its embedded resource shares the exact Revision and generation", async () => {
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      expect(String(input)).toBe(
        "/api/v2/changes/change%3Asha256%3Areading/revisions/rev%3Asha256%3Areading?artifactHash=sha256%3Aartifact-reading",
      );
      return new Response(JSON.stringify(contextualDetail()));
    }) as typeof fetch;

    const reading = await loadChangeInspectorReading(exactRoute(), stamp);
    expect(reading.kind).toBe("revision");
  });

  it("refuses a captured resource returned for another exact Revision", async () => {
    const route: Extract<ChangeInspectorRoute, { kind: "resource" }> = {
      kind: "resource",
      changeId,
      revision,
      query: {},
    };
    globalThis.fetch = vi.fn(
      async () => new Response(JSON.stringify(resource(otherRevision))),
    ) as typeof fetch;

    await expect(loadChangeInspectorReading(route, stamp)).rejects.toThrow(
      "captured resource does not match its exact Revision route",
    );
  });

  it("refuses an interdiff whose ordered endpoints are reversed", async () => {
    const route: Extract<ChangeInspectorRoute, { kind: "interdiff" }> = {
      kind: "interdiff",
      changeId,
      from: revision,
      to: otherRevision,
      query: {},
    };
    globalThis.fetch = vi.fn(
      async () =>
        new Response(
          JSON.stringify({
            schema: "pointbreak.review-revision-interdiff",
            version: 1,
            projectionStamp: stamp,
            interdiff: {
              from: otherRevision,
              to: revision,
              algorithmVersion: "rows-v1",
              scope: [],
            },
            availability: "unavailable",
            diagnostics: [],
            cacheKey: "sha256:interdiff",
          }),
        ),
    ) as typeof fetch;

    await expect(loadChangeInspectorReading(route, stamp)).rejects.toThrow(
      "ordered Revision interdiff does not match its exact route",
    );
  });
});
