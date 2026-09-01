import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { loadChangeInspectorReading } from "../src/change-inspector-reading";
import type { ChangeInspectorRoute } from "../src/change-inspector-router";
import { CHANGE_READER_DOCUMENTS } from "../src/change-protocol";
import { authorityCursor } from "./support/authority";
import { mountInspectorDom, resetDom } from "./support/dom";

const changeId = "change:sha256:one";
const revision = {
  revisionId: "revision:sha256:one",
  objectArtifactContentHash: "sha256:artifact",
};
const exactHash =
  "#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact";

function profile(sequence = 1) {
  return {
    schema: "pointbreak.inspect-reader-profile",
    version: 1,
    availability: "ready",
    authorityCursor: authorityCursor(sequence),
    commitGraphStamp: "sha256:stamp",
    minimumReaderProfile: "review_change_revision_v1",
    documents: { ...CHANGE_READER_DOCUMENTS },
  };
}

function page(
  lens: "changes" | "attention",
  projectionStamp = "sha256:generation-1",
) {
  return {
    schema:
      lens === "changes"
        ? "pointbreak.inspect-changes-page"
        : "pointbreak.inspect-attention",
    version: lens === "changes" ? 1 : 2,
    projectionStamp,
    next: null,
    changes: [
      {
        changeId,
        declarationState: "authoritative",
        titleAssertions: [],
        memberCount: 1,
        topology: "initial",
        lifecycle: "in_progress",
        attentionSummary: "in_progress",
        availabilitySummary: "available",
        currentRevisionRefs: [revision],
        projectionStamp,
      },
    ],
  };
}

function revisionDetail(
  projectionStamp = "sha256:generation-1",
  nestedProjectionStamp = projectionStamp,
) {
  return {
    schema: "pointbreak.review-change-revision",
    version: 1,
    changeId,
    revision,
    membershipSupport: [],
    revisionCurrency: "current",
    relationClassification: "current",
    availability: "available",
    exactRevisionDocument: {
      schema: "pointbreak.review-revision-resource",
      version: 1,
      projectionStamp: nestedProjectionStamp,
      resource: { revision, objectId: "obj:sha256:one" },
      projection: { includeBody: true },
      availability: "available",
      capturedDocumentHash: "sha256:captured",
      capturedDocument: {
        schema: "pointbreak.review-snapshot",
        version: 1,
        contentHash: revision.objectArtifactContentHash,
        snapshot: {
          review_id: "review:sha256:one",
          object_id: "obj:sha256:one",
          files: [],
        },
      },
      diagnostics: [],
      cacheKey: "sha256:resource",
    },
    factPresentations: [],
    factPorts: [],
    associations: [],
    diagnostics: [],
    projectionStamp,
  };
}

function exactRoute(): Extract<ChangeInspectorRoute, { kind: "revision" }> {
  return { kind: "revision", changeId, revision, query: {} };
}

function isExactRevisionPath(path: string): boolean {
  return path.startsWith(
    "/api/v2/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?",
  );
}

function identityResponse(): Response {
  return new Response(
    JSON.stringify({
      schema: "pointbreak.inspect-identity",
      storeIdentity: "store:sha256:floor",
      contextIdentity: "context:sha256:floor",
      repository: "floor-pointbreak",
      placement: { tier: "clone", label: "clone store" },
    }),
  );
}

function staleProjectionResponse(): Response {
  return new Response(
    JSON.stringify({
      schema: "pointbreak.inspect-change-page-error",
      version: 1,
      code: "stale_projection",
    }),
    { status: 409 },
  );
}

function movingJournalResponse(): Response {
  return new Response(
    JSON.stringify({
      schema: "pointbreak.inspect-event-history-error",
      version: 1,
      code: "moving_journal",
      retryable: true,
    }),
    { status: 503 },
  );
}

async function stopComposition(): Promise<void> {
  const reader = await import("../src/change-inspector");
  reader.stopChangeInspector();
}

beforeEach(() => {
  vi.resetModules();
  localStorage.clear();
  sessionStorage.clear();
  mountInspectorDom();
  history.replaceState(null, "", `/${exactHash}`);
});

afterEach(async () => {
  await stopComposition();
  vi.useRealTimers();
  vi.restoreAllMocks();
  resetDom();
});

