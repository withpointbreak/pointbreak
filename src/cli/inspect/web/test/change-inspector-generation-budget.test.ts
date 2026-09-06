import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CHANGE_READER_DOCUMENTS } from "../src/change-protocol";
import { authorityCursor } from "./support/authority";
import { mountInspectorDom, resetDom } from "./support/dom";

const profile = {
  schema: "pointbreak.inspect-reader-profile",
  version: 1,
  availability: "ready",
  authorityCursor: authorityCursor(1),
  commitGraphStamp: "sha256:stamp",
  minimumReaderProfile: "review_change_revision_v1",
  documents: { ...CHANGE_READER_DOCUMENTS },
};
const page = (
  lens: "changes" | "attention",
  projectionStamp = "sha256:generation",
) => ({
  schema:
    lens === "changes"
      ? "pointbreak.inspect-changes-page"
      : "pointbreak.inspect-attention",
  version: lens === "changes" ? 1 : 2,
  projectionStamp,
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
      currentRevisionRefs: [
        {
          revisionId: "revision:sha256:one",
          objectArtifactContentHash: "sha256:artifact",
        },
      ],
      projectionStamp,
    },
  ],
});

const revision = {
  revisionId: "revision:sha256:one",
  objectArtifactContentHash: "sha256:artifact",
};

