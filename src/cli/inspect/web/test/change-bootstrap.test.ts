import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CHANGE_READER_DOCUMENTS } from "../src/change-protocol";
import { mountInspectorDom, resetDom } from "./support/dom";

function profile(availability: "ready" | "migration_required") {
  return {
    schema: "pointbreak.inspect-reader-profile",
    version: 1,
    availability,
    authorityCursor: { eventCount: 3 },
    commitGraphStamp:
      availability === "ready" ? "sha256:commit-graph" : undefined,
    minimumReaderProfile:
      availability === "ready" ? "review_change_revision_v1" : undefined,
    documents: { ...CHANGE_READER_DOCUMENTS },
  };
}

beforeEach(() => {
  vi.resetModules();
  mountInspectorDom();
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
  resetDom();
});

describe("profile-first Change bootstrap", () => {
  it("stages bounded Changes and Attention before a matching profile postflight", async () => {
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile") {
        return new Response(JSON.stringify(profile("ready")));
      }
      if (path === "/api/v2/changes?limit=50&order=change_id_asc") {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-changes-page",
            version: 1,
            projectionStamp: "sha256:generation",
            diagnostics: [],
            next: null,
            changes: [],
          }),
        );
      }
      if (path === "/api/v2/attention?limit=50&order=change_id_asc") {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-attention",
            version: 2,
            projectionStamp: "sha256:generation",
            next: null,
            changes: [],
          }),
        );
      }
      throw new Error(`unexpected request ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeReader } = await import("../src/change-bootstrap");

    await bootstrapChangeReader({ poll: false });

    expect(requests).toEqual([
      "/api/v2/profile",
      "/api/v2/changes?limit=50&order=change_id_asc",
      "/api/v2/attention?limit=50&order=change_id_asc",
      "/api/v2/profile",
    ]);
    expect(document.querySelector("#master")?.textContent).toContain(
      "No Changes.",
    );
  });

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
      if (path.startsWith("/api/v2/changes?")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-changes-page",
            version: 1,
            projectionStamp: "sha256:generation",
            diagnostics: [],
            next: null,
            presentations: {
              "change:sha256:one": {
                currentRevisions: [
                  {
                    revision: {
                      revisionId: "rev:sha256:a",
                      objectArtifactContentHash: `sha256:${"a".repeat(64)}`,
                    },
                    revisionProposalSummary: "First exact state",
                    summarySource: "revision_proposal_summary",
                  },
                  {
                    revision: {
                      revisionId: "rev:sha256:b",
                      objectArtifactContentHash: `sha256:${"b".repeat(64)}`,
                    },
                    summarySource: "absent",
                  },
                ],
              },
              "change:sha256:z-divergent": {
                currentRevisions: [],
              },
            },
            changes: [
              {
                changeId: "change:sha256:one",
                declarationState: "authoritative",
                titleAssertions: [],
                memberCount: 0,
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
                changeId: "change:sha256:z-divergent",
                declarationState: "authoritative",
                titleAssertions: [],
                memberCount: 0,
                topology: "replacement_divergent",
                lifecycle: "in_progress",
                attentionSummary: "conflicted",
                availabilitySummary: "available",
                projectionStamp: "sha256:generation",
                currentRevisionRefs: [],
              },
            ],
          }),
        );
      }
      if (path.startsWith("/api/v2/attention?")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-attention",
            version: 2,
            projectionStamp: "sha256:generation",
            next: null,
            changes: [],
            presentations: {},
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
            declarationState: "authoritative",
            titleAssertions: [],
            memberCount: 0,
            revision: {
              revisionId: "rev:sha256:b",
              objectArtifactContentHash: `sha256:${"b".repeat(64)}`,
            },
            revisionCurrency: "current",
            relationClassification: "current",
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
            factContentPresentations: {
              "observation:one": {
                contentType: "text/markdown",
                bodyContentState: "present",
                content: {
                  kind: "observation",
                  title: "Readable finding",
                  body: "Exact context",
                },
              },
            },
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
      new Set([
        "/api/v2/changes?limit=50&order=change_id_asc",
        "/api/v2/attention?limit=50&order=change_id_asc",
        "/api/v2/profile",
      ]),
    );
    expect(document.querySelector("[data-change-id]")?.textContent).toContain(
      "parallel current",
    );
    expect(document.querySelector("#master")?.textContent).toContain(
      "replacement divergent",
    );
    expect(document.querySelectorAll("[data-revision-id]")).toHaveLength(2);
    expect(document.querySelector("[data-revision-id]")?.textContent).toBe(
      "Current Revision — proposal summary: First exact state · rev:sha256:a",
    );
    expect(
      document.querySelectorAll("[data-revision-id]")[1]?.textContent,
    ).toBe("Current Revision — summary absent · rev:sha256:b");

    // `#master` is a flex shell that gives one lens body the available height.
    // The Change reader must keep its heading and cards inside that one scrollable
    // body; direct cards would each become flex children and collapse into one
    // another on a populated store.
    const changeList = document.querySelector<HTMLElement>("#master > .units");
    expect(changeList).not.toBeNull();
    expect(document.querySelector("#master")?.children).toHaveLength(1);
    expect(changeList?.querySelector("h1")?.textContent).toBe("Changes · 2");
    expect(changeList?.querySelectorAll("[data-change-id]")).toHaveLength(2);

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
    expect(requests).toContainEqual(
      expect.stringContaining(
        "/api/v2/changes/change%3Asha256%3Aone/revisions/rev%3Asha256%3Ab?artifactHash=sha256%3A",
      ),
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
    expect(requests).toContainEqual(
      expect.stringContaining("/resource?artifactHash=sha256%3A"),
    );

    const compare = Array.from(
      document.querySelectorAll<HTMLButtonElement>("#master button"),
    ).find((button) => button.textContent === "Compare exact Revisions");
    compare?.click();
    await vi.waitFor(() => {
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "revision_interdiff_not_available",
      );
    });
    expect(requests).toContainEqual(expect.stringContaining("/interdiff/"));
  });

  it("bootstraps one coherent generation without optional presentations", async () => {
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile") {
        return new Response(JSON.stringify(profile("ready")));
      }
      if (path.startsWith("/api/v2/changes?")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-changes-page",
            version: 1,
            projectionStamp: "sha256:generation",
            diagnostics: [],
            next: null,
            changes: [
              {
                changeId: "change:sha256:one",
                declarationState: "authoritative",
                titleAssertions: [],
                memberCount: 0,
                topology: "initial",
                lifecycle: "in_progress",
                attentionSummary: "in_progress",
                availabilitySummary: "available",
                projectionStamp: "sha256:generation",
                currentRevisionRefs: [
                  {
                    revisionId: "rev:sha256:one",
                    objectArtifactContentHash: `sha256:${"a".repeat(64)}`,
                  },
                ],
              },
            ],
          }),
        );
      }
      if (path.startsWith("/api/v2/attention?")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-attention",
            version: 2,
            projectionStamp: "sha256:generation",
            next: null,
            changes: [],
          }),
        );
      }
      if (path.includes("/api/v2/changes/change%3Asha256%3Aone/revisions/")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.review-change-revision",
            version: 1,
            changeId: "change:sha256:one",
            declarationState: "authoritative",
            titleAssertions: [],
            memberCount: 0,
            revision: {
              revisionId: "rev:sha256:one",
              objectArtifactContentHash: `sha256:${"a".repeat(64)}`,
            },
            revisionCurrency: "current",
            relationClassification: "current",
            availability: "available",
            factPresentations: [],
            associations: [],
            diagnostics: [],
            projectionStamp: "sha256:generation",
          }),
        );
      }
      throw new Error(`unexpected request ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeReader } = await import("../src/change-bootstrap");

    await bootstrapChangeReader({ poll: false });

    expect(document.querySelector("[data-change-id]")?.textContent).toContain(
      "change:sha256:one",
    );
    document.querySelector<HTMLButtonElement>("[data-revision-id]")?.click();
    await vi.waitFor(() => {
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "No facts.",
      );
    });
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
      const schema = path.startsWith("/api/v2/changes?")
        ? "pointbreak.inspect-changes-page"
        : "pointbreak.inspect-attention";
      return new Response(
        JSON.stringify({
          schema,
          version: path.startsWith("/api/v2/changes?") ? 1 : 2,
          changes: [],
          diagnostics: [],
          next: null,
          projectionStamp: path,
        }),
      );
    }) as typeof fetch;
    const { bootstrapChangeReader } = await import("../src/change-bootstrap");

    await bootstrapChangeReader({ poll: false });

    expect(requests).toHaveLength(4);
    expect(document.querySelector("[data-change-id]")).toBeNull();
    expect(document.querySelector("#master")?.textContent).toContain(
      "Reader refused",
    );
  });

  it("restarts before paint when the postflight profile moved during staging", async () => {
    let profileReads = 0;
    let changesReads = 0;
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile") {
        profileReads += 1;
        return new Response(
          JSON.stringify({
            ...profile("ready"),
            authorityCursor: { eventCount: profileReads === 2 ? 4 : 3 },
            commitGraphStamp:
              profileReads === 2
                ? "sha256:changed-commit-graph"
                : "sha256:commit-graph",
          }),
        );
      }
      if (path.startsWith("/api/v2/changes?")) {
        changesReads += 1;
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-changes-page",
            version: 1,
            projectionStamp: "sha256:generation",
            diagnostics: [],
            next: null,
            changes: [],
          }),
        );
      }
      if (path.startsWith("/api/v2/attention?")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-attention",
            version: 2,
            projectionStamp: "sha256:generation",
            next: null,
            changes: [],
          }),
        );
      }
      throw new Error(`unexpected request ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeReader } = await import("../src/change-bootstrap");

    await bootstrapChangeReader({ poll: false });

    expect(changesReads).toBe(2);
    expect(document.querySelector("#master")?.textContent).toContain(
      "No Changes.",
    );
    expect(document.querySelector("#error")?.textContent).toContain(
      "restarting from the first page",
    );
  });

  it("loads opaque continuation pages in server order and caps live cards at 150", async () => {
    const rows = (page: number) =>
      Array.from({ length: 50 }, (_, index) => {
        const id = String(page * 50 + index + 1).padStart(3, "0");
        return {
          changeId: `change:sha256:${id}`,
          declarationState: "authoritative",
          titleAssertions: [],
          memberCount: 0,
          topology: "initial",
          lifecycle: "in_progress",
          attentionSummary: "in_progress",
          availabilitySummary: "available",
          projectionStamp: "sha256:generation",
          currentRevisionRefs: [],
        };
      });
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile") {
        return new Response(JSON.stringify(profile("ready")));
      }
      if (path.startsWith("/api/v2/attention?")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-attention",
            version: 2,
            projectionStamp: "sha256:generation",
            next: null,
            changes: [],
          }),
        );
      }
      if (path.startsWith("/api/v2/changes?")) {
        const page = path.includes("after=page-3")
          ? 3
          : path.includes("after=page-2")
            ? 2
            : path.includes("after=page-1")
              ? 1
              : 0;
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-changes-page",
            version: 1,
            projectionStamp: "sha256:generation",
            diagnostics: [],
            next: page === 3 ? null : `page-${page + 1}`,
            changes: rows(page),
          }),
        );
      }
      throw new Error(`unexpected request ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeReader } = await import("../src/change-bootstrap");

    await bootstrapChangeReader({ poll: false });
    for (let page = 0; page < 3; page += 1) {
      document
        .querySelector<HTMLButtonElement>("#master button:last-child")
        ?.click();
      await vi.waitFor(() => {
        expect(document.querySelectorAll("[data-change-id]")).toHaveLength(
          page === 0 ? 100 : 150,
        );
        expect(
          Array.from(
            document.querySelectorAll<HTMLElement>("[data-change-id]"),
          ).at(-1)?.dataset.changeId,
        ).toBe(`change:sha256:${String((page + 2) * 50).padStart(3, "0")}`);
      });
    }

    const cards = document.querySelectorAll<HTMLElement>("[data-change-id]");
    expect(cards).toHaveLength(150);
    expect(cards[0]?.dataset.changeId).toBe("change:sha256:051");
    expect(Array.from(cards).at(-1)?.dataset.changeId).toBe(
      "change:sha256:200",
    );
  });

  it("visibly restarts a stale 409 continuation without requesting a legacy route", async () => {
    const requests: string[] = [];
    let initialPages = 0;
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile") {
        return new Response(JSON.stringify(profile("ready")));
      }
      if (path.startsWith("/api/v2/attention?")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-attention",
            version: 2,
            projectionStamp: "sha256:generation",
            next: null,
            changes: [],
          }),
        );
      }
      if (path.includes("after=stale-token")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-change-page-error",
            version: 1,
            code: "stale_projection",
            message: "server-only detail",
          }),
          { status: 409 },
        );
      }
      if (path.startsWith("/api/v2/changes?")) {
        initialPages += 1;
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-changes-page",
            version: 1,
            projectionStamp: "sha256:generation",
            diagnostics: [],
            next: "stale-token",
            changes: [
              {
                changeId: "change:sha256:one",
                declarationState: "authoritative",
                titleAssertions: [],
                memberCount: 0,
                topology: "initial",
                lifecycle: "in_progress",
                attentionSummary: "in_progress",
                availabilitySummary: "available",
                projectionStamp: "sha256:generation",
                currentRevisionRefs: [],
              },
            ],
          }),
        );
      }
      throw new Error(`unexpected request ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeReader } = await import("../src/change-bootstrap");

    await bootstrapChangeReader({ poll: false });
    document
      .querySelector<HTMLButtonElement>("#master button:last-child")
      ?.click();
    await vi.waitFor(() => {
      expect(initialPages).toBe(2);
      expect(document.querySelector("#error")?.textContent).toContain(
        "stale; restarting from the first page",
      );
    });
    expect(
      requests.every(
        (path) =>
          ![
            "/api/history",
            "/api/revisions",
            "/api/threads",
            "/api/attention",
          ].some((legacy) => path.startsWith(legacy)),
      ),
    ).toBe(true);
  });

  it("keeps an accumulated page window when an unchanged profile poll completes", async () => {
    vi.useFakeTimers();
    let profileReads = 0;
    let changesReads = 0;
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile") {
        profileReads += 1;
        return new Response(JSON.stringify(profile("ready")));
      }
      if (path.startsWith("/api/v2/attention?")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-attention",
            version: 2,
            projectionStamp: "sha256:generation",
            next: null,
            changes: [],
          }),
        );
      }
      if (path.startsWith("/api/v2/changes?")) {
        changesReads += 1;
        const continued = path.includes("after=page-1");
        const id = continued ? "002" : "001";
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-changes-page",
            version: 1,
            projectionStamp: "sha256:generation",
            diagnostics: [],
            next: continued ? null : "page-1",
            changes: [
              {
                changeId: `change:sha256:${id}`,
                declarationState: "authoritative",
                titleAssertions: [],
                memberCount: 0,
                topology: "initial",
                lifecycle: "in_progress",
                attentionSummary: "in_progress",
                availabilitySummary: "available",
                projectionStamp: "sha256:generation",
                currentRevisionRefs: [],
              },
            ],
          }),
        );
      }
      throw new Error(`unexpected request ${path}`);
    }) as typeof fetch;
    const reader = await import("../src/change-bootstrap");

    await reader.bootstrapChangeReader();
    document
      .querySelector<HTMLButtonElement>("#master button:last-child")
      ?.click();
    await vi.waitFor(() => {
      expect(document.querySelectorAll("[data-change-id]")).toHaveLength(2);
    });
    await vi.advanceTimersByTimeAsync(3000);
    await vi.waitFor(() => expect(profileReads).toBe(4));

    expect(changesReads).toBe(2);
    expect(
      Array.from(
        document.querySelectorAll<HTMLElement>("[data-change-id]"),
      ).map((card) => card.dataset.changeId),
    ).toEqual(["change:sha256:001", "change:sha256:002"]);
    reader.stopChangeReader();
  });

  it("does not publish an in-flight continuation after the reader stops", async () => {
    let resolveContinuation!: (response: Response) => void;
    const continuation = new Promise<Response>((resolve) => {
      resolveContinuation = resolve;
    });
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile") {
        return new Response(JSON.stringify(profile("ready")));
      }
      if (path.startsWith("/api/v2/attention?")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-attention",
            version: 2,
            projectionStamp: "sha256:generation",
            next: null,
            changes: [],
          }),
        );
      }
      if (path.includes("after=page-1")) return continuation;
      if (path.startsWith("/api/v2/changes?")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-changes-page",
            version: 1,
            projectionStamp: "sha256:generation",
            diagnostics: [],
            next: "page-1",
            changes: [
              {
                changeId: "change:sha256:001",
                declarationState: "authoritative",
                titleAssertions: [],
                memberCount: 0,
                topology: "initial",
                lifecycle: "in_progress",
                attentionSummary: "in_progress",
                availabilitySummary: "available",
                projectionStamp: "sha256:generation",
                currentRevisionRefs: [],
              },
            ],
          }),
        );
      }
      throw new Error(`unexpected request ${path}`);
    }) as typeof fetch;
    const reader = await import("../src/change-bootstrap");

    await reader.bootstrapChangeReader({ poll: false });
    document
      .querySelector<HTMLButtonElement>("#master button:last-child")
      ?.click();
    reader.stopChangeReader();
    resolveContinuation(
      new Response(
        JSON.stringify({
          schema: "pointbreak.inspect-changes-page",
          version: 1,
          projectionStamp: "sha256:generation",
          diagnostics: [],
          next: null,
          changes: [
            {
              changeId: "change:sha256:002",
              declarationState: "authoritative",
              titleAssertions: [],
              memberCount: 0,
              topology: "initial",
              lifecycle: "in_progress",
              attentionSummary: "in_progress",
              availabilitySummary: "available",
              projectionStamp: "sha256:generation",
              currentRevisionRefs: [],
            },
          ],
        }),
      ),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(document.querySelectorAll("[data-change-id]")).toHaveLength(1);
    expect(document.querySelector("#error")?.textContent).not.toContain(
      "restarting",
    );
  });

  it("does not restart for an old continuation failure after refresh installs a new generation", async () => {
    vi.useFakeTimers();
    let profileReads = 0;
    let fullPageReads = 0;
    let resolveContinuation!: (response: Response) => void;
    const continuation = new Promise<Response>((resolve) => {
      resolveContinuation = resolve;
    });
    const row = (id: string, stamp: string) => ({
      changeId: `change:sha256:${id}`,
      declarationState: "authoritative",
      titleAssertions: [],
      memberCount: 0,
      topology: "initial",
      lifecycle: "in_progress",
      attentionSummary: "in_progress",
      availabilitySummary: "available",
      projectionStamp: stamp,
      currentRevisionRefs: [],
    });
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile") {
        profileReads += 1;
        return new Response(
          JSON.stringify({
            ...profile("ready"),
            authorityCursor: { eventCount: profileReads < 3 ? 3 : 4 },
          }),
        );
      }
      if (path.includes("after=page-1")) return continuation;
      const refreshed = profileReads >= 3;
      const stamp = refreshed ? "sha256:generation-b" : "sha256:generation-a";
      if (path.startsWith("/api/v2/attention?")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-attention",
            version: 2,
            projectionStamp: stamp,
            next: null,
            changes: [],
          }),
        );
      }
      if (path.startsWith("/api/v2/changes?")) {
        fullPageReads += 1;
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-changes-page",
            version: 1,
            projectionStamp: stamp,
            diagnostics: [],
            next: refreshed ? null : "page-1",
            changes: [row(refreshed ? "010" : "001", stamp)],
          }),
        );
      }
      throw new Error(`unexpected request ${path}`);
    }) as typeof fetch;
    const reader = await import("../src/change-bootstrap");

    await reader.bootstrapChangeReader();
    document
      .querySelector<HTMLButtonElement>("#master button:last-child")
      ?.click();
    await vi.advanceTimersByTimeAsync(3000);
    await vi.waitFor(() => {
      expect(
        document.querySelector<HTMLElement>("[data-change-id]")?.dataset
          .changeId,
      ).toBe("change:sha256:010");
    });
    resolveContinuation(
      new Response(
        JSON.stringify({
          schema: "pointbreak.inspect-change-page-error",
          version: 1,
          code: "stale_projection",
          message: "obsolete continuation",
        }),
        { status: 409 },
      ),
    );
    await vi.advanceTimersByTimeAsync(0);

    expect(
      document.querySelector<HTMLElement>("[data-change-id]")?.dataset.changeId,
    ).toBe("change:sha256:010");
    expect(fullPageReads).toBe(2);
    expect(document.querySelector("#error")?.textContent).not.toContain(
      "restarting",
    );
    reader.stopChangeReader();
  });

  it("silently returns when an obsolete full-generation request fails", async () => {
    vi.useFakeTimers();
    let profileReads = 0;
    let rejectChanges!: (reason: Error) => void;
    const changes = new Promise<Response>((_resolve, reject) => {
      rejectChanges = reject;
    });
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile") {
        profileReads += 1;
        return new Response(JSON.stringify(profile("ready")));
      }
      if (path.startsWith("/api/v2/changes?")) return changes;
      if (path.startsWith("/api/v2/attention?")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-attention",
            version: 2,
            projectionStamp: "sha256:generation",
            next: null,
            changes: [],
          }),
        );
      }
      throw new Error(`unexpected request ${path}`);
    }) as typeof fetch;
    const reader = await import("../src/change-bootstrap");

    const boot = reader.bootstrapChangeReader();
    await vi.advanceTimersByTimeAsync(0);
    reader.stopChangeReader();
    rejectChanges(new Error("obsolete request failed"));
    await boot;
    await vi.advanceTimersByTimeAsync(3000);

    expect(profileReads).toBe(1);
    expect(document.querySelector("#master")?.textContent).not.toContain(
      "Reader refused",
    );
  });

  it.each([
    "response",
    "failure",
  ] as const)("does not let an older Change selection %s overwrite a newer detail", async (firstOutcome) => {
    let resolveFirst!: (response: Response) => void;
    let rejectFirst!: (error: Error) => void;
    const firstDetail = new Promise<Response>((resolve, reject) => {
      resolveFirst = resolve;
      rejectFirst = reject;
    });
    const row = (id: "a" | "b") => ({
      changeId: `change:sha256:${id}`,
      declarationState: "authoritative",
      titleAssertions: [],
      memberCount: 0,
      topology: "initial",
      lifecycle: "in_progress",
      attentionSummary: "in_progress",
      availabilitySummary: "available",
      projectionStamp: "sha256:generation",
      currentRevisionRefs: [],
    });
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile") {
        return new Response(JSON.stringify(profile("ready")));
      }
      if (path.startsWith("/api/v2/attention?")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-attention",
            version: 2,
            projectionStamp: "sha256:generation",
            next: null,
            changes: [],
          }),
        );
      }
      if (path === "/api/v2/changes?limit=50&order=change_id_asc") {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-changes-page",
            version: 1,
            projectionStamp: "sha256:generation",
            diagnostics: [],
            next: null,
            changes: [row("a"), row("b")],
          }),
        );
      }
      const selected = path.endsWith("change%3Asha256%3Aa") ? "a" : "b";
      const response = new Response(
        JSON.stringify({
          schema: "pointbreak.review-change",
          version: 1,
          summary: row(selected),
          relationClaims: [],
          diagnostics: [],
          projectionStamp: "sha256:generation",
        }),
      );
      return selected === "a" ? firstDetail : response;
    }) as typeof fetch;
    const reader = await import("../src/change-bootstrap");

    await reader.bootstrapChangeReader({ poll: false });
    const opens = document.querySelectorAll<HTMLButtonElement>(
      "[data-change-id] > button:first-child",
    );
    opens[0]?.click();
    opens[1]?.click();
    await vi.waitFor(() => {
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "change:sha256:b",
      );
    });
    if (firstOutcome === "response") {
      resolveFirst(
        new Response(
          JSON.stringify({
            schema: "pointbreak.review-change",
            version: 1,
            summary: row("a"),
            relationClaims: [],
            diagnostics: [],
            projectionStamp: "sha256:generation",
          }),
        ),
      );
    } else {
      rejectFirst(new Error("obsolete detail failed"));
    }
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "change:sha256:b",
    );
    expect(document.querySelector("#master")?.textContent).not.toContain(
      "Reader refused",
    );
  });

  it("refuses a detail whose profile postflight moved before publication", async () => {
    let profileReads = 0;
    const row = {
      changeId: "change:sha256:a",
      declarationState: "authoritative",
      titleAssertions: [],
      memberCount: 0,
      topology: "initial",
      lifecycle: "in_progress",
      attentionSummary: "in_progress",
      availabilitySummary: "available",
      projectionStamp: "sha256:generation",
      currentRevisionRefs: [],
    };
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile") {
        profileReads += 1;
        return new Response(
          JSON.stringify({
            ...profile("ready"),
            authorityCursor: { eventCount: profileReads < 3 ? 3 : 4 },
          }),
        );
      }
      if (path.startsWith("/api/v2/attention?")) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-attention",
            version: 2,
            projectionStamp: "sha256:generation",
            next: null,
            changes: [],
          }),
        );
      }
      if (path === "/api/v2/changes?limit=50&order=change_id_asc") {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-changes-page",
            version: 1,
            projectionStamp: "sha256:generation",
            diagnostics: [],
            next: null,
            changes: [row],
          }),
        );
      }
      return new Response(
        JSON.stringify({
          schema: "pointbreak.review-change",
          version: 1,
          summary: row,
          relationClaims: [],
          diagnostics: [],
          projectionStamp: "sha256:generation",
        }),
      );
    }) as typeof fetch;
    const reader = await import("../src/change-bootstrap");

    await reader.bootstrapChangeReader({ poll: false });
    document
      .querySelector<HTMLButtonElement>("[data-change-id] > button:first-child")
      ?.click();
    await vi.waitFor(() => {
      expect(document.querySelector("#master")?.textContent).toContain(
        "Reader refused",
      );
    });
    expect(document.querySelector("#detail-body")?.textContent).not.toContain(
      "change:sha256:a",
    );
  });
});