describe("exact-reading preservation floor", () => {
  it("rejects an exact reading whose stamp differs from the staged generation", async () => {
    globalThis.fetch = vi.fn(
      async () =>
        new Response(JSON.stringify(revisionDetail("sha256:generation-other"))),
    ) as typeof fetch;

    await expect(
      loadChangeInspectorReading(exactRoute(), "sha256:generation-1"),
    ).rejects.toThrow(
      "contextual Revision detail projection stamp does not match the staged Change generation",
    );
  });

  it("rejects an embedded resource from another projection stamp", async () => {
    globalThis.fetch = vi.fn(
      async () =>
        new Response(
          JSON.stringify(
            revisionDetail("sha256:generation-1", "sha256:generation-other"),
          ),
        ),
    ) as typeof fetch;

    await expect(
      loadChangeInspectorReading(exactRoute(), "sha256:generation-1"),
    ).rejects.toThrow(
      "embedded captured resource is from another projection stamp",
    );
  });

  it("accepts an exact reading only after a same-generation profile postflight", async () => {
    let profileRequests = 0;
    let exactRequests = 0;
    let activeStamp = "sha256:generation-1";
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") return identityResponse();
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        if (profileRequests === 4) activeStamp = "sha256:generation-2";
        const sequence = [1, 1, 2, 2, 2, 2][profileRequests - 1] ?? 2;
        return new Response(JSON.stringify(profile(sequence)));
      }
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes", activeStamp)));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention", activeStamp)));
      if (isExactRevisionPath(path)) {
        exactRequests += 1;
        return new Response(JSON.stringify(revisionDetail(activeStamp)));
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;

    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    expect(exactRequests).toBe(2);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Exact Revision",
    );
    expect(
      document.querySelector<HTMLElement>("#detail-body")?.dataset
        .changeReadingKey,
    ).toContain("sha256:generation-2");
  });

  it.each([
    ["stale_projection", staleProjectionResponse],
    ["moving_journal", movingJournalResponse],
  ])("retries the eligible %s refusal exactly once", async (_code, refusal) => {
    let exactRequests = 0;
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") return identityResponse();
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile()));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isExactRevisionPath(path)) {
        exactRequests += 1;
        return exactRequests === 1
          ? refusal()
          : new Response(JSON.stringify(revisionDetail()));
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;

    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    expect(exactRequests).toBe(2);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Exact Revision",
    );
  });

  it("does not retry a transport failure", async () => {
    let exactRequests = 0;
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") return identityResponse();
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile()));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isExactRevisionPath(path)) {
        exactRequests += 1;
        throw new TypeError("network down");
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;

    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    expect(exactRequests).toBe(1);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Reader refused this exact surface: server unavailable",
    );
  });

  it("does not retry projection_unstable", async () => {
    let exactRequests = 0;
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") return identityResponse();
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile()));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isExactRevisionPath(path)) {
        exactRequests += 1;
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-change-projection-error",
            version: 1,
            code: "projection_unstable",
            retryable: true,
          }),
          { status: 503 },
        );
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;

    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    expect(exactRequests).toBe(1);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Reader refused this exact surface: server response error",
    );
  });

  it("preserves a focused filter draft across a failed poll", async () => {
    vi.useFakeTimers();
    history.replaceState(null, "", "/#/changes");
    let profileRequests = 0;
    let reportPollStarted!: () => void;
    let releasePollFailure!: () => void;
    const pollStarted = new Promise<void>((resolve) => {
      reportPollStarted = resolve;
    });
    const pollFailureReleased = new Promise<void>((resolve) => {
      releasePollFailure = resolve;
    });
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") return identityResponse();
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        if (profileRequests === 3) {
          reportPollStarted();
          await pollFailureReleased;
          return new Response(JSON.stringify({ error: "poll failed" }), {
            status: 500,
          });
        }
        return new Response(JSON.stringify(profile()));
      }
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;

    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    // Settle any queued same-route hashchange before the draft is created so
    // this assertion isolates the poll that follows.
    await vi.advanceTimersByTimeAsync(0);
    const failedPoll = vi.advanceTimersByTimeAsync(3_000);
    await pollStarted;
    const input = document.querySelector<HTMLInputElement>("#filter-text");
    input?.focus();
    if (input) input.value = "unfinished draft";

    releasePollFailure();
    await failedPoll;

    expect(profileRequests).toBe(3);
    expect(document.querySelector("#refresh")?.getAttribute("data-state")).toBe(
      "degraded",
    );
    expect(
      document.querySelector<HTMLInputElement>("#filter-text")?.value,
    ).toBe("unfinished draft");
    expect(document.activeElement).toBe(input);
  });

  it("renders a typed selector refusal on the exact-reading surface", async () => {
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") return identityResponse();
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile()));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isExactRevisionPath(path)) {
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-change-page-error",
            version: 1,
            code: "invalid_query",
          }),
          { status: 400 },
        );
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;

    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Reader refused this exact surface: server response error",
    );
  });
});
