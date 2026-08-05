import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mountInspectorDom, resetDom } from "./support/dom";

const requiredDocuments = {
  "pointbreak.inspect-reader-profile": 1,
  "pointbreak.inspect-changes-page": 1,
  "pointbreak.review-change": 1,
  "pointbreak.review-change-revision": 1,
  "pointbreak.review-revision": 3,
  "pointbreak.review-revision-resource": 1,
  "pointbreak.review-association-comparison": 1,
  "pointbreak.review-revision-interdiff": 1,
  "pointbreak.inspect-attention": 2,
  "pointbreak.reader-upgrade-required": 1,
  "pointbreak.store-migration-required": 1,
  "pointbreak.store-migration-in-progress": 1,
};

function profile(availability: "ready" | "migration_required") {
  return {
    schema: "pointbreak.inspect-reader-profile",
    version: 1,
    availability,
    authorityCursor: { eventCount: 3 },
    minimumReaderProfile:
      availability === "ready" ? "review_change_revision_v1" : undefined,
    documents: { ...requiredDocuments },
  };
}

beforeEach(() => {
  vi.resetModules();
  mountInspectorDom();
});

afterEach(() => {
  vi.restoreAllMocks();
  resetDom();
});

describe("profile-first Change bootstrap", () => {
  it("refuses before semantic fetch or paint when migration is required", async () => {
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      requests.push(String(input));
      return new Response(JSON.stringify(profile("migration_required")));
    }) as typeof fetch;
    const { bootstrapChangeReader } = await import("../src/change-bootstrap");

    await bootstrapChangeReader({ poll: false });

    expect(requests).toEqual(["/api/v2/profile"]);
    expect(document.querySelector("#master")?.textContent).toContain(
      "migration required",
    );
    expect(document.querySelector("[data-change-id]")).toBeNull();
  });

  it("commits one coherent Change generation only after profile validation", async () => {
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile") {
        return new Response(JSON.stringify(profile("ready")));
      }
      if (path === "/api/v2/changes") {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-changes-page",
            version: 1,
            projectionStamp: "sha256:generation",
            diagnostics: [],
            changes: [
              {
                changeId: "change:sha256:one",
                topology: "parallel_current",
                lifecycle: "in_progress",
                attentionSummary: "in_progress",
                availabilitySummary: "available",
                projectionStamp: "sha256:generation",
                currentRevisionRefs: [
                  {
                    revisionId: "rev:sha256:a",
                    objectArtifactContentHash: `sha256:${"a".repeat(64)}`,
                  },
                  {
                    revisionId: "rev:sha256:b",
                    objectArtifactContentHash: `sha256:${"b".repeat(64)}`,
                  },
                ],
              },
              {
                changeId: "change:sha256:divergent",
                topology: "replacement_divergent",
                lifecycle: "in_progress",
                attentionSummary: "replacement_divergent",
                availabilitySummary: "available",
                projectionStamp: "sha256:generation",
                currentRevisionRefs: [],
              },
            ],
          }),
        );
      }
      if (path === "/api/v2/attention") {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-attention",
            version: 2,
            projectionStamp: "sha256:generation",
            changes: [],
          }),
        );
      }
      if (path.includes("/resource?artifactHash=")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.review-revision-resource",
            version: 1,
            resource: {
              revision: {
                revisionId: "rev:sha256:b",
                objectArtifactContentHash: `sha256:${"b".repeat(64)}`,
              },
              objectId: "obj:sha256:b",
            },
            availability: "available",
            capturedDocumentHash: "sha256:captured",
            capturedDocument: { summary: "captured" },
            diagnostics: [],
          }),
        );
      }
      if (path.includes("/interdiff/")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.review-revision-interdiff",
            version: 1,
            interdiff: {
              from: {
                revisionId: "rev:sha256:a",
                objectArtifactContentHash: `sha256:${"a".repeat(64)}`,
              },
              to: {
                revisionId: "rev:sha256:b",
                objectArtifactContentHash: `sha256:${"b".repeat(64)}`,
              },
            },
            availability: "unavailable",
            diagnostics: ["revision_interdiff_not_available"],
          }),
        );
      }
      if (path.includes("/api/v2/changes/change%3Asha256%3Aone/revisions/")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.review-change-revision",
            version: 1,
            changeId: "change:sha256:one",
            revision: {
              revisionId: "rev:sha256:b",
              objectArtifactContentHash: `sha256:${"b".repeat(64)}`,
            },
            revisionCurrency: "current",
            relationClassification: "parallel_current",
            availability: "available",
            factPresentations: [
              {
                factId: "observation:one",
                family: "observation",
                originRevision: {
                  revisionId: "rev:sha256:b",
                  objectArtifactContentHash: `sha256:${"b".repeat(64)}`,
                },
                revisionCurrency: "current",
                familyState: "current",
                availability: "available",
              },
            ],
            associations: [
              {
                state: "unknown",
                proofAvailability: "not_requested",
                comparison: {
                  revision: {
                    revisionId: "rev:sha256:b",
                    objectArtifactContentHash: `sha256:${"b".repeat(64)}`,
                  },
                  commitOid: "f00dbabe",
                },
              },
            ],
            diagnostics: [],
            projectionStamp: "sha256:generation",
          }),
        );
      }
      throw new Error(`unexpected request ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeReader } = await import("../src/change-bootstrap");

    await bootstrapChangeReader({ poll: false });

    expect(requests[0]).toBe("/api/v2/profile");
    expect(new Set(requests.slice(1))).toEqual(
      new Set(["/api/v2/changes", "/api/v2/attention"]),
    );
    expect(document.querySelector("[data-change-id]")?.textContent).toContain(
      "parallel current",
    );
    expect(document.querySelector("#master")?.textContent).toContain(
      "replacement divergent",
    );
    expect(document.querySelectorAll("[data-revision-id]")).toHaveLength(2);

    const revisions =
      document.querySelectorAll<HTMLButtonElement>("[data-revision-id]");
    revisions[1]?.click();
    await vi.waitFor(() => {
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "origin rev:sha256:b",
      );
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "f00dbabe · unknown · proof not requested",
      );
    });
    expect(requests.at(-1)).toContain(
      "/api/v2/changes/change%3Asha256%3Aone/revisions/rev%3Asha256%3Ab?artifactHash=sha256%3A",
    );

    const openResource = Array.from(
      document.querySelectorAll<HTMLButtonElement>("#detail-body button"),
    ).find((button) => button.textContent === "Open exact captured resource");
    openResource?.click();
    await vi.waitFor(() => {
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "document hash: sha256:captured",
      );
    });
    expect(requests.at(-1)).toContain("/resource?artifactHash=sha256%3A");

    const compare = Array.from(
      document.querySelectorAll<HTMLButtonElement>("#master button"),
    ).find((button) => button.textContent === "Compare exact Revisions");
    compare?.click();
    await vi.waitFor(() => {
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "revision_interdiff_not_available",
      );
    });
    expect(requests.at(-1)).toContain("/interdiff/");
  });

  it("refuses an incomplete registry before any semantic request", async () => {
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      requests.push(String(input));
      const incompatible = profile("ready");
      incompatible.documents["pointbreak.review-revision-interdiff"] = 0;
      return new Response(JSON.stringify(incompatible));
    }) as typeof fetch;
    const { bootstrapChangeReader } = await import("../src/change-bootstrap");

    await bootstrapChangeReader({ poll: false });

    expect(requests).toEqual(["/api/v2/profile"]);
    expect(document.querySelector("#master")?.textContent).toContain(
      "Reader refused",
    );
    expect(document.querySelector("[data-change-id]")).toBeNull();
  });

  it("does not paint a mixed list and attention generation", async () => {
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile") {
        return new Response(JSON.stringify(profile("ready")));
      }
      const schema =
        path === "/api/v2/changes"
          ? "pointbreak.inspect-changes-page"
          : "pointbreak.inspect-attention";
      return new Response(
        JSON.stringify({
          schema,
          version: path === "/api/v2/changes" ? 1 : 2,
          changes: [],
          diagnostics: [],
          projectionStamp: path,
        }),
      );
    }) as typeof fetch;
    const { bootstrapChangeReader } = await import("../src/change-bootstrap");

    await bootstrapChangeReader({ poll: false });

    expect(requests).toHaveLength(3);
    expect(document.querySelector("[data-change-id]")).toBeNull();
    expect(document.querySelector("#master")?.textContent).toContain(
      "Reader refused",
    );
  });
});
