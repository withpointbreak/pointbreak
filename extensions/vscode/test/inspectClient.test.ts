import { describe, expect, it, vi } from "vitest";
import revisionFixture from "../../../src/cli/inspect/web/test/fixtures/revision.json";
import snapshotFixture from "../../../src/cli/inspect/web/test/fixtures/snapshot.json";
import { CHANGE_READER_DOCUMENTS } from "../src/changeProtocol";
import {
  type FetchFn,
  InspectClient,
  InspectClientError,
  revisionIsCurrent,
} from "../src/inspectClient";
import { VERSION_DOC } from "./fixtures";

const IDENTITY = {
  storeIdentity: "store:sha256:store",
  contextIdentity: "context:sha256:context",
};

describe("InspectClient", () => {
  it("validates the reader profile before version, identity, or semantic reads", async () => {
    const fetch = vi
      .fn<FetchFn>()
      .mockResolvedValueOnce(response(readerProfile()))
      .mockResolvedValueOnce(response(VERSION_DOC))
      .mockResolvedValueOnce(
        response({ schema: "pointbreak.inspect-identity", ...IDENTITY }),
      );
    const client = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      fetch,
    );

    await expect(client.verify(IDENTITY)).resolves.toBeUndefined();
    expect(fetch.mock.calls.map(([url]) => url.pathname)).toEqual([
      "/api/v2/profile",
      "/api/version",
      "/api/identity",
    ]);
  });

  it("uses exact Change and Revision identity on warm reads", async () => {
    const reference = {
      revisionId: "rev:sha256:one",
      objectArtifactContentHash: `sha256:${"a".repeat(64)}`,
    };
    const document = {
      schema: "pointbreak.review-change-revision",
      version: 1,
      changeId: "change:sha256:one",
      revision: reference,
      membershipSupport: [],
      revisionCurrency: "current",
      relationClassification: "current",
      exactRevisionDocument: {
        schema: "pointbreak.review-revision-resource",
        version: 1,
        resource: { revision: reference, objectId: "obj:sha256:one" },
        projection: { includeBody: true },
        availability: "available",
        capturedDocumentHash: `sha256:${"b".repeat(64)}`,
        capturedDocument: {
          schema: "pointbreak.review-revision",
          version: 3,
          revisionRef: reference,
        },
        diagnostics: [],
        cacheKey: `sha256:${"c".repeat(64)}`,
      },
      factPresentations: [],
      associations: [],
      availability: "available",
      diagnostics: [],
      projectionStamp: `sha256:${"d".repeat(64)}`,
    };
    const fetch = vi
      .fn<FetchFn>()
      .mockResolvedValueOnce(response(readerProfile()))
      .mockResolvedValueOnce(response(document));
    const client = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      fetch,
    );

    await expect(
      client.changeRevision("change:sha256:one", reference),
    ).resolves.toMatchObject({ revision: reference });
    expect(
      fetch.mock.calls.map(([url]) => `${url.pathname}${url.search}`),
    ).toEqual([
      "/api/v2/profile",
      `/api/v2/changes/change%3Asha256%3Aone/revisions/rev%3Asha256%3Aone?artifactHash=${encodeURIComponent(reference.objectArtifactContentHash)}`,
    ]);
  });

  it("decodes contextual Revision facts without optional content presentations", async () => {
    const reference = {
      revisionId: "rev:sha256:one",
      objectArtifactContentHash: `sha256:${"a".repeat(64)}`,
    };
    const { factContentPresentations: _, ...document } = changeRevisionDocument(
      reference,
      "sha256:projection",
    );
    const fetch = vi
      .fn<FetchFn>()
      .mockResolvedValueOnce(response(readerProfile()))
      .mockResolvedValueOnce(response(document));
    const client = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      fetch,
    );

    const detail = await client.changeRevision("change:sha256:one", reference);
    expect(detail).toMatchObject({
      factPresentations: [
        {
          contextChangeId: "change:sha256:one",
          factId: "observation:sha256:one",
        },
      ],
    });
    expect(detail.factContentPresentations).toBeUndefined();
  });

  it("refuses a warm Revision from another projection generation", async () => {
    const reference = {
      revisionId: "rev:sha256:one",
      objectArtifactContentHash: `sha256:${"a".repeat(64)}`,
    };
    const document = changeRevisionDocument(reference, "sha256:returned");
    const fetch = vi
      .fn<FetchFn>()
      .mockResolvedValueOnce(response(readerProfile()))
      .mockResolvedValueOnce(response(document));
    const client = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      fetch,
    );

    await expect(
      client.changeRevision("change:sha256:one", reference, "sha256:requested"),
    ).rejects.toMatchObject({ kind: "protocol" });
  });

  it("loads Change relation provenance only from the exact warm generation", async () => {
    const reference = {
      revisionId: "rev:sha256:one",
      objectArtifactContentHash: `sha256:${"a".repeat(64)}`,
    };
    const document = {
      schema: "pointbreak.review-change",
      version: 1,
      summary: {
        changeId: "change:sha256:one",
        declarationState: "authoritative",
        memberCount: 1,
        currentRevisionRefs: [reference],
        topology: "linear",
        lifecycle: "active",
        attentionSummary: "none",
        availabilitySummary: "available",
        diagnostics: [],
        projectionStamp: "sha256:projection",
      },
      relationClaims: [{ kind: "declared_by" }],
      currentRevisionRefs: [reference],
      diagnostics: [],
      projectionStamp: "sha256:projection",
    };
    const fetch = vi
      .fn<FetchFn>()
      .mockResolvedValueOnce(response(readerProfile()))
      .mockResolvedValueOnce(response(document));
    const client = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      fetch,
    );

    await expect(
      client.changeDetail("change:sha256:one", "sha256:projection"),
    ).resolves.toMatchObject({ relationClaims: [{ kind: "declared_by" }] });
    expect(fetch.mock.calls.map(([url]) => url.pathname)).toEqual([
      "/api/v2/profile",
      "/api/v2/changes/change%3Asha256%3Aone",
    ]);
  });

  it("polls both event and commit-graph freshness from the capable profile", async () => {
    const fetch = vi
      .fn<FetchFn>()
      .mockResolvedValueOnce(response(readerProfile(41, "sha256:graph-one")))
      .mockResolvedValueOnce(response(readerProfile(42, "sha256:graph-two")));
    const client = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      fetch,
    );

    await client.profile();
    await expect(client.freshness()).resolves.toEqual({
      schema: "pointbreak.inspect-freshness",
      version: 1,
      eventCount: 42,
      commitGraphStamp: "sha256:graph-two",
    });
    expect(fetch.mock.calls.map(([url]) => url.pathname)).toEqual([
      "/api/v2/profile",
      "/api/v2/profile",
    ]);
  });

  it("authenticates profile, version, then identity without exposing its credentials", async () => {
    const fetch = vi
      .fn<FetchFn>()
      .mockResolvedValueOnce(response(readerProfile()))
      .mockResolvedValueOnce(response(VERSION_DOC))
      .mockResolvedValueOnce(
        response({ schema: "pointbreak.inspect-identity", ...IDENTITY }),
      );
    const client = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      fetch,
    );

    await expect(client.verify(IDENTITY)).resolves.toBeUndefined();

    expect(fetch.mock.calls.map(([url]) => url.pathname)).toEqual([
      "/api/v2/profile",
      "/api/version",
      "/api/identity",
    ]);
    for (const [url, init] of fetch.mock.calls) {
      expect(url.hash).toBe("");
      expect(init.headers).toEqual({
        Host: "127.0.0.1:63831",
        Authorization: "Bearer secret-bearer",
      });
    }
    expect(JSON.stringify(client)).not.toContain("secret-bearer");
  });

  it.each([
    [401, "unauthorized"],
    [404, "protocol"],
    [500, "protocol"],
  ] as const)("classifies HTTP %i without echoing secrets", async (status, kind) => {
    const fetch = vi.fn<FetchFn>(async () => ({
      status,
      text: async () => "secret-bearer http://127.0.0.1:63831",
    }));
    const client = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      fetch,
    );

    const error = await client.verify(IDENTITY).catch((caught) => caught);
    expect(error).toBeInstanceOf(InspectClientError);
    expect(error.kind).toBe(kind);
    expect(error.message).not.toMatch(/secret-bearer|127\.0\.0\.1|63831/);
  });

  it("classifies transport failure and timeout as unreachable", async () => {
    const refused = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      vi.fn<FetchFn>(async () => {
        throw new Error("connect ECONNREFUSED 127.0.0.1:63831 secret-bearer");
      }),
    );
    await expect(refused.verifyVersion()).rejects.toMatchObject({
      kind: "unreachable",
      message: "Pointbreak Review could not be reached.",
    });

    const hanging = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      vi.fn<FetchFn>(() => new Promise(() => undefined)),
      5,
    );
    await expect(hanging.verifyVersion()).rejects.toMatchObject({
      kind: "unreachable",
    });
  });

  it("rejects version and identity mismatches with typed secret-free failures", async () => {
    const incompatible = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      vi.fn<FetchFn>(async () =>
        response({ ...VERSION_DOC, cliVersion: "9.0.0" }),
      ),
    );
    await expect(incompatible.verify(IDENTITY)).rejects.toMatchObject({
      kind: "version-incompatible",
    });

    const mismatchFetch = vi
      .fn<FetchFn>()
      .mockResolvedValueOnce(response(readerProfile()))
      .mockResolvedValueOnce(response(VERSION_DOC))
      .mockResolvedValueOnce(
        response({
          schema: "pointbreak.inspect-identity",
          storeIdentity: IDENTITY.storeIdentity,
          contextIdentity: "context:sha256:other",
        }),
      );
    const mismatch = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      mismatchFetch,
    );
    await expect(mismatch.verify(IDENTITY)).rejects.toMatchObject({
      kind: "identity-mismatch",
    });
  });

  it("verifies once, decodes typed warm documents, and keeps credentials internal", async () => {
    const fetch = vi
      .fn<FetchFn>()
      .mockResolvedValueOnce(response(VERSION_DOC))
      .mockResolvedValueOnce(response(revisionFixture))
      .mockResolvedValueOnce(response(snapshotFixture))
      .mockResolvedValueOnce(response(readerProfile(42)));
    const client = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      fetch,
    );

    const revision = await client.revision(revisionFixture.revision.id);
    expect(revision).toMatchObject({
      schema: "pointbreak.review-revision",
      version: 2,
    });
    const snapshot = await client.snapshot(
      snapshotFixture.snapshot.object_id,
      snapshotFixture.contentHash,
    );
    expect(snapshot).toMatchObject({
      schema: "pointbreak.review-snapshot",
      version: 1,
    });
    const freshnessDocument = await client.freshness();
    expect(freshnessDocument).toEqual({
      schema: "pointbreak.inspect-freshness",
      version: 1,
      eventCount: 42,
    });

    expect(
      fetch.mock.calls.map(([url]) => `${url.pathname}${url.search}`),
    ).toEqual([
      "/api/version",
      `/api/revisions/${encodeURIComponent(revisionFixture.revision.id)}`,
      `/api/snapshots/${encodeURIComponent(snapshotFixture.snapshot.object_id)}?contentHash=${encodeURIComponent(snapshotFixture.contentHash)}`,
      "/api/v2/profile",
    ]);
    expect(
      fetch.mock.calls.filter(([url]) => url.pathname === "/api/version"),
    ).toHaveLength(1);
    for (const [, init] of fetch.mock.calls) {
      expect(init.headers).toEqual({
        Host: "127.0.0.1:63831",
        Authorization: "Bearer secret-bearer",
      });
    }
    expect(
      JSON.stringify({
        revision,
        snapshot,
        freshness: freshnessDocument,
      }),
    ).not.toMatch(/secret-bearer|127\.0\.0\.1|63831/);
  });

  it.each([
    ["revision", { schema: "pointbreak.review-revision", version: 2 }],
    [
      "snapshot",
      {
        schema: "pointbreak.review-snapshot",
        version: 1,
        contentHash: "sha256:artifact",
        snapshot: { review_id: "review:default", object_id: "obj:one" },
      },
    ],
    ["freshness", { schema: "pointbreak.inspect-freshness", version: 1 }],
  ] as const)("fails closed when the %s document omits hard-core fields", async (kind, document) => {
    const fetch = vi.fn<FetchFn>();
    if (kind === "freshness") {
      fetch.mockResolvedValueOnce(
        response({ ...readerProfile(), authorityCursor: {} }),
      );
    } else {
      fetch
        .mockResolvedValueOnce(response(VERSION_DOC))
        .mockResolvedValueOnce(response(document));
    }
    const client = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      fetch,
    );

    const read =
      kind === "revision"
        ? client.revision("rev:one")
        : kind === "snapshot"
          ? client.snapshot("obj:one")
          : client.freshness();
    await expect(read).rejects.toMatchObject({ kind: "protocol" });
  });

  it("rejects invalid JSON and mismatched resource identities as protocol failures", async () => {
    const invalidJson = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      vi
        .fn<FetchFn>()
        .mockResolvedValueOnce(response(VERSION_DOC))
        .mockResolvedValueOnce({
          status: 200,
          text: async () => "{secret-bearer",
        }),
    );
    await expect(invalidJson.revision("rev:one")).rejects.toMatchObject({
      kind: "protocol",
      message: "Pointbreak Review returned an invalid response.",
    });

    const wrongRevision = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      vi
        .fn<FetchFn>()
        .mockResolvedValueOnce(response(VERSION_DOC))
        .mockResolvedValueOnce(response(revisionFixture)),
    );
    await expect(wrongRevision.revision("rev:other")).rejects.toMatchObject({
      kind: "protocol",
    });

    const wrongSnapshot = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      vi
        .fn<FetchFn>()
        .mockResolvedValueOnce(response(VERSION_DOC))
        .mockResolvedValueOnce(response(snapshotFixture)),
    );
    await expect(wrongSnapshot.snapshot("obj:other")).rejects.toMatchObject({
      kind: "protocol",
    });
  });

  it("refuses to turn a fragment-bearing capability into an HTTP base", () => {
    expect(
      () =>
        new InspectClient(
          "http://127.0.0.1:63831/#/timeline?token=secret-bearer",
          "secret-bearer",
          vi.fn<FetchFn>(),
        ),
    ).toThrow("Pointbreak Review returned an invalid response.");
  });
});