function historyPage(projectionStamp = "sha256:generation") {
  return {
    schema: "pointbreak.inspect-event-history",
    version: 1,
    authorityCursor: authorityCursor(1),
    sourceChangeProjectionStamp: projectionStamp,
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
        eventId: "evt:sha256:one",
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

function changeDetail(projectionStamp = "sha256:generation") {
  const summary = page("changes", projectionStamp).changes[0];
  if (!summary) throw new Error("fixture needs one Change");
  return {
    schema: "pointbreak.review-change",
    version: 1,
    summary,
    memberRevisions: [{ revision, supportingClaimIds: [] }],
    unavailableMemberRevisions: [],
    membershipClaims: [],
    membershipWithdrawals: [],
    relationClaims: [],
    relationWithdrawals: [],
    links: [],
    effectiveSupersedes: [],
    pendingOrConflictingEdges: [],
    currentRevisionRefs: [revision],
    perCurrentRevisionQualification: [{ revision, qualified: true }],
    operativeObligations: [],
    diagnostics: [],
    projectionStamp,
  };
}

type Leaf =
  | "profile"
  | "changes"
  | "attention"
  | "history"
  | "postflight"
  | "detail";
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function serve() {
  const held = deferred<Response>();
  const body = deferred<string>();
  const c = {
    stall: "" as Leaf | "",
    holdBody: false,
    generation: 1,
    identity: "one",
    calls: [] as Leaf[],
    signals: [] as AbortSignal[],
    held,
    body,
    conflicts: 0,
  };
  let profiles = 0;
  globalThis.fetch = vi.fn(async (input, init) => {
    const path = String(input);
    if (path === "/api/identity")
      return new Response(
        JSON.stringify({
          schema: "pointbreak.inspect-identity",
          storeIdentity: `store:sha256:${c.identity}`,
          contextIdentity: "context:sha256:one",
          repository: "test",
          placement: { tier: "clone", label: "clone store" },
        }),
      );
    const leaf: Leaf =
      path === "/api/v2/profile"
        ? ++profiles % 2 === 1
          ? "profile"
          : "postflight"
        : path.startsWith("/api/v2/changes?")
          ? "changes"
          : path.startsWith("/api/v2/attention?")
            ? "attention"
            : path.startsWith("/api/v2/history?")
              ? "history"
              : "detail";
    c.calls.push(leaf);
    if (init?.signal) c.signals.push(init.signal);
    if (leaf === c.stall) {
      if (c.holdBody)
        return {
          ok: false,
          status: 500,
          text: () => c.body.promise,
        } as Response;
      return c.held.promise; // Intentionally ignores cancellation.
    }
    if (leaf === "changes" && c.conflicts-- > 0)
      return new Response(
        JSON.stringify({
          schema: "pointbreak.inspect-change-page-error",
          version: 1,
          code: "stale_projection",
        }),
        { status: 409 },
      );
    const stamp = `sha256:generation-${c.generation}`;
    return new Response(
      JSON.stringify(
        leaf === "profile" || leaf === "postflight"
          ? { ...profile, authorityCursor: authorityCursor(c.generation) }
          : leaf === "changes" || leaf === "attention"
            ? page(leaf, stamp)
            : leaf === "history"
              ? historyPage(stamp)
              : changeDetail(stamp),
      ),
    );
  }) as typeof fetch;
  return c;
}
const detailRoute = "#/changes/change%3Asha256%3Aone";
const readingKey = () =>
  document.querySelector<HTMLElement>("#detail-body")?.dataset.changeReadingKey;
const retry = () =>
  document.querySelector<HTMLButtonElement>("#connection-action");
async function flush() {
  await vi.advanceTimersByTimeAsync(0);
}
function navigate(hash: string) {
  history.replaceState(null, "", `/${hash}`);
  window.dispatchEvent(new HashChangeEvent("hashchange"));
}
beforeEach(() => {
  vi.resetModules();
  vi.useFakeTimers({ now: 0 });
  localStorage.clear();
  sessionStorage.clear();
  mountInspectorDom();
  history.replaceState(null, "", `/${detailRoute}`);
});
afterEach(async () => {
  (await import("../src/change-inspector")).stopChangeInspector();
  vi.useRealTimers();
  vi.restoreAllMocks();
  resetDom();
});

describe("route and recovery generation budget", () => {
  it.each([
    "profile",
    "changes",
    "attention",
    "history",
    "postflight",
  ] as const)("%s stall settles at 30 seconds and exposes a working Retry", async (leaf) => {
    if (leaf === "history") history.replaceState(null, "", "/#/timeline");
    const c = serve();
    c.stall = leaf;
    const { bootstrapChangeInspector, stopChangeInspector } = await import(
      "../src/change-inspector"
    );
    let settled = false;
    const pending = bootstrapChangeInspector().then(() => {
      settled = true;
    });
    await flush();
    const { getConnectionSnapshot } = await import("../src/connection");
    expect(getConnectionSnapshot().connection).toBe(
      leaf === "profile" ? "connecting" : "connected",
    );
    expect(c.calls).toContain(leaf);
    expect(settled).toBe(false);
    await vi.advanceTimersByTimeAsync(29_999);
    expect(settled).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(settled).toBe(true);
    await pending;
    expect(c.signals.length).toBeGreaterThan(0);
    expect(c.signals.every((signal) => signal.aborted)).toBe(true);
    expect(c.calls).not.toContain("detail");
    expect(retry()?.classList.contains("hidden")).toBe(false);
    expect(retry()?.textContent).toBe("Retry");
    expect(getConnectionSnapshot()).toEqual({
      connection: leaf === "profile" ? "unreachable" : "connected",
      refresh: "degraded",
    });
    const before = c.calls.length;
    await vi.advanceTimersByTimeAsync(30_000);
    expect(c.calls.length).toBe(before);
    c.stall = "";
    retry()?.click();
    await flush();
    expect(getConnectionSnapshot().refresh).not.toBe("degraded");
    if (leaf !== "history")
      expect(readingKey()).toContain("change%3Asha256%3Aone");
    stopChangeInspector();
    await flush();
    expect(vi.getTimerCount()).toBe(0);
  });

  it.each([
    false,
    true,
  ])("recovery timeout preserves only same-identity accepted data (retire=%s)", async (retire) => {
    const c = serve();
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    const oldKey = readingKey();
    expect(oldKey).toBeTruthy();
    c.generation = 2;
    if (retire) c.identity = "two";
    c.stall = "changes";
    retry()?.click();
    await flush();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(c.signals.at(-1)?.aborted).toBe(true);
    expect(retire ? readingKey() !== oldKey : readingKey() === oldKey).toBe(
      true,
    );
    expect(retry()?.textContent).toBe("Retry");
    expect(retry()?.classList.contains("hidden")).toBe(false);
    c.stall = "";
    retry()?.click();
    await flush();
    expect(readingKey()).toBeTruthy();
    expect(readingKey()).not.toBe(oldKey);
  });

  it.each([
    "resolve",
    "reject",
    "body",
  ] as const)("late %s cannot publish, dispatch, or alter health after timeout", async (mode) => {
    const c = serve();
    c.stall = "changes";
    c.holdBody = mode === "body";
    const { bootstrapChangeInspector, stopChangeInspector } = await import(
      "../src/change-inspector"
    );
    let settled = false;
    void bootstrapChangeInspector({ poll: false }).then(() => {
      settled = true;
    });
    await flush();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(settled).toBe(true);
    const { getConnectionSnapshot } = await import("../src/connection");
    const health = getConnectionSnapshot();
    const html = document.body.innerHTML;
    const count = c.calls.length;
    if (mode === "body")
      c.body.resolve(JSON.stringify({ error: "late failure" }));
    else if (mode === "reject")
      c.held.reject(new Error("late network failure"));
    else c.held.resolve(new Response(JSON.stringify(page("changes"))));
    await flush();
    expect(c.calls.length).toBe(count);
    expect(document.body.innerHTML).toBe(html);
    expect(getConnectionSnapshot()).toEqual(health);
    stopChangeInspector();
    await flush();
    expect(vi.getTimerCount()).toBe(0);
  });

  it.each([
    "query",
    "exact",
    "stop",
  ] as const)("%s supersession settles the obsolete phase without aborting its replacement", async (action) => {
    const c = serve();
    c.stall = "changes";
    const { bootstrapChangeInspector, stopChangeInspector } = await import(
      "../src/change-inspector"
    );
    let settled = false;
    void bootstrapChangeInspector({ poll: false }).then(() => {
      settled = true;
    });
    await flush();
    const obsolete = c.held;
    c.held = deferred<Response>();
    const oldSignals = [...c.signals];
    c.signals.length = 0;
    if (action === "stop") stopChangeInspector();
    else {
      c.stall = "attention";
      navigate(
        action === "query"
          ? "#/changes?q=new"
          : detailRoute +
              "/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
      );
    }
    await flush();
    expect(settled).toBe(true);
    expect(oldSignals.every((s) => s.aborted)).toBe(true);
    if (action !== "stop") {
      expect(c.signals.length).toBeGreaterThan(0);
      expect(c.signals.at(-1)?.aborted).toBe(false);
    }
    const count = c.calls.length;
    const html = document.body.innerHTML;
    obsolete.reject(new Error("obsolete"));
    await flush();
    expect(c.calls.length).toBe(count);
    expect(document.body.innerHTML).toBe(html);
    stopChangeInspector();
    await flush();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("aborts fan-out siblings before retrying a conflict under a fresh phase", async () => {
    const c = serve();
    c.stall = "attention";
    c.conflicts = 1;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    let settled = false;
    void bootstrapChangeInspector({ poll: false }).then(() => {
      settled = true;
    });
    await flush();
    expect(c.calls.filter((x) => x === "changes")).toHaveLength(2);
    expect(c.signals[0]?.aborted).toBe(true);
    expect(c.signals.at(-1)?.aborted).toBe(false);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(settled).toBe(true);
    expect(c.calls.filter((x) => x === "changes")).toHaveLength(2);
    c.stall = "";
    retry()?.click();
    await flush();
    expect(readingKey()).toBeTruthy();
  });

  it("ends generation before the independent document and postflight budgets", async () => {
    const c = serve();
    c.stall = "changes";
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    let settled = false;
    const pending = bootstrapChangeInspector({ poll: false }).then(() => {
      settled = true;
    });
    await flush();
    await vi.advanceTimersByTimeAsync(29_900);
    const generationHold = c.held;
    c.held = deferred<Response>();
    c.stall = "detail";
    generationHold.resolve(
      new Response(JSON.stringify(page("changes", "sha256:generation-1"))),
    );
    await flush();
    expect(c.calls.at(-1)).toBe("detail");
    await vi.advanceTimersByTimeAsync(29_900);
    expect(c.signals.at(-1)?.aborted).toBe(false);
    expect(settled).toBe(false);
    const detailHold = c.held;
    c.held = deferred<Response>();
    c.stall = "profile";
    detailHold.resolve(
      new Response(JSON.stringify(changeDetail("sha256:generation-1"))),
    );
    await flush();
    await vi.advanceTimersByTimeAsync(2_900);
    expect(c.signals.at(-1)?.aborted).toBe(false);
    expect(settled).toBe(false);
    c.held.resolve(new Response(JSON.stringify(profile)));
    await flush();
    await pending;
    expect(readingKey()).toBeTruthy();
  });

  it("preserves Reconnect when a generation timeout follows an unauthorized state", async () => {
    const c = serve();
    c.stall = "profile";
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    let settled = false;
    void bootstrapChangeInspector({ poll: false }).then(() => {
      settled = true;
    });
    await flush();
    const { markRequestFailure, getConnectionSnapshot } = await import(
      "../src/connection"
    );
    markRequestFailure("unauthorized");
    await vi.advanceTimersByTimeAsync(30_000);
    expect(settled).toBe(true);
    expect(getConnectionSnapshot().connection).toBe("unauthorized");
    expect(retry()?.textContent).toBe("Reconnect");
  });

  it("a new composition settles teardown without letting the obsolete phase clear its timer", async () => {
    const c = serve();
    c.stall = "changes";
    const reader = await import("../src/change-inspector");
    let stopped = false;
    void reader.bootstrapChangeInspector({ poll: false }).then(() => {
      stopped = true;
    });
    await flush();
    const obsolete = c.held;
    const old = c.signals.at(-1);
    c.held = deferred<Response>();
    c.stall = "attention";
    let settled = false;
    void reader.bootstrapChangeInspector({ poll: false }).then(() => {
      settled = true;
    });
    await flush();
    expect(stopped).toBe(true);
    expect(old?.aborted).toBe(true);
    obsolete.reject(new Error("old composition"));
    await flush();
    await vi.advanceTimersByTimeAsync(29_999);
    expect(settled).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(settled).toBe(true);
    expect(retry()?.textContent).toBe("Retry");
  });

  it("a Timeline boundary epoch cancels an in-flight recovery generation", async () => {
    history.replaceState(null, "", "/#/timeline");
    const c = serve();
    const reader = await import("../src/change-inspector");
    await reader.bootstrapChangeInspector({ poll: false });
    c.stall = "changes";
    retry()?.click();
    await flush();
    const obsolete = c.signals.at(-1);
    expect(obsolete?.aborted).toBe(false);
    c.stall = "";
    document.querySelector<HTMLElement>("#timeline")?.focus();
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "G", bubbles: true }),
    );
    await flush();
    expect(obsolete?.aborted).toBe(true);
    const html = document.body.innerHTML;
    const count = c.calls.length;
    c.held.reject(new Error("obsolete recovery"));
    await flush();
    expect(c.calls.length).toBe(count);
    expect(document.body.innerHTML).toBe(html);
    expect(document.querySelector("#master")?.textContent).not.toContain(
      "refused",
    );
  });

  it("a second conflict refuses without a third automatic phase", async () => {
    const c = serve();
    c.conflicts = 2;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    expect(c.calls.filter((x) => x === "changes")).toHaveLength(2);
    expect(c.calls).not.toContain("detail");
    expect(document.querySelector("#master")?.textContent).toContain("refused");
  });
});
