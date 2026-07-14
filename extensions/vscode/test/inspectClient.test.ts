import { describe, expect, it, vi } from "vitest";
import revisionFixture from "../../../src/cli/inspect/web/test/fixtures/revision.json";
import snapshotFixture from "../../../src/cli/inspect/web/test/fixtures/snapshot.json";
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
  it("authenticates version then identity without exposing its credentials", async () => {
    const fetch = vi
      .fn<FetchFn>()
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
    const freshness = {
      schema: "pointbreak.inspect-freshness",
      version: 1,
      eventCount: 42,
      commitGraphStamp: "sha256:graph",
    };
    const fetch = vi
      .fn<FetchFn>()
      .mockResolvedValueOnce(response(VERSION_DOC))
      .mockResolvedValueOnce(response(revisionFixture))
      .mockResolvedValueOnce(response(snapshotFixture))
      .mockResolvedValueOnce(response(freshness));
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
    expect(freshnessDocument).toEqual(freshness);

    expect(
      fetch.mock.calls.map(([url]) => `${url.pathname}${url.search}`),
    ).toEqual([
      "/api/version",
      `/api/revisions/${encodeURIComponent(revisionFixture.revision.id)}`,
      `/api/snapshots/${encodeURIComponent(snapshotFixture.snapshot.object_id)}?contentHash=${encodeURIComponent(snapshotFixture.contentHash)}`,
      "/api/freshness",
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
    const fetch = vi
      .fn<FetchFn>()
      .mockResolvedValueOnce(response(VERSION_DOC))
      .mockResolvedValueOnce(response(document));
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