describe("revisionIsCurrent", () => {
  const revision = {
    schema: "pointbreak.review-revision" as const,
    version: 2 as const,
    revision: { id: "rev:one" },
    observations: [],
    inputRequests: [],
    assessments: [],
    diagnostics: [],
  };

  it("treats an isolated exact revision as current", () => {
    expect(revisionIsCurrent(revision, "rev:one")).toBe(true);
  });

  it("requires a supersession component to name the exact revision as a head", () => {
    expect(
      revisionIsCurrent(
        { ...revision, revisionSupersession: { heads: ["rev:two"] } },
        "rev:one",
      ),
    ).toBe(false);
    expect(
      revisionIsCurrent(
        { ...revision, revisionSupersession: { heads: ["rev:one"] } },
        "rev:one",
      ),
    ).toBe(true);
  });

  it("fails closed on malformed supersession data or identity mismatch", () => {
    expect(
      revisionIsCurrent(
        { ...revision, revisionSupersession: { heads: "rev:one" } },
        "rev:one",
      ),
    ).toBe(false);
    expect(revisionIsCurrent(revision, "rev:two")).toBe(false);
  });
});

function response(document: unknown) {
  return {
    status: 200,
    text: async () => JSON.stringify(document),
  };
}

function readerProfile(eventCount = 7, commitGraphStamp?: string) {
  return {
    schema: "pointbreak.inspect-reader-profile",
    version: 1,
    availability: "ready",
    minimumReaderProfile: "review_change_revision_v1",
    authorityCursor: { eventCount },
    commitGraphStamp,
    documents: { ...CHANGE_READER_DOCUMENTS },
  };
}

