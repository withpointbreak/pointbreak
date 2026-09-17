import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CHANGE_READER_DOCUMENTS } from "../src/change-protocol";
import { authorityCursor } from "./support/authority";
import { mountInspectorDom, resetDom } from "./support/dom";

const realSetTimeout = globalThis.setTimeout.bind(globalThis);

const profile = {
  schema: "pointbreak.inspect-reader-profile",
  version: 1,
  availability: "ready",
  authorityCursor: authorityCursor(1),
  commitGraphStamp: "sha256:stamp",
  minimumReaderProfile: "review_change_revision_v1",
  documents: { ...CHANGE_READER_DOCUMENTS },
};
function page(lens: "changes" | "attention") {
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
        declarationState: "authoritative",
        titleAssertions: [],
        memberCount: 1,
        topology: "initial",
        lifecycle: "in_progress",
        attentionSummary: "in_progress",
        availabilitySummary: "available",
        currentRevisionRefs: [],
        projectionStamp: "sha256:generation",
      },
    ],
  };
}
function historyPage(eventId: string) {
  return {
    schema: "pointbreak.inspect-event-history",
    version: 1,
    authorityCursor: authorityCursor(1),
    sourceChangeProjectionStamp: "sha256:generation",
    timelineProjectionStamp: "sha256:timeline",
    order: "desc",
    eventCount: 1,
    matchCount: 1,
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
    entries: [
      {
        eventId,
        eventType: "review_note_imported",
        occurredAt: "2026-08-08T00:00:00Z",
        payloadHash: "sha256:payload",
        journalId: "journal:sha256:one",
        writer: {
          actorId: "actor:one",
          producer: { name: "pointbreak", version: "0.10.0" },
        },
        verificationStatus: "valid",
        assertionMode: "advisory",
        subject: { kind: "journal", journalId: "journal:sha256:one" },
        changeIds: [],
        revisionRefs: [],
        unresolvedRevisionIds: [],
        summary: { kind: "review_note_imported" },
      },
    ],
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((success) => {
    resolve = success;
  });
  return { promise, resolve };
}
const unavailable = {
  schema: "pointbreak.inspect-change-projection-error",
  version: 1,
  code: "projection_rebuild_required",
  message: "derived Change projection rebuild is required",
  retryable: false,
};
const status = (overrides: Record<string, unknown> = {}) => ({
  schema: "pointbreak.inspect-derived-access-status",
  version: 1,
  active: true,
  availability: "rebuild_required",
  namespace: "stable",
  rebuildInFlight: false,
  rebuildPaused: false,
  servingCurrent: false,
  fallbackInFlight: false,
  actions: ["authoritative_fallback", "retry"],
  ...overrides,
});
const identity = {
  schema: "pointbreak.inspect-identity",
  storeIdentity: "store:sha256:one",
  contextIdentity: "context:sha256:one",
  repository: "pointbreak",
  placement: { tier: "clone", label: "clone store" },
};

beforeEach(async () => {
  vi.resetModules();
  mountInspectorDom();
  history.replaceState(null, "", "/#/changes");
  // Happy DOM emits a non-standard hashchange for replaceState. Drain it
  // before any Inspector route listener can observe this fixture setup.
  await new Promise<void>((resolve) => realSetTimeout(resolve, 0));
});
afterEach(async () => {
  (await import("../src/change-inspector")).stopChangeInspector();
  vi.restoreAllMocks();
  resetDom();
});

describe("Change-first recovery integration", () => {
  it("keeps the shell usable and starts fallback only after an explicit click", async () => {
    const requests: Array<{ path: string; method: string }> = [];
    globalThis.fetch = vi.fn(async (input, init) => {
      const path = String(input);
      requests.push({ path, method: init?.method ?? "GET" });
      if (path === "/api/identity")
        return new Response(JSON.stringify(identity));
      if (path === "/api/derived-access/status")
        return new Response(JSON.stringify(status()));
      if (!path.includes("access=authoritative"))
        return new Response(JSON.stringify(unavailable), { status: 503 });
      const headers = {
        "X-Pointbreak-Access-Source": "authoritative-fallback",
      };
      if (path.startsWith("/api/v2/profile"))
        return new Response(JSON.stringify(profile), { headers });
      if (path.startsWith("/api/v2/changes"))
        return new Response(JSON.stringify(page("changes")), { headers });
      if (path.startsWith("/api/v2/attention"))
        return new Response(JSON.stringify(page("attention")), { headers });
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;

    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    await vi.waitFor(() =>
      expect(
        document.querySelector("#derived-access-fallback")?.classList,
      ).not.toContain("hidden"),
    );
    expect(document.querySelector("#lens-switcher")?.textContent).toContain(
      "Changes",
    );
    expect(requests.some((request) => request.method === "POST")).toBe(false);

    document
      .querySelector<HTMLButtonElement>("#derived-access-fallback")
      ?.click();
    await vi.waitFor(() =>
      expect(
        document.querySelector(".unit-card[data-change-id]"),
      ).not.toBeNull(),
    );
    expect(document.querySelector("#derived-access-summary")?.textContent).toBe(
      "Authoritative fallback",
    );
    expect(
      requests
        .filter((request) => request.path.startsWith("/api/v2/"))
        .slice(1)
        .every(
          (request) =>
            request.path.includes("access=authoritative") &&
            request.method === "GET",
        ),
    ).toBe(true);
  });

  it("supersedes an authoritative Timeline consumer without releasing its transport slot", async () => {
    const oldBody = deferred<string>();
    const v2Requests: string[] = [];
    let historyRequests = 0;
    globalThis.fetch = vi.fn(async (input) => {
      const path = String(input);
      if (path === "/api/identity")
        return new Response(JSON.stringify(identity));
      if (path === "/api/derived-access/status")
        return new Response(JSON.stringify(status()));
      if (!path.includes("access=authoritative"))
        return new Response(JSON.stringify(unavailable), { status: 503 });
      v2Requests.push(path);
      const headers = {
        "X-Pointbreak-Access-Source": "authoritative-fallback",
      };
      if (path.startsWith("/api/v2/profile"))
        return new Response(JSON.stringify(profile), { headers });
      if (path.startsWith("/api/v2/changes"))
        return new Response(JSON.stringify(page("changes")), { headers });
      if (path.startsWith("/api/v2/attention"))
        return new Response(JSON.stringify(page("attention")), { headers });
      if (path.startsWith("/api/v2/history")) {
        historyRequests += 1;
        if (historyRequests === 1)
          return {
            ok: true,
            status: 200,
            headers: new Headers(headers),
            text: () => oldBody.promise,
          } as Response;
        return new Response(JSON.stringify(historyPage("evt:sha256:new")), {
          headers,
        });
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;

    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    await vi.waitFor(() =>
      expect(
        document.querySelector("#derived-access-fallback")?.classList,
      ).not.toContain("hidden"),
    );
    document
      .querySelector<HTMLButtonElement>("#derived-access-fallback")
      ?.click();
    await vi.waitFor(() =>
      expect(
        document.querySelector(".unit-card[data-change-id]"),
      ).not.toBeNull(),
    );

    history.replaceState(null, "", "/#/timeline?limit=100&order=desc");
    window.dispatchEvent(new HashChangeEvent("hashchange"));
    await vi.waitFor(() => expect(historyRequests).toBe(1));
    history.replaceState(null, "", "/#/timeline?limit=100&q=new&order=desc");
    window.dispatchEvent(new HashChangeEvent("hashchange"));
    await Promise.resolve();
    const blockedCount = v2Requests.length;
    await Promise.resolve();
    expect(v2Requests).toHaveLength(blockedCount);
    expect(
      document.querySelector('[data-event-id="evt:sha256:old"]'),
    ).toBeNull();

    oldBody.resolve(JSON.stringify(historyPage("evt:sha256:old")));
    await vi.waitFor(() => expect(historyRequests).toBe(2));
    await vi.waitFor(() =>
      expect(
        document.querySelector('[data-event-id="evt:sha256:new"]'),
      ).not.toBeNull(),
    );
    expect(
      document.querySelector('[data-event-id="evt:sha256:old"]'),
    ).toBeNull();
    expect(document.querySelector("#master")?.textContent).not.toContain(
      "Reader refused:",
    );
  });

  it("orders status and control observations without replaying a POST", async () => {
    let statusRequests = 0;
    let releaseStale!: (response: Response) => void;
    const stale = new Promise<Response>((resolve) => {
      releaseStale = resolve;
    });
    const methods: string[] = [];
    globalThis.fetch = vi.fn(async (input, init) => {
      const path = String(input);
      methods.push(`${init?.method ?? "GET"} ${path}`);
      if (path === "/api/identity")
        return new Response(JSON.stringify(identity));
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(unavailable), { status: 503 });
      if (path === "/api/derived-access/retry")
        return new Response(
          JSON.stringify(
            status({
              availability: "current",
              servingCurrent: true,
              actions: [],
            }),
          ),
        );
      if (path === "/api/derived-access/status") {
        statusRequests += 1;
        if (statusRequests === 2) return stale;
        return new Response(
          JSON.stringify(
            status(
              statusRequests === 1
                ? { rebuildInFlight: true, actions: ["wait", "retry"] }
                : {
                    availability: "current",
                    servingCurrent: true,
                    actions: [],
                  },
            ),
          ),
        );
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;

    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    await vi.waitFor(() =>
      expect(
        document.querySelector("#derived-access-wait")?.classList,
      ).not.toContain("hidden"),
    );
    document.querySelector<HTMLButtonElement>("#derived-access-wait")?.click();
    await vi.waitFor(() => expect(statusRequests).toBe(2));
    document.querySelector<HTMLButtonElement>("#derived-access-retry")?.click();
    await vi.waitFor(() => expect(statusRequests).toBe(3));
    releaseStale(
      new Response(
        JSON.stringify(status({ rebuildInFlight: true, actions: ["retry"] })),
      ),
    );
    await Promise.resolve();

    expect(methods.filter((entry) => entry.startsWith("POST "))).toEqual([
      "POST /api/derived-access/retry",
    ]);
    expect(
      document.querySelector("#derived-access-status")?.classList,
    ).toContain("hidden");
  });
});