function changeRevisionDocument(
  reference: { revisionId: string; objectArtifactContentHash: string },
  projectionStamp: string,
) {
  return {
    schema: "pointbreak.review-change-revision",
    version: 1,
    changeId: "change:sha256:one",
    revision: reference,
    membershipSupport: [],
    revisionCurrency: "current",
    relationClassification: "current",
    exactRevisionDocument: {
      schema: "pointbreak.review-revision-resource",
      version: 1,
      resource: { revision: reference, objectId: "obj:sha256:one" },
      projection: { includeBody: true },
      availability: "available",
      capturedDocumentHash: `sha256:${"b".repeat(64)}`,
      capturedDocument: {
        schema: "pointbreak.review-revision",
        version: 3,
        revisionRef: reference,
      },
      diagnostics: [],
      cacheKey: `sha256:${"c".repeat(64)}`,
    },
    factPresentations: [
      {
        factId: "observation:sha256:one",
        family: "observation",
        originRevision: reference,
        contextChangeId: "change:sha256:one",
        revisionCurrency: "current",
        familyState: "current",
        availability: "available",
      },
    ],
    factContentPresentations: {
      "observation:sha256:one": {
        contentType: "text/markdown",
        bodyContentState: "present",
        content: {
          kind: "observation",
          title: "Readable finding",
          body: "Exact context",
        },
      },
    },
    associations: [],
    availability: "available",
    diagnostics: [],
    projectionStamp,
  };
}
