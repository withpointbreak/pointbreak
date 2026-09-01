import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { parseChangeInspectorRoute } from "../src/change-inspector-router";
import type { ChangeInspectorSnapshot } from "../src/change-inspector-state";
import {
  CHANGE_READER_DOCUMENTS,
  type EventHistoryDocument,
  type EventHistoryEntry,
  type ReaderProfileAvailability,
} from "../src/change-protocol";
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
    entries: [],
  };
}

function searchableHistoryPage(
  projectionStamp = "sha256:generation",
): EventHistoryDocument {
  return {
    ...(historyPage(projectionStamp) as EventHistoryDocument),
    completion: {
      eventTypes: ["review_note_imported", "validation_check_recorded"],
      trackIds: ["track:author"],
      changeIds: ["change:sha256:one", "change:sha256:two"],
      revisionRefs: [revision],
      unresolvedRevisionIds: ["revision:sha256:unresolved"],
    },
  };
}

function activationHistoryPage(
  context: Pick<
    EventHistoryEntry,
    "changeIds" | "revisionRefs" | "unresolvedRevisionIds"
  > = {
    changeIds: ["change:sha256:one"],
    revisionRefs: [revision],
    unresolvedRevisionIds: [],
  },
): EventHistoryDocument {
  const entry: EventHistoryEntry = {
    eventId: "evt:sha256:activation",
    eventType: "review_note_imported",
    occurredAt: "2026-08-08T00:00:00Z",
    payloadHash: "sha256:activation-payload",
    journalId: "journal:sha256:activation",
    writer: {
      actorId: "actor:author",
      producer: { name: "pointbreak", version: "0.10.0" },
    },
    verificationStatus: "valid",
    assertionMode: "advisory",
    subject: {
      kind: "journal",
      journalId: "journal:sha256:activation",
    },
    ...context,
    summary: { kind: "review_note_imported" },
  };
  return {
    ...(historyPage() as EventHistoryDocument),
    eventCount: 1,
    matchCount: 1,
    completion: {
      eventTypes: [entry.eventType],
      trackIds: [],
      changeIds: [...entry.changeIds],
      revisionRefs: [...entry.revisionRefs],
      unresolvedRevisionIds: [...entry.unresolvedRevisionIds],
    },
    entries: [entry],
  };
}

function boundaryHistoryPage(options: {
  authoritySequence?: number;
  eventIds: string[];
  next?: string;
  offset: number;
  timelineProjectionStamp?: string;
}): EventHistoryDocument {
  const authoritySequence = options.authoritySequence ?? 2;
  return {
    ...(historyPage() as EventHistoryDocument),
    authorityCursor: authorityCursor(authoritySequence),
    timelineProjectionStamp:
      options.timelineProjectionStamp ?? "sha256:timeline-current",
    // The decoder requires the document totals to agree with the authority
    // cursor it was minted against, so both counts follow that sequence.
    eventCount: authoritySequence,
    matchCount: authoritySequence,
    offset: options.offset,
    next: options.next,
    entries: options.eventIds.map((eventId) => ({
      eventId,
      eventType: "review_note_imported",
      occurredAt: "2026-08-08T00:00:00Z",
      payloadHash: `sha256:${eventId}`,
      journalId: "journal:sha256:test",
      writer: {
        actorId: "actor:test",
        producer: { name: "pointbreak", version: "0.10.0" },
      },
      verificationStatus: "valid",
      assertionMode: "advisory",
      subject: { kind: "journal", journalId: "journal:sha256:test" },
      changeIds: [],
      revisionRefs: [],
      unresolvedRevisionIds: [],
      summary: { kind: "review_note_imported" },
    })),
  };
}

function revisionDetail(projectionStamp = "sha256:generation") {
  return {
    schema: "pointbreak.review-change-revision",
    version: 1,
    changeId: "change:sha256:one",
    revision,
    membershipSupport: [],
    revisionCurrency: "current",
    relationClassification: "current",
    availability: "available",
    exactRevisionDocument: {
      schema: "pointbreak.review-revision-resource",
      version: 1,
      projectionStamp,
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
      message: "private server detail",
      retryable: true,
    }),
    { status: 503 },
  );
}

function isExactRevisionPath(path: string): boolean {
  return path.startsWith(
    "/api/v2/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?",
  );
}

function isChangeDetailPath(path: string): boolean {
  return path === "/api/v2/changes/change%3Asha256%3Aone";
}

function isExactResourcePath(path: string): boolean {
  return path.startsWith(
    "/api/v2/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone/resource?",
  );
}

function serveComposition(
  historyDocument: EventHistoryDocument,
  readerProfile: typeof profile = profile,
): string[] {
  const requests: string[] = [];
  globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
    const path = String(input);
    requests.push(path);
    if (path === "/api/v2/profile")
      return new Response(JSON.stringify(readerProfile));
    if (path.startsWith("/api/v2/changes?"))
      return new Response(JSON.stringify(page("changes")));
    if (path.startsWith("/api/v2/attention?"))
      return new Response(JSON.stringify(page("attention")));
    if (path.startsWith("/api/v2/history?"))
      return new Response(JSON.stringify(historyDocument));
    if (isChangeDetailPath(path))
      return new Response(JSON.stringify(changeDetail()));
    if (isExactRevisionPath(path))
      return new Response(JSON.stringify(revisionDetail()));
    throw new Error(`unexpected ${path}`);
  }) as typeof fetch;
  return requests;
}

interface PollCompositionControl {
  availability: ReaderProfileAvailability;
  changesMode: "ok" | "failure" | "hang" | "stale_once";
  generation: number;
  onStaleChanges: (() => void) | null;
  requests: string[];
  requestTimes: number[];
  signals: AbortSignal[];
}

function servePollComposition(): PollCompositionControl {
  const control: PollCompositionControl = {
    availability: "ready",
    changesMode: "ok",
    generation: 1,
    onStaleChanges: null,
    requests: [],
    requestTimes: [],
    signals: [],
  };
  globalThis.fetch = vi.fn(
    (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
      const path = String(input);
      control.requests.push(path);
      control.requestTimes.push(Date.now());
      if (init?.signal != null) control.signals.push(init.signal);
      const stamp = `sha256:generation-${control.generation}`;
      if (path === "/api/identity") {
        return Promise.resolve(
          new Response(
            JSON.stringify({
              schema: "pointbreak.inspect-identity",
              storeIdentity: "store:sha256:poll",
              contextIdentity: "context:sha256:poll",
              repository: "poll-pointbreak",
              placement: { tier: "clone", label: "clone store" },
            }),
          ),
        );
      }
      if (path === "/api/v2/profile") {
        return Promise.resolve(
          new Response(
            JSON.stringify({
              ...profile,
              availability: control.availability,
              authorityCursor: authorityCursor(control.generation),
            }),
          ),
        );
      }
      if (path.startsWith("/api/v2/changes?")) {
        if (control.changesMode === "failure") {
          return Promise.resolve(
            new Response(JSON.stringify({ error: "generation failure" }), {
              status: 500,
            }),
          );
        }
        if (control.changesMode === "stale_once") {
          control.changesMode = "ok";
          control.onStaleChanges?.();
          return Promise.resolve(staleProjectionResponse());
        }
        if (control.changesMode === "hang") {
          return new Promise<Response>((_resolve, reject) => {
            init?.signal?.addEventListener(
              "abort",
              () => reject(new DOMException("aborted", "AbortError")),
              { once: true },
            );
          });
        }
        return Promise.resolve(
          new Response(JSON.stringify(page("changes", stamp))),
        );
      }
      if (path.startsWith("/api/v2/attention?")) {
        return Promise.resolve(
          new Response(JSON.stringify(page("attention", stamp))),
        );
      }
      if (path.startsWith("/api/v2/history?")) {
        return Promise.resolve(
          new Response(
            JSON.stringify({
              ...historyPage(stamp),
              authorityCursor: authorityCursor(control.generation),
              eventCount: control.generation,
            }),
          ),
        );
      }
      throw new Error(`unexpected ${path}`);
    },
  ) as typeof fetch;
  return control;
}

function setNarrowViewport(narrow: boolean): void {
  vi.spyOn(window, "matchMedia").mockImplementation(
    (query: string) =>
      ({
        matches: narrow && query === "(max-width: 760px)",
        media: query,
        onchange: null,
        addListener: vi.fn(),
        removeListener: vi.fn(),
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        dispatchEvent: vi.fn(() => true),
      }) as unknown as MediaQueryList,
  );
}

beforeEach(() => {
  vi.resetModules();
  localStorage.clear();
  sessionStorage.clear();
  mountInspectorDom();
  history.replaceState(null, "", "/#/changes");
});
afterEach(async () => {
  const reader = await import("../src/change-inspector");
  reader.stopChangeInspector();
  vi.useRealTimers();
  vi.restoreAllMocks();
  resetDom();
});

describe("Change-first composition", () => {
  it.each([
    ["wide keyboard", false, "keyboard"],
    ["narrow keyboard", true, "keyboard"],
    ["wide pointer", false, "pointer"],
    ["narrow pointer", true, "pointer"],
  ])("descends from a selected located Timeline event to its sole exact annotated diff and returns via %s", async (_case, narrow, activationKind) => {
    setNarrowViewport(narrow);
    history.replaceState(
      null,
      "",
      "/#/timeline?q=review&limit=20&at=evt%3Asha256%3Aactivation",
    );
    serveComposition(activationHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    if (activationKind === "keyboard") {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key: "j", bubbles: true }),
      );
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    } else {
      document
        .querySelector<HTMLElement>(
          "#timeline [data-event-id='evt:sha256:activation']",
        )
        ?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    }
    await vi.waitFor(() =>
      expect(parseChangeInspectorRoute(location.hash)).toEqual({
        kind: "event",
        eventId: "evt:sha256:activation",
        historyQuery: { q: "review", limit: 20 },
        query: {},
      }),
    );
    await vi.waitFor(() =>
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Event",
      ),
    );

    await vi.waitFor(() => {
      const activation = document.querySelector<HTMLButtonElement>(
        "[data-exact-diff-activation]",
      );
      expect(activation?.textContent).toBe("Open annotated diff");
      expect(document.activeElement).toBe(activation);
    });
    // A same-event refresh replaces the projected detail body. Keep the
    // primary exact action focused across that repaint.
    const activationBeforeRefresh = document.querySelector<HTMLButtonElement>(
      "[data-exact-diff-activation]",
    );
    window.dispatchEvent(new HashChangeEvent("hashchange"));
    await vi.waitFor(() => {
      const activation = document.querySelector<HTMLButtonElement>(
        "[data-exact-diff-activation]",
      );
      expect(activation).not.toBe(activationBeforeRefresh);
      expect(activation?.textContent).toBe("Open annotated diff");
      expect(document.activeElement).toBe(activation);
    });
    // HTMLElement.click() models the native button activation synthesized by
    // Enter; the document controller intentionally leaves native controls
    // alone.
    document
      .querySelector<HTMLButtonElement>("[data-exact-diff-activation]")
      ?.click();
    await vi.waitFor(() =>
      expect(parseChangeInspectorRoute(location.hash)).toEqual({
        kind: "diff",
        changeId: "change:sha256:one",
        revision,
        query: {},
      }),
    );
    await vi.waitFor(() =>
      expect(document.querySelector("#diff-page")?.classList).not.toContain(
        "hidden",
      ),
    );

    document.querySelector<HTMLButtonElement>("#diff-page-close")?.click();
    await vi.waitFor(() =>
      expect(parseChangeInspectorRoute(location.hash)).toEqual({
        kind: "event",
        eventId: "evt:sha256:activation",
        historyQuery: { q: "review", limit: 20 },
        query: {},
      }),
    );
    await vi.waitFor(() =>
      expect(
        document
          .querySelector("#timeline")
          ?.getAttribute("aria-activedescendant"),
      ).toContain("evt_3Asha256_3Aactivation"),
    );

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
    await vi.waitFor(() =>
      expect(parseChangeInspectorRoute(location.hash)).toEqual({
        kind: "timeline",
        historyQuery: {
          q: "review",
          limit: 20,
          at: "evt:sha256:activation",
        },
      }),
    );
  });

  it.each([
    [
      "zero context",
      { changeIds: [], revisionRefs: [], unresolvedRevisionIds: [] },
      0,
      0,
    ],
    [
      "one unresolved Revision",
      {
        changeIds: ["change:sha256:one"],
        revisionRefs: [],
        unresolvedRevisionIds: ["revision:sha256:unresolved"],
      },
      1,
      0,
    ],
    [
      "an exact Revision plus an unresolved Revision",
      {
        changeIds: ["change:sha256:one"],
        revisionRefs: [revision],
        unresolvedRevisionIds: ["revision:sha256:unresolved"],
      },
      1,
      1,
    ],
    [
      "plural Changes",
      {
        changeIds: ["change:sha256:one", "change:sha256:two"],
        revisionRefs: [revision],
        unresolvedRevisionIds: [],
      },
      2,
      0,
    ],
    [
      "plural exact Revisions",
      {
        changeIds: ["change:sha256:one"],
        revisionRefs: [
          revision,
          {
            revisionId: "revision:sha256:two",
            objectArtifactContentHash: "sha256:artifact-two",
          },
        ],
        unresolvedRevisionIds: [],
      },
      1,
      2,
    ],
  ])("refuses to infer an annotated diff from %s", async (_label, context, expectedChangeChoices, expectedRevisionChoices) => {
    history.replaceState(
      null,
      "",
      "/#/timeline/events/evt%3Asha256%3Aactivation?q=review&limit=20",
    );
    const requests = serveComposition(activationHistoryPage(context));
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const eventRoute = parseChangeInspectorRoute(location.hash);
    const refusal = document.querySelector<HTMLElement>(
      "[data-event-diff-refusal]",
    );
    expect(document.activeElement).toBe(refusal);

    refusal?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );

    await vi.waitFor(() =>
      expect(parseChangeInspectorRoute(location.hash)).toEqual(eventRoute),
    );
    expect(refusal?.getAttribute("role")).toBe("status");
    expect(refusal?.textContent).toContain(
      "exactly one Change and one exact Revision",
    );
    expect(document.activeElement).toBe(refusal);
    expect(requests.some(isExactRevisionPath)).toBe(false);
    expect(
      document.querySelectorAll("[data-event-change-choice]"),
    ).toHaveLength(expectedChangeChoices);
    expect(
      document.querySelectorAll("[data-event-revision-choice]"),
    ).toHaveLength(expectedRevisionChoices);
  });

  it.each([
    ["wide", false],
    ["narrow", true],
  ])("opens the same canonical annotated diff from exact Revision detail at %s width", async (_viewport, narrow) => {
    setNarrowViewport(narrow);
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact&q=review",
    );
    serveComposition(activationHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    await vi.waitFor(() =>
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Exact Revision",
      ),
    );

    const activation = document.querySelector<HTMLButtonElement>(
      "[data-exact-diff-activation]",
    );
    expect(activation?.textContent).toBe("Open annotated diff");
    expect(document.activeElement).toBe(activation);
    activation?.click();

    await vi.waitFor(() =>
      expect(parseChangeInspectorRoute(location.hash)).toEqual({
        kind: "diff",
        changeId: "change:sha256:one",
        revision,
        query: { q: "review" },
      }),
    );
    await vi.waitFor(() =>
      expect(document.querySelector("#diff-page")?.classList).not.toContain(
        "hidden",
      ),
    );
    document.querySelector<HTMLButtonElement>("#diff-page-close")?.click();
    await vi.waitFor(() =>
      expect(parseChangeInspectorRoute(location.hash)).toEqual({
        kind: "revision",
        changeId: "change:sha256:one",
        revision,
        query: { q: "review" },
      }),
    );
  });

  it("renders a native Change Show in Timeline link with only canonical Change scope", async () => {
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone?q=old&topology=initial&after=opaque&limit=25&order=change_id_asc",
    );
    serveComposition(activationHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    await vi.waitFor(() =>
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Current Revisions",
      ),
    );

    const link = Array.from(
      document.querySelectorAll<HTMLAnchorElement>("#detail-body a"),
    ).find((candidate) => candidate.textContent === "Show in Timeline");
    expect(link?.getAttribute("href")).toBe(
      "#/timeline?change=change%3Asha256%3Aone",
    );
    expect(link?.getAttribute("aria-label")).toBe(
      "Show Change change:sha256:one in Timeline",
    );
    expect(parseChangeInspectorRoute(link?.hash ?? "")).toEqual({
      kind: "timeline",
      historyQuery: { change: "change:sha256:one" },
    });
  });

  it("renders a native exact-Revision Show in Timeline link with only canonical exact scope", async () => {
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact&q=old&topology=initial&after=opaque&limit=25&order=change_id_asc",
    );
    serveComposition(activationHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    await vi.waitFor(() =>
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Exact Revision",
      ),
    );

    const link = Array.from(
      document.querySelectorAll<HTMLAnchorElement>("#detail-body a"),
    ).find((candidate) => candidate.textContent === "Show in Timeline");
    expect(link?.getAttribute("href")).toBe(
      "#/timeline?change=change%3Asha256%3Aone&revision=revision%3Asha256%3Aone&artifactHash=sha256%3Aartifact",
    );
    expect(link?.getAttribute("aria-label")).toBe(
      "Show exact Revision revision:sha256:one with artifact sha256:artifact for Change change:sha256:one in Timeline",
    );
    expect(parseChangeInspectorRoute(link?.hash ?? "")).toEqual({
      kind: "timeline",
      historyQuery: {
        change: "change:sha256:one",
        revision: "revision:sha256:one",
        artifactHash: "sha256:artifact",
      },
    });
  });

  it("decodes and retries one typed moving-Journal Timeline refusal", async () => {
    history.replaceState(null, "", "/#/timeline?q=review&limit=20");
    let historyRequests = 0;
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (path.startsWith("/api/v2/history?")) {
        historyRequests += 1;
        return historyRequests === 1
          ? movingJournalResponse()
          : new Response(JSON.stringify(historyPage()));
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );

    await bootstrapChangeInspector({ poll: false });

    expect(historyRequests).toBe(2);
    expect(document.querySelector("#master")?.textContent).toContain(
      "Timeline",
    );
    expect(document.querySelector("#error")?.textContent).not.toContain(
      "private server detail",
    );
  });

  it("debounces valid Timeline input into one replace read while preserving outer filters", async () => {
    history.replaceState(
      null,
      "",
      "/#/timeline?limit=20&at=evt%3Asha256%3Aanchor&type=review_note_imported&track=track%3Aauthor&change=change%3Asha256%3Aone&revision=revision%3Asha256%3Aone&artifactHash=sha256%3Aartifact&order=asc",
    );
    const requests = serveComposition(searchableHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const search = document.querySelector<HTMLInputElement>("#filter-text");
    if (!search) throw new Error("missing Timeline search input");
    const replaceState = vi.spyOn(history, "replaceState");
    const historyRequestCount = () =>
      requests.filter((request) => request.startsWith("/api/v2/history?"))
        .length;
    const initialHistoryRequests = historyRequestCount();
    vi.useFakeTimers();

    for (const draft of ["revision:0", "revision:0123", "revision:01234567"]) {
      search.value = draft;
      search.dispatchEvent(new Event("input", { bubbles: true }));
    }
    search.dispatchEvent(new Event("change", { bubbles: true }));

    expect(replaceState).not.toHaveBeenCalled();
    expect(historyRequestCount()).toBe(initialHistoryRequests);
    await vi.advanceTimersByTimeAsync(149);
    expect(replaceState).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(replaceState).toHaveBeenCalledOnce();
    expect(parseChangeInspectorRoute(location.hash)).toEqual({
      kind: "timeline",
      historyQuery: {
        limit: 20,
        q: "revision:01234567",
        type: "review_note_imported",
        track: "track:author",
        change: "change:sha256:one",
        revision: revision.revisionId,
        artifactHash: revision.objectArtifactContentHash,
        order: "asc",
      },
    });
    await vi.advanceTimersByTimeAsync(0);
    expect(historyRequestCount()).toBe(initialHistoryRequests + 1);
  });

  it("keeps incomplete Timeline identity clauses local while completing only server-provided values", async () => {
    history.replaceState(null, "", "/#/timeline?limit=20");
    const requests = serveComposition(searchableHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const search = document.querySelector<HTMLInputElement>("#filter-text");
    const suggestions = document.querySelector<HTMLElement>(
      "#filter-suggestions",
    );
    if (!search || !suggestions) throw new Error("missing Timeline search UI");
    const initialHash = location.hash;
    const historyRequestCount = () =>
      requests.filter((request) => request.startsWith("/api/v2/history?"))
        .length;
    const initialHistoryRequests = historyRequestCount();
    vi.useFakeTimers();

    for (const [draft, expected] of [
      [
        "revision:",
        ["revision:revision:sha256:one", "revision:revision:sha256:unresolved"],
      ],
      ["rev:", ["rev:revision:sha256:one", "rev:revision:sha256:unresolved"]],
      ["change:", ["change:change:sha256:one", "change:change:sha256:two"]],
    ] as const) {
      search.value = draft;
      search.dispatchEvent(new Event("input", { bubbles: true }));
      expect(location.hash).toBe(initialHash);
      expect(historyRequestCount()).toBe(initialHistoryRequests);
      expect(
        Array.from(
          suggestions.querySelectorAll<HTMLElement>("[role='option']"),
        ).map((option) => option.textContent),
      ).toEqual(expected);
    }

    for (const draft of ["actor:", "tag:", "check:", "assessment:", "is:"]) {
      search.value = draft;
      search.dispatchEvent(new Event("input", { bubbles: true }));
      expect(suggestions.querySelectorAll("[role='option']")).toHaveLength(0);
      expect(search.getAttribute("aria-expanded")).toBe("false");
      await vi.advanceTimersByTimeAsync(150);
      expect(location.hash).toBe(initialHash);
      expect(historyRequestCount()).toBe(initialHistoryRequests);
    }
  });

  it("offers partial field, event-type, and track completions with full accessible identities", async () => {
    history.replaceState(null, "", "/#/timeline?limit=20");
    const fullRevisionId = `revision:sha256:${"a".repeat(64)}`;
    const fullChangeId = `change:sha256:${"b".repeat(64)}`;
    const searchable = searchableHistoryPage();
    serveComposition({
      ...searchable,
      completion: {
        ...searchable.completion,
        changeIds: [fullChangeId],
        revisionRefs: [
          {
            revisionId: fullRevisionId,
            objectArtifactContentHash: `sha256:${"c".repeat(64)}`,
          },
          {
            revisionId: fullRevisionId,
            objectArtifactContentHash: `sha256:${"d".repeat(64)}`,
          },
        ],
      },
    });
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const search = document.querySelector<HTMLInputElement>("#filter-text");
    const suggestions = document.querySelector<HTMLElement>(
      "#filter-suggestions",
    );
    if (!search || !suggestions) throw new Error("missing Timeline search UI");
    const optionTexts = () =>
      Array.from(
        suggestions.querySelectorAll<HTMLElement>("[role='option']"),
      ).map((option) => option.textContent);

    for (const [draft, expected] of [
      ["cha", ["change:"]],
      ["act", ["actor:"]],
      ["type:valid", ["type:validation_check_recorded"]],
      ["track:auth", ["track:track:author"]],
    ] as const) {
      search.value = draft;
      search.dispatchEvent(new Event("input", { bubbles: true }));
      expect(optionTexts()).toEqual(expected);
    }

    search.value = "revision:aaaa";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    const exactRevision =
      suggestions.querySelector<HTMLElement>("[role='option']");
    expect(suggestions.querySelectorAll("[role='option']")).toHaveLength(1);
    expect(exactRevision?.textContent).toBe("revision:revision:aaaaaaaa");
    expect(exactRevision?.title).toContain(fullRevisionId);
    expect(exactRevision?.getAttribute("aria-label")).toContain(fullRevisionId);

    search.value = "change:bbbb";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    const change = suggestions.querySelector<HTMLElement>("[role='option']");
    expect(change?.textContent).toBe("change:change:bbbbbbbb");
    expect(change?.title).toContain(fullChangeId);
    expect(change?.getAttribute("aria-label")).toContain(fullChangeId);
  });

  it("keeps invalid Timeline input local and announces its parser diagnostic", async () => {
    history.replaceState(null, "", "/#/timeline?q=before&limit=20");
    const requests = serveComposition(searchableHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const search = document.querySelector<HTMLInputElement>("#filter-text");
    const diagnostic = document.querySelector<HTMLElement>("#route-diagnostic");
    if (!search || !diagnostic) throw new Error("missing Timeline search UI");
    const initialHash = location.hash;
    const initialRequestCount = requests.length;

    search.value = 'revision:"two words"';
    search.dispatchEvent(new Event("input", { bubbles: true }));

    expect(location.hash).toBe(initialHash);
    expect(requests).toHaveLength(initialRequestCount);
    expect(search.getAttribute("aria-invalid")).toBe("true");
    expect(search.getAttribute("aria-describedby")).toBe("route-diagnostic");
    expect(diagnostic.classList).not.toContain("hidden");
    expect(diagnostic.textContent).toContain(
      "identity fragments cannot contain whitespace",
    );

    search.value = "revision:";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    expect(search.getAttribute("aria-invalid")).toBeNull();
    expect(diagnostic.classList).toContain("hidden");
  });

  it("keeps an over-limit multibyte Timeline query local", async () => {
    history.replaceState(null, "", "/#/timeline?q=before&limit=20");
    const requests = serveComposition(searchableHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const search = document.querySelector<HTMLInputElement>("#filter-text");
    const diagnostic = document.querySelector<HTMLElement>("#route-diagnostic");
    if (!search || !diagnostic) throw new Error("missing Timeline search UI");
    const initialHash = location.hash;
    const initialRequestCount = requests.length;
    vi.useFakeTimers();

    search.value = "é".repeat(129);
    search.dispatchEvent(new Event("input", { bubbles: true }));
    await vi.advanceTimersByTimeAsync(150);

    expect(location.hash).toBe(initialHash);
    expect(requests).toHaveLength(initialRequestCount);
    expect(search.getAttribute("aria-invalid")).toBe("true");
    expect(diagnostic.textContent).toContain("at most 256 bytes");
  });

  it("announces query notices from the accepted Timeline document", async () => {
    history.replaceState(null, "", "/#/timeline?q=review&limit=20");
    const searchable = searchableHistoryPage();
    serveComposition({
      ...searchable,
      queryNotices: ["The Timeline query was normalized by the reader."],
    });
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    const search = document.querySelector<HTMLInputElement>("#filter-text");
    const diagnostic = document.querySelector<HTMLElement>("#route-diagnostic");
    expect(search?.getAttribute("aria-describedby")).toBe("route-diagnostic");
    expect(diagnostic?.classList).not.toContain("hidden");
    expect(diagnostic?.textContent).toContain(
      "The Timeline query was normalized by the reader.",
    );
  });

  it("moves Enter from a settled Timeline search to the one Timeline tab stop", async () => {
    history.replaceState(null, "", "/#/timeline?q=free&limit=20");
    serveComposition(searchableHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const search = document.querySelector<HTMLInputElement>("#filter-text");
    const timeline = document.querySelector<HTMLElement>("#timeline");
    if (!search || !timeline) throw new Error("missing Timeline search UI");
    search.focus();

    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );

    expect(document.activeElement).toBe(timeline);
  });

  it("hands Enter to the Timeline after an in-flight query replacement mounts", async () => {
    history.replaceState(null, "", "/#/timeline?limit=20");
    serveComposition(searchableHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    let releaseProfile!: () => void;
    const profileGate = new Promise<void>((resolve) => {
      releaseProfile = resolve;
    });
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile") {
        await profileGate;
        return new Response(JSON.stringify(profile));
      }
      if (path.startsWith("/api/v2/changes?")) {
        return new Response(JSON.stringify(page("changes")));
      }
      if (path.startsWith("/api/v2/attention?")) {
        return new Response(JSON.stringify(page("attention")));
      }
      if (path.startsWith("/api/v2/history?")) {
        return new Response(JSON.stringify(searchableHistoryPage()));
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const search = document.querySelector<HTMLInputElement>("#filter-text");
    if (!search) throw new Error("missing Timeline search input");
    search.focus();
    search.value = "after";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );

    expect(parseChangeInspectorRoute(location.hash)).toEqual({
      kind: "timeline",
      historyQuery: { limit: 20, q: "after" },
    });
    expect(document.querySelector("#master")?.textContent).toContain(
      "Loading Change generation",
    );
    releaseProfile();
    await vi.waitFor(() =>
      expect(document.activeElement).toBe(
        document.querySelector<HTMLElement>("#timeline"),
      ),
    );
  });

  it("does not steal focus changed deliberately while the Timeline replacement is in flight", async () => {
    history.replaceState(null, "", "/#/timeline?limit=20");
    serveComposition(searchableHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    let releaseProfile!: () => void;
    const profileGate = new Promise<void>((resolve) => {
      releaseProfile = resolve;
    });
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile") {
        await profileGate;
        return new Response(JSON.stringify(profile));
      }
      if (path.startsWith("/api/v2/changes?")) {
        return new Response(JSON.stringify(page("changes")));
      }
      if (path.startsWith("/api/v2/attention?")) {
        return new Response(JSON.stringify(page("attention")));
      }
      if (path.startsWith("/api/v2/history?")) {
        return new Response(JSON.stringify(searchableHistoryPage()));
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const search = document.querySelector<HTMLInputElement>("#filter-text");
    const viewToggle =
      document.querySelector<HTMLButtonElement>("#view-toggle");
    if (!search || !viewToggle) throw new Error("missing Inspector controls");
    search.focus();
    search.value = "after";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );

    viewToggle.focus();
    expect(document.activeElement).toBe(viewToggle);
    releaseProfile();
    await vi.waitFor(() =>
      expect(document.querySelector<HTMLElement>("#timeline")).not.toBeNull(),
    );

    expect(document.activeElement).toBe(viewToggle);
  });

  it("opens the accessible command palette from a focused served search input", async () => {
    history.replaceState(null, "", "/#/timeline?limit=20");
    serveComposition(searchableHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const search = document.querySelector<HTMLInputElement>("#filter-text");
    const palette = document.querySelector<HTMLElement>("#cmd-palette");
    const input = document.querySelector<HTMLInputElement>("#cmd-input");
    const results = document.querySelector<HTMLElement>("#cmd-results");
    if (!search || !palette || !input || !results) {
      throw new Error("missing served command palette");
    }
    search.focus();
    search.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "k",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );

    expect(palette.classList).not.toContain("hidden");
    expect(document.activeElement).toBe(input);
    expect(input.getAttribute("role")).toBe("combobox");
    expect(results.getAttribute("role")).toBe("listbox");
    expect(results.textContent).toContain("Copy current link");
    expect(results.textContent).toContain("Clear filters");
  });

  it("blurs served search before clearing its query through route replacement", async () => {
    history.replaceState(null, "", "/#/timeline?limit=20&q=review");
    const replaceState = vi.spyOn(history, "replaceState");
    serveComposition(searchableHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const search = document.querySelector<HTMLInputElement>("#filter-text");
    if (!search) throw new Error("missing served search input");
    search.focus();

    search.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(document.activeElement).not.toBe(search);
    expect(parseChangeInspectorRoute(location.hash)).toEqual({
      kind: "timeline",
      historyQuery: { limit: 20, q: "review" },
    });

    document.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(parseChangeInspectorRoute(location.hash)).toEqual({
      kind: "timeline",
      historyQuery: { limit: 20 },
    });
    expect(replaceState).toHaveBeenCalled();
  });

  it("keeps Change-page search plain instead of advertising Timeline grammar", async () => {
    history.replaceState(null, "", "/#/changes");
    serveComposition(searchableHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const search = document.querySelector<HTMLInputElement>("#filter-text");
    const suggestions = document.querySelector<HTMLElement>(
      "#filter-suggestions",
    );
    if (!search || !suggestions) throw new Error("missing Change search UI");
    vi.useFakeTimers();

    search.value = "revision:";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    expect(suggestions.querySelectorAll("[role='option']")).toHaveLength(0);
    expect(search.getAttribute("aria-invalid")).toBeNull();
    await vi.advanceTimersByTimeAsync(150);

    expect(parseChangeInspectorRoute(location.hash)).toEqual({
      kind: "lens",
      lens: "changes",
      query: { q: "revision:" },
    });
  });

  it("cancels a Timeline draft when the live URL changes before hashchange", async () => {
    history.replaceState(null, "", "/#/timeline?limit=20");
    const requests = serveComposition(searchableHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const search = document.querySelector<HTMLInputElement>("#filter-text");
    if (!search) throw new Error("missing Timeline search input");
    vi.useFakeTimers();
    search.value = "review";
    search.dispatchEvent(new Event("input", { bubbles: true }));

    history.replaceState(null, "", "/#/changes");
    await vi.advanceTimersByTimeAsync(150);

    expect(location.hash).toBe("#/changes");
    expect(requests.some((request) => request.includes("q=review"))).toBe(
      false,
    );
  });

  it("accepts Timeline completions without constructing exact routes and keeps combobox focus", async () => {
    history.replaceState(null, "", "/#/timeline?limit=20");
    serveComposition(searchableHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const search = document.querySelector<HTMLInputElement>("#filter-text");
    const suggestions = document.querySelector<HTMLElement>(
      "#filter-suggestions",
    );
    if (!search || !suggestions) throw new Error("missing Timeline search UI");
    search.focus();
    const initialRoute = parseChangeInspectorRoute(location.hash);
    search.value = "cha";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
    );
    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    expect(search.value).toBe("change:");
    expect(parseChangeInspectorRoute(location.hash)).toEqual(initialRoute);
    expect(
      Array.from(
        suggestions.querySelectorAll<HTMLElement>("[role='option']"),
      ).map((option) => option.textContent),
    ).toEqual(["change:change:sha256:one", "change:change:sha256:two"]);

    search.value = "rev:one";
    search.dispatchEvent(new Event("input", { bubbles: true }));

    expect(search.getAttribute("role")).toBe("combobox");
    expect(search.getAttribute("aria-controls")).toBe("filter-suggestions");
    expect(suggestions.getAttribute("role")).toBe("listbox");
    expect(search.getAttribute("aria-expanded")).toBe("true");
    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
    );
    expect(search.getAttribute("aria-activedescendant")).not.toBeNull();
    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );

    expect(search.value).toBe("rev:revision:sha256:one ");
    expect(document.activeElement).toBe(search);
    expect(search.getAttribute("aria-expanded")).toBe("false");
    expect(parseChangeInspectorRoute(location.hash)).toEqual({
      kind: "timeline",
      historyQuery: { limit: 20, q: "rev:revision:sha256:one" },
    });

    search.value = "change:two";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
    );
    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Tab", bubbles: true }),
    );
    expect(search.value).toBe("change:change:sha256:two ");
    expect(document.activeElement).toBe(search);

    search.value = "revision:";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    expect(search.getAttribute("aria-expanded")).toBe("true");
    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
    expect(search.value).toBe("revision:");
    expect(document.activeElement).toBe(search);
    expect(search.getAttribute("aria-expanded")).toBe("false");
  });

  it("accepts a Timeline completion by pointer without losing search focus", async () => {
    history.replaceState(null, "", "/#/timeline?limit=20");
    serveComposition(searchableHistoryPage());
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const search = document.querySelector<HTMLInputElement>("#filter-text");
    if (!search) throw new Error("missing Timeline search input");
    search.focus();
    search.value = "change:two";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    const option = document.querySelector<HTMLElement>(
      "#filter-suggestions [role='option']",
    );
    option?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    option?.dispatchEvent(new MouseEvent("click", { bubbles: true }));

    expect(search.value).toBe("change:change:sha256:two ");
    expect(document.activeElement).toBe(search);
    expect(parseChangeInspectorRoute(location.hash)).toEqual({
      kind: "timeline",
      historyQuery: { limit: 20, q: "change:change:sha256:two" },
    });
  });

  it("returns an exact event search to its filtered Timeline instead of a card lens", async () => {
    history.replaceState(
      null,
      "",
      "/#/timeline/events/evt%3Asha256%3Aone?q=before&limit=20",
    );
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (path.startsWith("/api/v2/history?"))
        return new Response(JSON.stringify(historyPage()));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const search = document.querySelector<HTMLInputElement>("#filter-text");
    if (!search) throw new Error("missing search input");
    search.value = "after";
    search.dispatchEvent(new Event("change"));

    await vi.waitFor(() =>
      expect(location.hash).toBe("#/timeline?limit=20&q=after"),
    );
    expect(requests.some((path) => path.startsWith("/api/v2/changes?"))).toBe(
      true,
    );
  });

  it("consumes a same-document capability before strict Change routing", async () => {
    const token = "opaque_test_capability_0123456789abcdef";
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    const { sessionTokenKey } = await import("../src/auth");
    await bootstrapChangeInspector({ poll: false });
    requests.length = 0;

    history.replaceState(
      null,
      "",
      `/#/changes?limit=100&order=change_id_asc&token=${token}`,
    );
    window.dispatchEvent(new HashChangeEvent("hashchange"));

    await vi.waitFor(() =>
      expect(location.hash).toBe("#/changes?limit=100&order=change_id_asc"),
    );
    expect(sessionStorage.getItem(sessionTokenKey())).toBe(token);
    await vi.waitFor(() =>
      expect(requests).toContain(
        "/api/v2/changes?limit=100&order=change_id_asc",
      ),
    );
    expect(document.querySelector("#route-diagnostic")?.textContent).toBe("");
  });

  it("keeps keyboard selection local until Enter while the palette chord remains global", async () => {
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isExactRevisionPath(path))
        return new Response(JSON.stringify(revisionDetail()));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true }),
    );
    expect(document.querySelector(".change-card-selected")).not.toBeNull();
    expect(location.hash).toBe("#/changes");
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    await vi.waitFor(() =>
      expect(location.hash).toContain("/changes/change%3Asha256%3Aone"),
    );

    const search = document.querySelector<HTMLInputElement>("#filter-text");
    search?.focus();
    search?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true }),
    );
    expect(document.querySelector(".change-card-selected")).not.toBeNull();
    search?.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "k",
        ctrlKey: true,
        bubbles: true,
      }),
    );
    expect(document.querySelector("#cmd-palette")?.classList).not.toContain(
      "hidden",
    );
    expect(document.activeElement).toBe(document.querySelector("#cmd-input"));
  });

  it("crosses served Change pages only through signed renderer capabilities", async () => {
    history.replaceState(null, "", "/#/changes?limit=1");
    const requests: string[] = [];
    const changesPage = (
      changeId: string,
      capabilities: {
        previous: string | null;
        next: string | null;
        last: string | null;
      },
    ) => {
      const source = page("changes");
      const summary = source.changes[0];
      if (!summary) throw new Error("missing served Change summary");
      return {
        ...source,
        ...capabilities,
        changes: [{ ...summary, changeId }],
      };
    };
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (path.startsWith("/api/v2/changes?")) {
        const after = new URL(path, "http://pointbreak.test").searchParams.get(
          "after",
        );
        const response =
          after === "signed-last"
            ? changesPage("change:sha256:last", {
                previous: "signed-first",
                next: null,
                last: null,
              })
            : changesPage("change:sha256:first", {
                previous: null,
                next: "signed-next",
                last: "signed-last",
              });
        return new Response(JSON.stringify(response));
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const first = document.querySelector<HTMLButtonElement>(
      ".change-card-primary",
    );
    first?.focus();

    first?.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "G",
        bubbles: true,
        cancelable: true,
      }),
    );
    await vi.waitFor(() =>
      expect(parseChangeInspectorRoute(location.hash)).toEqual({
        kind: "lens",
        lens: "changes",
        query: { after: "signed-last", limit: 1 },
      }),
    );
    await vi.waitFor(() =>
      expect(
        document.querySelector<HTMLElement>(".unit-card[aria-current='true']")
          ?.dataset.changeId,
      ).toBe("change:sha256:last"),
    );
    const last = document.querySelector<HTMLButtonElement>(
      ".change-card-primary",
    );
    expect(document.activeElement).toBe(last);

    last?.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "b",
        bubbles: true,
        cancelable: true,
      }),
    );
    await vi.waitFor(() =>
      expect(parseChangeInspectorRoute(location.hash)).toEqual({
        kind: "lens",
        lens: "changes",
        query: { after: "signed-first", limit: 1 },
      }),
    );
    await vi.waitFor(() => {
      expect(
        requests.some((request) => request.includes("after=signed-last")),
      ).toBe(true);
      expect(
        requests.some((request) => request.includes("after=signed-first")),
      ).toBe(true);
    });
  });

  it("makes the closed narrow detail inert and restores focus after an exact reading", async () => {
    vi.spyOn(window, "matchMedia").mockImplementation(
      (query: string) =>
        ({
          matches: query === "(max-width: 760px)",
          media: query,
          onchange: null,
          addListener: vi.fn(),
          removeListener: vi.fn(),
          addEventListener: vi.fn(),
          removeEventListener: vi.fn(),
          dispatchEvent: vi.fn(() => true),
        }) as unknown as MediaQueryList,
    );
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isChangeDetailPath(path))
        return new Response(JSON.stringify(changeDetail()));
      if (isExactRevisionPath(path))
        return new Response(JSON.stringify(revisionDetail()));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    const detail = document.querySelector<HTMLElement>("#detail");
    const opener = document.querySelector<HTMLButtonElement>(
      "[data-change-id] .change-card-primary",
    );
    expect(detail?.inert).toBe(true);
    expect(detail?.getAttribute("aria-hidden")).toBe("true");
    opener?.focus();
    opener?.click();
    await vi.waitFor(() =>
      expect(
        document.querySelector<HTMLButtonElement>(
          "#detail-body .detail-current-revisions button",
        ),
      ).not.toBeNull(),
    );
    document
      .querySelector<HTMLButtonElement>(
        "#detail-body .detail-current-revisions button",
      )
      ?.click();
    await vi.waitFor(() =>
      expect(
        document.querySelector("[data-exact-diff-activation]"),
      ).not.toBeNull(),
    );
    expect(detail?.inert).toBe(false);
    expect(detail?.hasAttribute("aria-hidden")).toBe(false);
    expect(document.activeElement).toBe(
      document.querySelector("[data-exact-diff-activation]"),
    );
    for (const selector of [
      "#topbar",
      "#toolbar",
      "#master-rail",
      "#master",
      ".divider",
    ]) {
      expect(document.querySelector<HTMLElement>(selector)?.inert).toBe(true);
    }

    document.querySelector<HTMLButtonElement>("#detail-back")?.click();
    await vi.waitFor(() => {
      expect(location.hash).toBe("#/changes");
      expect(detail?.inert).toBe(true);
    });
    expect(detail?.inert).toBe(true);
    expect(detail?.getAttribute("aria-hidden")).toBe("true");
    expect(document.activeElement).toBe(opener);
    for (const selector of [
      "#topbar",
      "#toolbar",
      "#master-rail",
      "#master",
      ".divider",
    ]) {
      expect(document.querySelector<HTMLElement>(selector)?.inert).toBe(false);
    }
  });

  it("preserves the bounded query across lens changes and exposes local display modes", async () => {
    history.replaceState(null, "", "/#/changes?q=needle");
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    document
      .querySelector<HTMLButtonElement>("[data-lens='attention']")
      ?.click();
    expect(location.hash).toContain("#/attention?q=needle");
    expect(document.querySelector("#view-order-section")?.classList).toContain(
      "hidden",
    );
    expect(
      document.querySelector("#jump-latest")?.closest(".control-section")
        ?.classList,
    ).toContain("hidden");
    const compact =
      document.querySelector<HTMLInputElement>("#density-compact");
    if (compact) {
      compact.checked = true;
      compact.dispatchEvent(new Event("change", { bubbles: true }));
    }
    expect(document.documentElement.classList.contains("compact")).toBe(true);

    history.replaceState(
      null,
      "",
      "/#/attention?q=needle&topology=initial&after=opaque&limit=20&order=change_id_asc",
    );
    document.querySelector<HTMLButtonElement>("#filter-clear")?.click();
    expect(location.hash).toBe("#/attention?limit=20&order=change_id_asc");
  });

  it("sends an opaque continuation only to its active lens and clears it on lens changes", async () => {
    history.replaceState(
      null,
      "",
      "/#/changes?after=changes-page&limit=20&order=change_id_asc",
    );
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    expect(requests).toContain(
      "/api/v2/changes?limit=20&after=changes-page&order=change_id_asc",
    );
    expect(requests).toContain(
      "/api/v2/attention?limit=20&order=change_id_asc",
    );

    document
      .querySelector<HTMLButtonElement>("[data-lens='attention']")
      ?.click();
    expect(location.hash).toBe("#/attention?limit=20&order=change_id_asc");
    await vi.waitFor(() => {
      expect(
        requests.filter((path) => path.startsWith("/api/v2/changes?")),
      ).toHaveLength(2);
      expect(
        requests.filter((path) => path.startsWith("/api/v2/attention?")),
      ).toHaveLength(2);
    });
    expect(requests.at(-3)).toBe(
      "/api/v2/changes?limit=20&order=change_id_asc",
    );
    expect(requests.at(-2)).toBe(
      "/api/v2/attention?limit=20&order=change_id_asc",
    );
  });

  it("drops an Attention continuation before exact navigation so later polls cannot cross lenses", async () => {
    vi.useFakeTimers();
    history.replaceState(
      null,
      "",
      "/#/attention?after=attention-page&limit=20&order=change_id_asc",
    );
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isChangeDetailPath(path))
        return new Response(JSON.stringify(changeDetail()));
      if (isExactRevisionPath(path))
        return new Response(JSON.stringify(revisionDetail()));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();

    document.querySelector<HTMLButtonElement>(".change-card-primary")?.click();
    await vi.waitFor(() =>
      expect(
        document.querySelector<HTMLButtonElement>(
          "#detail-body .detail-current-revisions button",
        ),
      ).not.toBeNull(),
    );
    document
      .querySelector<HTMLButtonElement>(
        "#detail-body .detail-current-revisions button",
      )
      ?.click();
    await vi.waitFor(() =>
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Exact Revision",
      ),
    );
    expect(location.hash).not.toContain("after=");

    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() =>
      expect(
        requests.filter((path) => path.startsWith("/api/v2/profile")).length,
      ).toBeGreaterThanOrEqual(6),
    );
    expect(
      requests.some(
        (path) =>
          path.startsWith("/api/v2/changes?") &&
          path.includes("after=attention-page"),
      ),
    ).toBe(false);
  });

  it("maps 1, 2, and 3 to Timeline, Changes, and Attention", async () => {
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (path.startsWith("/api/v2/history?"))
        return new Response(JSON.stringify(historyPage()));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "2", bubbles: true }),
    );
    expect(location.hash).toBe("#/changes");
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "3", bubbles: true }),
    );
    expect(location.hash).toBe("#/attention");
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "1", bubbles: true }),
    );
    expect(location.hash).toBe("#/timeline");

    const divider = document.querySelector<HTMLElement>(".divider");
    divider?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
    );
    expect(divider?.getAttribute("aria-valuenow")).toBe("55");
    for (let step = 0; step < 6; step += 1) {
      divider?.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "ArrowRight",
          bubbles: true,
        }),
      );
    }
    expect(divider?.getAttribute("aria-valuenow")).toBe("75");
    divider?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    expect(divider?.getAttribute("aria-valuenow")).toBe("50");

    const detailViewport = document.querySelector<HTMLElement>("#detail-body");
    if (detailViewport) detailViewport.scrollTop = 17;
    const reading = document.querySelector<HTMLButtonElement>("#detail-read");
    reading?.click();
    expect(
      document.querySelector(".split")?.classList.contains("reading"),
    ).toBe(true);
    expect(reading?.getAttribute("aria-label")).toBe("Exit reading mode");
    expect(detailViewport?.scrollTop).toBe(17);
    document.querySelector<HTMLButtonElement>("#master-rail")?.click();
    expect(
      document.querySelector(".split")?.classList.contains("reading"),
    ).toBe(false);

    document.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "P",
        ctrlKey: true,
        shiftKey: true,
        bubbles: true,
      }),
    );
    expect(document.querySelector("#cmd-palette")?.classList).not.toContain(
      "hidden",
    );
    const paletteInput = document.querySelector<HTMLInputElement>("#cmd-input");
    expect(document.activeElement).toBe(paletteInput);
    expect(paletteInput?.getAttribute("role")).toBe("combobox");
    expect(document.querySelector("#cmd-results")?.getAttribute("role")).toBe(
      "listbox",
    );
    if (paletteInput) {
      paletteInput.value = "attention";
      paletteInput.dispatchEvent(new Event("input", { bubbles: true }));
    }
    expect(
      Array.from(
        document.querySelectorAll<HTMLButtonElement>("#cmd-results button"),
      ).map((button) => button.textContent),
    ).toEqual(["Open Attention"]);
    if (paletteInput) {
      paletteInput.value = "";
      paletteInput.dispatchEvent(new Event("input", { bubbles: true }));
    }
    paletteInput?.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Tab",
        shiftKey: true,
        bubbles: true,
      }),
    );
    expect(document.activeElement).toBe(paletteInput);
    paletteInput?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Tab", bubbles: true }),
    );
    expect(document.activeElement).toBe(paletteInput);
    paletteInput?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
    expect(document.querySelector("#cmd-palette")?.classList).toContain(
      "hidden",
    );

    const firstCard = document.querySelector<HTMLElement>(
      ".unit-card[data-change-id]",
    );
    const lastCard = firstCard?.cloneNode(true) as HTMLElement | undefined;
    if (lastCard) {
      lastCard.dataset.changeId = "change:sha256:last";
      firstCard?.parentElement?.append(lastCard);
    }
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "G", bubbles: true }),
    );
    expect(
      document.querySelector<HTMLElement>(".change-card-selected")?.dataset
        .changeId,
    ).toBe("change:sha256:last");
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "g", bubbles: true }),
    );
    expect(
      document.querySelector<HTMLElement>(".change-card-selected")?.dataset
        .changeId,
    ).toBe("change:sha256:one");
  });

  it("validates profile before staging the two bounded lenses and does not fetch placeholder detail", async () => {
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isChangeDetailPath(path))
        return new Response(JSON.stringify(changeDetail()));
      if (isExactRevisionPath(path))
        return new Response(JSON.stringify(revisionDetail()));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    expect(requests).toEqual([
      "/api/v2/profile",
      "/api/identity",
      "/api/v2/changes?limit=50&order=change_id_asc",
      "/api/v2/attention?limit=50&order=change_id_asc",
      "/api/v2/profile",
    ]);
    document
      .querySelector<HTMLButtonElement>("[data-change-id] .change-card-primary")
      ?.click();
    await vi.waitFor(() =>
      expect(
        document.querySelector<HTMLButtonElement>(
          "#detail-body .detail-current-revisions button",
        ),
      ).not.toBeNull(),
    );
    document
      .querySelector<HTMLButtonElement>(
        "#detail-body .detail-current-revisions button",
      )
      ?.click();
    await vi.waitFor(() =>
      expect(requests.some(isExactRevisionPath)).toBe(true),
    );
    expect(requests.some(isChangeDetailPath)).toBe(true);
    expect(location.hash).toContain("artifactHash=sha256%3Aartifact");
  });

  it("hydrates the served identity once at bootstrap and never polls it", async () => {
    vi.useFakeTimers();
    const requests: string[] = [];
    const identity = {
      schema: "pointbreak.inspect-identity",
      storeIdentity: "store:sha256:served",
      contextIdentity: "context:sha256:served",
      repository: "served-pointbreak",
      placement: { tier: "family", label: "family store" },
      family: { id: "served-family" },
      worktree: "feat-served-pointbreak",
    };
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/identity")
        return new Response(JSON.stringify(identity));
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
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

    expect(requests.filter((path) => path === "/api/identity")).toHaveLength(1);
    expect(document.querySelector("#store-chip-repo")?.textContent).toBe(
      identity.repository,
    );
    expect(
      document.querySelector("#store-chip")?.getAttribute("aria-label"),
    ).toBe(
      "repository served-pointbreak, store family store, family served-family, worktree feat-served-pointbreak",
    );
    expect(document.querySelector("#store-identity-rows")?.textContent).toBe(
      "repositoryserved-pointbreakstorefamily storefamilyserved-familyworktreefeat-served-pointbreak",
    );
    expect(document.title).toBe("served-pointbreak · Pointbreak Review");

    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() =>
      expect(
        requests.filter((path) => path === "/api/v2/profile"),
      ).toHaveLength(3),
    );
    expect(requests.filter((path) => path === "/api/identity")).toHaveLength(1);
  });

  it("does not let a hung identity request gate semantic paint or poll installation", async () => {
    vi.useFakeTimers();
    let identityResolve!: (response: Response) => void;
    const identityResponse = new Promise<Response>((resolve) => {
      identityResolve = resolve;
    });
    let profileRequests = 0;
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") return identityResponse;
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        return Promise.resolve(new Response(JSON.stringify(profile)));
      }
      if (path.startsWith("/api/v2/changes?"))
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );

    const bootstrap = bootstrapChangeInspector();
    try {
      await vi.waitFor(() =>
        expect(
          document.querySelector(".unit-card[data-change-id]"),
        ).not.toBeNull(),
      );
      expect(profileRequests).toBe(2);
      await vi.advanceTimersByTimeAsync(3_000);
      expect(profileRequests).toBe(3);
      expect(document.querySelector("#refresh-status")?.textContent).toBe(
        "watching",
      );
    } finally {
      identityResolve(
        new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-identity",
            storeIdentity: "store:sha256:late",
            contextIdentity: "context:sha256:late",
            repository: "late-identity",
            placement: { tier: "clone", label: "clone store" },
          }),
        ),
      );
      await bootstrap;
    }
  });

  it("does not let an older bootstrap identity repaint a newer bootstrap", async () => {
    let olderIdentityResolve!: (response: Response) => void;
    const olderIdentityResponse = new Promise<Response>((resolve) => {
      olderIdentityResolve = resolve;
    });
    let identityRequests = 0;
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") {
        identityRequests += 1;
        if (identityRequests === 1) return olderIdentityResponse;
        return Promise.resolve(
          new Response(
            JSON.stringify({
              schema: "pointbreak.inspect-identity",
              storeIdentity: "store:sha256:newer",
              contextIdentity: "context:sha256:newer",
              repository: "newer-identity",
              placement: { tier: "clone", label: "clone store" },
            }),
          ),
        );
      }
      if (path === "/api/v2/profile")
        return Promise.resolve(new Response(JSON.stringify(profile)));
      if (path.startsWith("/api/v2/changes?"))
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const reader = await import("../src/change-inspector");

    const olderBootstrap = reader.bootstrapChangeInspector({ poll: false });
    await vi.waitFor(() =>
      expect(
        document.querySelector(".unit-card[data-change-id]"),
      ).not.toBeNull(),
    );
    await reader.bootstrapChangeInspector({ poll: false });
    expect(document.querySelector("#store-chip-repo")?.textContent).toBe(
      "newer-identity",
    );

    olderIdentityResolve(
      new Response(
        JSON.stringify({
          schema: "pointbreak.inspect-identity",
          storeIdentity: "store:sha256:older",
          contextIdentity: "context:sha256:older",
          repository: "older-identity",
          placement: { tier: "clone", label: "clone store" },
        }),
      ),
    );
    await olderBootstrap;

    expect(document.querySelector("#store-chip-repo")?.textContent).toBe(
      "newer-identity",
    );
    expect(document.title).toBe("newer-identity · Pointbreak Review");
  });

  it("retains the last verified identity and generation across a failed poll and retry", async () => {
    vi.useFakeTimers();
    const identity = {
      schema: "pointbreak.inspect-identity",
      storeIdentity: "store:sha256:stable",
      contextIdentity: "context:sha256:stable",
      repository: "stable-pointbreak",
      placement: { tier: "clone", label: "clone store" },
    };
    let failPoll = false;
    let failIdentity = false;
    let identityRequests = 0;
    let changesRequests = 0;
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") {
        identityRequests += 1;
        if (failIdentity) {
          return new Response(JSON.stringify({ error: "unavailable" }), {
            status: 500,
          });
        }
        return new Response(JSON.stringify(identity));
      }
      if (path === "/api/v2/profile") {
        if (failPoll) return new Response("not a profile", { status: 500 });
        return new Response(JSON.stringify(profile));
      }
      if (path.startsWith("/api/v2/changes?")) {
        changesRequests += 1;
        return new Response(JSON.stringify(page("changes")));
      }
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );

    await bootstrapChangeInspector();
    const publishedHash = document.querySelector("#stat-hash")?.textContent;
    failPoll = true;
    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() =>
      expect(document.querySelector("#refresh-status")?.textContent).toBe(
        "response error",
      ),
    );
    expect(document.querySelector(".unit-card[data-change-id]")).not.toBeNull();
    expect(document.querySelector("#stat-hash")?.textContent).toBe(
      publishedHash,
    );
    expect(document.querySelector("#store-chip-repo")?.textContent).toBe(
      identity.repository,
    );

    failPoll = false;
    failIdentity = true;
    document.querySelector<HTMLButtonElement>("#connection-action")?.click();
    await vi.waitFor(() => expect(identityRequests).toBe(2));
    expect(changesRequests).toBe(1);
    expect(document.querySelector("#stat-hash")?.textContent).toBe(
      publishedHash,
    );
    expect(document.querySelector("#store-chip-repo")?.textContent).toBe(
      identity.repository,
    );
    expect(document.querySelector("#refresh-status")?.textContent).toBe(
      "response error",
    );

    failIdentity = false;
    document.querySelector<HTMLButtonElement>("#connection-action")?.click();
    await vi.waitFor(() => expect(changesRequests).toBe(2));
    expect(document.querySelector(".unit-card[data-change-id]")).not.toBeNull();
    expect(document.querySelector("#store-chip-repo")?.textContent).toBe(
      identity.repository,
    );
  });

  it("reports accepted poll liveness only after a coherent stage and never degrades for a retried mismatch", async () => {
    vi.useFakeTimers();
    let generation = 1;
    let profileRequests = 0;
    let releaseChangedPostflight!: () => void;
    const changedPostflight = new Promise<void>((resolve) => {
      releaseChangedPostflight = resolve;
    });
    let holdChangedPostflight = false;
    let mismatchPostflight = false;
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity")
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-identity",
            storeIdentity: "store:sha256:liveness",
            contextIdentity: "context:sha256:liveness",
            repository: "liveness-pointbreak",
            placement: { tier: "clone", label: "clone store" },
          }),
        );
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        if (holdChangedPostflight && profileRequests === 4) {
          await changedPostflight;
        }
        if (mismatchPostflight && profileRequests === 7) {
          return new Response(
            JSON.stringify({
              ...profile,
              authorityCursor: authorityCursor(2),
            }),
          );
        }
        return new Response(
          JSON.stringify({
            ...profile,
            authorityCursor: authorityCursor(generation),
          }),
        );
      }
      const stamp = `sha256:generation-${generation}`;
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes", stamp)));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention", stamp)));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );

    await bootstrapChangeInspector();
    expect(document.querySelector("#refresh-status")?.textContent).toBe(
      "watching",
    );

    generation = 2;
    holdChangedPostflight = true;
    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() => expect(profileRequests).toBe(4));
    expect(document.querySelector("#stat-hash")?.textContent).toBe(
      "sha256:generation-1",
    );
    expect(document.querySelector("#refresh-status")?.textContent).toBe(
      "watching",
    );
    releaseChangedPostflight();
    await vi.waitFor(() =>
      expect(document.querySelector("#stat-hash")?.textContent).toBe(
        "sha256:generation-2",
      ),
    );
    expect(document.querySelector("#refresh-status")?.textContent).toBe(
      "updated",
    );

    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() => expect(profileRequests).toBe(5));
    expect(document.querySelector("#refresh-status")?.textContent).toBe(
      "watching",
    );

    generation = 3;
    mismatchPostflight = true;
    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() => expect(profileRequests).toBe(9));
    expect(document.querySelector("#refresh-status")?.textContent).not.toBe(
      "response error",
    );
  });

  it("does not publish a poll generation after its credential session changes", async () => {
    vi.useFakeTimers();
    let generation = 1;
    let profileRequests = 0;
    let releasePollProfile!: (response: Response) => void;
    const pollProfile = new Promise<Response>((resolve) => {
      releasePollProfile = resolve;
    });
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") {
        return Promise.resolve(
          new Response(
            JSON.stringify({
              schema: "pointbreak.inspect-identity",
              storeIdentity: "store:sha256:credential",
              contextIdentity: "context:sha256:credential",
              repository: "credential-pointbreak",
              placement: { tier: "clone", label: "clone store" },
            }),
          ),
        );
      }
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        if (profileRequests === 3) return pollProfile;
        return Promise.resolve(
          new Response(
            JSON.stringify({
              ...profile,
              authorityCursor: authorityCursor(generation),
            }),
          ),
        );
      }
      const stamp = `sha256:generation-${generation}`;
      if (path.startsWith("/api/v2/changes?")) {
        return Promise.resolve(
          new Response(JSON.stringify(page("changes", stamp))),
        );
      }
      if (path.startsWith("/api/v2/attention?")) {
        return Promise.resolve(
          new Response(JSON.stringify(page("attention", stamp))),
        );
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const reader = await import("../src/change-inspector");
    const auth = await import("../src/auth");

    await reader.bootstrapChangeInspector();
    await vi.waitFor(() =>
      expect(document.querySelector("#store-chip-repo")?.textContent).toBe(
        "credential-pointbreak",
      ),
    );
    const acceptedHash = document.querySelector("#stat-hash")?.textContent;

    generation = 2;
    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() => expect(profileRequests).toBe(3));
    auth.setSessionToken("rotated-session-token");
    releasePollProfile(
      new Response(
        JSON.stringify({
          ...profile,
          authorityCursor: authorityCursor(generation),
        }),
      ),
    );

    await vi.waitFor(() =>
      expect(document.querySelector("#refresh-status")?.textContent).toBe(
        "response error",
      ),
    );
    expect(document.querySelector("#stat-hash")?.textContent).toBe(
      acceptedHash,
    );
    expect(document.querySelector("#store-chip-repo")?.textContent).toBe(
      "credential-pointbreak",
    );
    auth.resetAuthForTests();
  });

  it("restarts an initial semantic load and identity read after the credential session changes", async () => {
    let releaseIdentity!: (response: Response) => void;
    let releaseInitialProfile!: (response: Response) => void;
    const initialIdentity = new Promise<Response>((resolve) => {
      releaseIdentity = resolve;
    });
    const initialProfile = new Promise<Response>((resolve) => {
      releaseInitialProfile = resolve;
    });
    let identityRequests = 0;
    let profileRequests = 0;
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") {
        identityRequests += 1;
        if (identityRequests === 1) return initialIdentity;
        return Promise.resolve(
          new Response(
            JSON.stringify({
              schema: "pointbreak.inspect-identity",
              storeIdentity: "store:sha256:new-session",
              contextIdentity: "context:sha256:new-session",
              repository: "new-session-pointbreak",
              placement: { tier: "clone", label: "clone store" },
            }),
          ),
        );
      }
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        if (profileRequests === 1) return initialProfile;
        const generation = profileRequests <= 2 ? 1 : 2;
        return Promise.resolve(
          new Response(
            JSON.stringify({
              ...profile,
              authorityCursor: authorityCursor(generation),
            }),
          ),
        );
      }
      const generation = profileRequests <= 2 ? 1 : 2;
      const stamp = `sha256:generation-${generation}`;
      if (path.startsWith("/api/v2/changes?")) {
        return Promise.resolve(
          new Response(JSON.stringify(page("changes", stamp))),
        );
      }
      if (path.startsWith("/api/v2/attention?")) {
        return Promise.resolve(
          new Response(JSON.stringify(page("attention", stamp))),
        );
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const reader = await import("../src/change-inspector");
    const auth = await import("../src/auth");

    const bootstrap = reader.bootstrapChangeInspector({ poll: false });
    await vi.waitFor(() => {
      expect(identityRequests).toBe(1);
      expect(profileRequests).toBe(1);
    });
    auth.setSessionToken("new-session-token");
    releaseIdentity(
      new Response(
        JSON.stringify({
          schema: "pointbreak.inspect-identity",
          storeIdentity: "store:sha256:old-session",
          contextIdentity: "context:sha256:old-session",
          repository: "old-session-pointbreak",
          placement: { tier: "clone", label: "clone store" },
        }),
      ),
    );
    releaseInitialProfile(
      new Response(
        JSON.stringify({
          ...profile,
          authorityCursor: authorityCursor(1),
        }),
      ),
    );
    await bootstrap;

    expect(identityRequests).toBe(2);
    expect(profileRequests).toBe(4);
    expect(document.querySelector("#stat-hash")?.textContent).toBe(
      "sha256:generation-2",
    );
    expect(document.querySelector("#store-chip-repo")?.textContent).toBe(
      "new-session-pointbreak",
    );
    auth.resetAuthForTests();
  });

  it("revalidates identity when session A identity publishes before session B semantics", async () => {
    let releaseInitialProfile!: (response: Response) => void;
    const initialProfile = new Promise<Response>((resolve) => {
      releaseInitialProfile = resolve;
    });
    let identityRequests = 0;
    let profileRequests = 0;
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") {
        identityRequests += 1;
        const session = identityRequests === 1 ? "a" : "b";
        return Promise.resolve(
          new Response(
            JSON.stringify({
              schema: "pointbreak.inspect-identity",
              storeIdentity: `store:sha256:session-${session}`,
              contextIdentity: `context:sha256:session-${session}`,
              repository: `session-${session}-pointbreak`,
              placement: { tier: "clone", label: "clone store" },
            }),
          ),
        );
      }
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        if (profileRequests === 1) return initialProfile;
        const generation = profileRequests <= 2 ? 1 : 2;
        return Promise.resolve(
          new Response(
            JSON.stringify({
              ...profile,
              authorityCursor: authorityCursor(generation),
            }),
          ),
        );
      }
      const generation = profileRequests <= 2 ? 1 : 2;
      const stamp = `sha256:generation-${generation}`;
      if (path.startsWith("/api/v2/changes?")) {
        return Promise.resolve(
          new Response(JSON.stringify(page("changes", stamp))),
        );
      }
      if (path.startsWith("/api/v2/attention?")) {
        return Promise.resolve(
          new Response(JSON.stringify(page("attention", stamp))),
        );
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const reader = await import("../src/change-inspector");
    const auth = await import("../src/auth");

    const bootstrap = reader.bootstrapChangeInspector({ poll: false });
    await vi.waitFor(() => {
      expect(profileRequests).toBe(1);
      expect(document.querySelector("#store-chip-repo")?.textContent).toBe(
        "session-a-pointbreak",
      );
    });
    auth.setSessionToken("session-b-token");
    releaseInitialProfile(
      new Response(
        JSON.stringify({
          ...profile,
          authorityCursor: authorityCursor(1),
        }),
      ),
    );
    await bootstrap;

    expect(identityRequests).toBe(2);
    expect(profileRequests).toBe(4);
    expect(document.querySelector("#stat-hash")?.textContent).toBe(
      "sha256:generation-2",
    );
    expect(document.querySelector("#store-chip-repo")?.textContent).toBe(
      "session-b-pointbreak",
    );
    auth.resetAuthForTests();
  });

  it("does not expose session B identity over an accepted session A generation", async () => {
    let releaseInitialIdentity!: (response: Response) => void;
    const initialIdentity = new Promise<Response>((resolve) => {
      releaseInitialIdentity = resolve;
    });
    let identityRequests = 0;
    let profileRequests = 0;
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") {
        identityRequests += 1;
        if (identityRequests === 1) return initialIdentity;
        return Promise.resolve(
          new Response(
            JSON.stringify({
              schema: "pointbreak.inspect-identity",
              storeIdentity: "store:sha256:session-b",
              contextIdentity: "context:sha256:session-b",
              repository: "session-b-pointbreak",
              placement: { tier: "clone", label: "clone store" },
            }),
          ),
        );
      }
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        const generation = profileRequests <= 2 ? 1 : 2;
        return Promise.resolve(
          new Response(
            JSON.stringify({
              ...profile,
              authorityCursor: authorityCursor(generation),
            }),
          ),
        );
      }
      const generation = profileRequests <= 2 ? 1 : 2;
      const stamp = `sha256:generation-${generation}`;
      if (path.startsWith("/api/v2/changes?")) {
        return Promise.resolve(
          new Response(JSON.stringify(page("changes", stamp))),
        );
      }
      if (path.startsWith("/api/v2/attention?")) {
        return Promise.resolve(
          new Response(JSON.stringify(page("attention", stamp))),
        );
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const reader = await import("../src/change-inspector");
    const auth = await import("../src/auth");

    await reader.bootstrapChangeInspector({ poll: false });
    expect(document.querySelector("#stat-hash")?.textContent).toBe(
      "sha256:generation-1",
    );
    expect(document.querySelector("#store-chip-repo")?.textContent).toBe(
      "local server",
    );

    auth.setSessionToken("session-b-token");
    releaseInitialIdentity(
      new Response(
        JSON.stringify({
          schema: "pointbreak.inspect-identity",
          storeIdentity: "store:sha256:session-a",
          contextIdentity: "context:sha256:session-a",
          repository: "session-a-pointbreak",
          placement: { tier: "clone", label: "clone store" },
        }),
      ),
    );
    await vi.waitFor(() => expect(identityRequests).toBe(2));
    for (let turn = 0; turn < 8; turn += 1) await Promise.resolve();

    expect(document.querySelector("#stat-hash")?.textContent).toBe(
      "sha256:generation-1",
    );
    expect(document.querySelector("#store-chip-repo")?.textContent).toBe(
      "local server",
    );

    history.replaceState(null, "", "/#/changes?q=session-b");
    window.dispatchEvent(new Event("hashchange"));
    await vi.waitFor(() => {
      expect(document.querySelector("#stat-hash")?.textContent).toBe(
        "sha256:generation-2",
      );
      expect(document.querySelector("#store-chip-repo")?.textContent).toBe(
        "session-b-pointbreak",
      );
    });
    auth.resetAuthForTests();
  });

  it("does not let an old poll timeout invalidate a newer route load", async () => {
    vi.useFakeTimers();
    let profileRequests = 0;
    let newerRouteProfileResolve!: (response: Response) => void;
    let markPollStarted!: () => void;
    let markNewerRouteStarted!: () => void;
    const pollStarted = new Promise<void>((resolve) => {
      markPollStarted = resolve;
    });
    const newerRouteStarted = new Promise<void>((resolve) => {
      markNewerRouteStarted = resolve;
    });
    const requests: string[] = [];
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/identity")
        return Promise.resolve(
          new Response(
            JSON.stringify({
              schema: "pointbreak.inspect-identity",
              storeIdentity: "store:sha256:timeout",
              contextIdentity: "context:sha256:timeout",
              repository: "timeout-pointbreak",
              placement: { tier: "clone", label: "clone store" },
            }),
          ),
        );
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        if (profileRequests === 3) {
          markPollStarted();
          return new Promise<Response>(() => {});
        }
        if (profileRequests === 4) {
          markNewerRouteStarted();
          return new Promise<Response>((resolve) => {
            newerRouteProfileResolve = resolve;
          });
        }
        return Promise.resolve(new Response(JSON.stringify(profile)));
      }
      if (path.startsWith("/api/v2/changes?")) {
        const document = page("changes");
        return Promise.resolve(
          new Response(
            JSON.stringify({
              ...document,
              changes: path.includes("q=newer")
                ? [
                    {
                      ...document.changes[0],
                      changeId: "change:sha256:newer-route",
                    },
                  ]
                : document.changes,
            }),
          ),
        );
      }
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();

    let expireOldPoll: () => void = () => {
      throw new Error("poll timeout was not installed");
    };
    const timeout = vi
      .spyOn(globalThis, "setTimeout")
      .mockImplementation((handler, delay) => {
        if (delay === 15_000 && typeof handler === "function") {
          expireOldPoll = handler;
        }
        return 1 as unknown as ReturnType<typeof setTimeout>;
      });
    await vi.advanceTimersByTimeAsync(3_000);
    await pollStarted;
    history.replaceState(null, "", "/#/changes?q=newer");
    window.dispatchEvent(new Event("hashchange"));
    await newerRouteStarted;

    expireOldPoll();
    for (let turn = 0; turn < 8; turn += 1) await Promise.resolve();
    expect
      .soft(document.querySelector("#refresh-status")?.textContent)
      .toBe("watching");
    timeout.mockRestore();
    newerRouteProfileResolve(new Response(JSON.stringify(profile)));
    await vi.waitFor(() =>
      expect(requests).toContain(
        "/api/v2/changes?limit=50&q=newer&order=change_id_asc",
      ),
    );

    expect(requests).toContain(
      "/api/v2/changes?limit=50&q=newer&order=change_id_asc",
    );
    expect(document.querySelector("#master")?.textContent).toContain(
      "change:sha256:newer-route",
    );
  });

  it("preserves an accepted exact surface when poll hydration fails", async () => {
    vi.useFakeTimers();
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let generation = 1;
    let exactRequests = 0;
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity")
        return new Response(
          JSON.stringify({
            schema: "pointbreak.inspect-identity",
            storeIdentity: "store:sha256:exact",
            contextIdentity: "context:sha256:exact",
            repository: "exact-pointbreak",
            placement: { tier: "clone", label: "clone store" },
          }),
        );
      const stamp = `sha256:generation-${generation}`;
      if (path === "/api/v2/profile")
        return new Response(
          JSON.stringify({
            ...profile,
            authorityCursor: authorityCursor(generation),
          }),
        );
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes", stamp)));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention", stamp)));
      if (isExactRevisionPath(path)) {
        exactRequests += 1;
        return exactRequests === 1
          ? new Response(JSON.stringify(revisionDetail(stamp)))
          : new Response(JSON.stringify({ error: "hydration unavailable" }), {
              status: 500,
            });
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    const detail = document.querySelector<HTMLElement>("#detail-body");
    const acceptedReadingKey = detail?.dataset.changeReadingKey;
    expect(acceptedReadingKey).toContain("sha256:generation-1");
    expect(detail?.textContent).toContain("Exact Revision");

    generation = 2;
    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() => expect(exactRequests).toBe(2));

    expect(detail?.dataset.changeReadingKey).toBe(acceptedReadingKey);
    expect(detail?.textContent).toContain("Exact Revision");
    expect(detail?.textContent).not.toContain(
      "Reader refused this exact surface",
    );
    expect(document.querySelector("#refresh-status")?.textContent).toBe(
      "response error",
    );
    expect(document.querySelector("#refresh")?.getAttribute("data-state")).toBe(
      "degraded",
    );
  });

  it("refuses an invalid route without fetching any semantic document", async () => {
    history.replaceState(null, "", "/#/changes?unknown=value");
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      requests.push(String(input));
      throw new Error("invalid routes must not fetch");
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );

    await bootstrapChangeInspector({ poll: false });

    expect(requests).toEqual(["/api/identity"]);
    expect(document.querySelector("#route-diagnostic")?.textContent).toContain(
      "Unknown unknown route query.",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Unknown unknown route query.",
    );
  });

  it("clears the old generation for a new query while its profile-first replacement is pending", async () => {
    let replacementProfileResolve!: (value: Response) => void;
    let profileRequests = 0;
    const requests: string[] = [];
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        if (profileRequests === 3) {
          return new Promise<Response>((resolve) => {
            replacementProfileResolve = resolve;
          });
        }
        return Promise.resolve(new Response(JSON.stringify(profile)));
      }
      if (path.startsWith("/api/v2/changes?"))
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      if (isExactRevisionPath(path))
        return Promise.resolve(new Response(JSON.stringify(revisionDetail())));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    expect(document.querySelector("#master")?.textContent).toContain(
      "change:sha256:one",
    );

    location.hash = "#/changes?q=replacement";
    window.dispatchEvent(new Event("hashchange"));
    await vi.waitFor(() => {
      expect(document.querySelector("#master")?.textContent).toContain(
        "Loading Change generation",
      );
    });
    expect(requests).toHaveLength(6);
    expect(requests.slice(5)).toEqual(["/api/v2/profile"]);

    replacementProfileResolve(new Response(JSON.stringify(profile)));
    await vi.waitFor(() => {
      expect(requests).toContain(
        "/api/v2/changes?limit=50&q=replacement&order=change_id_asc",
      );
    });
  });

  it("withholds a parked Timeline page from interaction while its replacement generation loads", async () => {
    history.replaceState(null, "", "/#/timeline?limit=20");
    let replacementProfileResolve!: (value: Response) => void;
    let profileRequests = 0;
    const syncCalls: Array<{
      snapshot: ChangeInspectorSnapshot;
      timelinePage: EventHistoryDocument | null | undefined;
    }> = [];
    vi.doMock("../src/change-inspector-interaction", () => ({
      installChangeInspectorInteraction: () => ({
        sync(
          snapshot: ChangeInspectorSnapshot,
          timelinePage?: EventHistoryDocument | null,
        ) {
          syncCalls.push({ snapshot, timelinePage });
        },
        stop() {},
      }),
    }));
    try {
      globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
        const path = String(input);
        if (path === "/api/v2/profile") {
          profileRequests += 1;
          if (profileRequests === 3) {
            return new Promise<Response>((resolve) => {
              replacementProfileResolve = resolve;
            });
          }
          return Promise.resolve(new Response(JSON.stringify(profile)));
        }
        if (path.startsWith("/api/v2/changes?"))
          return Promise.resolve(new Response(JSON.stringify(page("changes"))));
        if (path.startsWith("/api/v2/attention?"))
          return Promise.resolve(
            new Response(JSON.stringify(page("attention"))),
          );
        if (path.startsWith("/api/v2/history?"))
          return Promise.resolve(new Response(JSON.stringify(historyPage())));
        throw new Error(`unexpected ${path}`);
      }) as typeof fetch;
      const { bootstrapChangeInspector } = await import(
        "../src/change-inspector"
      );
      await bootstrapChangeInspector({ poll: false });

      history.replaceState(null, "", "/#/timeline?limit=20&q=replacement");
      window.dispatchEvent(new Event("hashchange"));
      let loadingCall:
        | {
            snapshot: ChangeInspectorSnapshot;
            timelinePage: EventHistoryDocument | null | undefined;
          }
        | undefined;
      await vi.waitFor(() => {
        loadingCall = syncCalls.find(
          ({ snapshot }) =>
            snapshot.generation === null &&
            snapshot.route.kind === "timeline" &&
            snapshot.route.historyQuery.q === "replacement",
        );
        expect(loadingCall).toBeDefined();
      });
      expect(loadingCall?.timelinePage).toBeNull();

      replacementProfileResolve(new Response(JSON.stringify(profile)));
      await vi.waitFor(() =>
        expect(
          syncCalls.some(
            ({ snapshot, timelinePage }) =>
              snapshot.generation !== null &&
              snapshot.route.kind === "timeline" &&
              snapshot.route.historyQuery.q === "replacement" &&
              timelinePage !== null,
          ),
        ).toBe(true),
      );
    } finally {
      vi.doUnmock("../src/change-inspector-interaction");
    }
  });

  it("traverses a fresh authoritative generation while presentation remains parked", async () => {
    history.replaceState(null, "", "/#/timeline?limit=1&order=desc");
    const currentProfile = {
      ...profile,
      authorityCursor: authorityCursor(2),
    };
    const currentHead = boundaryHistoryPage({
      eventIds: ["evt:current-head"],
      next: "tail-token",
      offset: 0,
    });
    const currentTail = boundaryHistoryPage({
      eventIds: ["evt:current-tail"],
      offset: 1,
    });
    const parkedDisplay = boundaryHistoryPage({
      authoritySequence: 1,
      eventIds: ["evt:parked-head"],
      offset: 0,
      timelineProjectionStamp: "sha256:timeline-parked",
    });
    const parkedSnapshot = {
      mode: "parked" as const,
      newCount: 1,
      display: parkedDisplay,
    };
    vi.doMock("../src/change-inspector-timeline-monitor", () => ({
      createTimelineMonitor: () => ({
        observe: () => parkedSnapshot,
        toggle: () => parkedSnapshot,
        park: () => parkedSnapshot,
        follow: () => parkedSnapshot,
        snapshot: () => parkedSnapshot,
      }),
    }));
    try {
      globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
        const path = String(input);
        if (path === "/api/v2/profile") {
          return new Response(JSON.stringify(currentProfile));
        }
        if (path.startsWith("/api/v2/changes?")) {
          return new Response(JSON.stringify(page("changes")));
        }
        if (path.startsWith("/api/v2/attention?")) {
          return new Response(JSON.stringify(page("attention")));
        }
        if (path.startsWith("/api/v2/history?")) {
          const query = new URL(path, "https://pointbreak.invalid")
            .searchParams;
          return new Response(
            JSON.stringify(
              query.get("after") === "tail-token" ? currentTail : currentHead,
            ),
          );
        }
        throw new Error(`unexpected ${path}`);
      }) as typeof fetch;
      const { bootstrapChangeInspector } = await import(
        "../src/change-inspector"
      );
      await bootstrapChangeInspector({ poll: false });

      const list = document.querySelector<HTMLOListElement>("#timeline");
      list?.focus();
      list?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "G", bubbles: true }),
      );

      await vi.waitFor(() => expect(location.hash).toContain("tail-token"));
      expect(document.querySelector("#error")?.textContent).not.toContain(
        "Reader refused",
      );
    } finally {
      vi.doUnmock("../src/change-inspector-timeline-monitor");
    }
  });

  it("keeps a healthy poll tick behind a current global Timeline traversal", async () => {
    vi.useFakeTimers();
    history.replaceState(null, "", "/#/timeline?limit=1&order=desc");
    const currentProfile = {
      ...profile,
      authorityCursor: authorityCursor(2),
    };
    const currentHead = boundaryHistoryPage({
      eventIds: ["evt:current-head"],
      next: "tail-token",
      offset: 0,
    });
    const currentTail = boundaryHistoryPage({
      eventIds: ["evt:current-tail"],
      offset: 1,
    });
    let profileRequests = 0;
    let changesRequests = 0;
    let attentionRequests = 0;
    let historyRequests = 0;
    const profileRequestTimes: number[] = [];
    let traversalTailDeferred = false;
    let resolveTraversalTail!: (response: Response) => void;
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") {
        return Promise.resolve(
          new Response(
            JSON.stringify({
              schema: "pointbreak.inspect-identity",
              storeIdentity: "store:sha256:timeline-poll",
              contextIdentity: "context:sha256:timeline-poll",
              repository: "timeline-poll-pointbreak",
              placement: { tier: "clone", label: "clone store" },
            }),
          ),
        );
      }
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        profileRequestTimes.push(Date.now());
        return Promise.resolve(new Response(JSON.stringify(currentProfile)));
      }
      if (path.startsWith("/api/v2/changes?")) {
        changesRequests += 1;
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      }
      if (path.startsWith("/api/v2/attention?")) {
        attentionRequests += 1;
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      }
      if (path.startsWith("/api/v2/history?")) {
        historyRequests += 1;
        const query = new URL(path, "https://pointbreak.invalid").searchParams;
        if (query.get("after") === "tail-token" && !traversalTailDeferred) {
          traversalTailDeferred = true;
          return new Promise<Response>((resolve) => {
            resolveTraversalTail = resolve;
          });
        }
        return Promise.resolve(
          new Response(
            JSON.stringify(
              query.get("after") === "tail-token" ? currentTail : currentHead,
            ),
          ),
        );
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();

    const list = document.querySelector<HTMLOListElement>("#timeline");
    list?.focus();
    list?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "G", bubbles: true }),
    );
    await vi.waitFor(() => {
      expect(profileRequests).toBe(3);
      expect(historyRequests).toBe(3);
    });

    await vi.advanceTimersByTimeAsync(3_000);
    expect(profileRequests).toBe(3);
    expect(changesRequests).toBe(1);
    expect(attentionRequests).toBe(1);
    await vi.advanceTimersByTimeAsync(3_000);
    expect(profileRequests).toBe(3);

    const traversalReleasedAt = Date.now();
    resolveTraversalTail(new Response(JSON.stringify(currentTail)));
    await vi.waitFor(() => expect(location.hash).toContain("tail-token"));
    await vi.waitFor(() => expect(profileRequests).toBe(6));
    await vi.waitFor(() =>
      expect(
        document
          .querySelector("#timeline [aria-selected='true']")
          ?.getAttribute("data-event-id"),
      ).toBe("evt:current-tail"),
    );
    expect(historyRequests).toBe(4);

    await vi.advanceTimersByTimeAsync(3_000);
    expect(profileRequests).toBe(7);
    expect(
      (profileRequestTimes.at(-1) ?? 0) - traversalReleasedAt,
    ).toBeGreaterThanOrEqual(3_000);
    expect(changesRequests).toBe(2);
    expect(attentionRequests).toBe(2);
  });

  it("lifts Timeline traversal poll suppression after a later route epoch", async () => {
    vi.useFakeTimers();
    history.replaceState(null, "", "/#/timeline?limit=1&order=desc");
    const currentProfile = {
      ...profile,
      authorityCursor: authorityCursor(2),
    };
    const currentHead = boundaryHistoryPage({
      eventIds: ["evt:current-head"],
      next: "tail-token",
      offset: 0,
    });
    const currentTail = boundaryHistoryPage({
      eventIds: ["evt:current-tail"],
      offset: 1,
    });
    let profileRequests = 0;
    let changesRequests = 0;
    let attentionRequests = 0;
    let historyRequests = 0;
    let traversalPageSettled = false;
    let traversalTailDeferred = false;
    let resolveTraversalTail!: (response: Response) => void;
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") {
        return Promise.resolve(
          new Response(
            JSON.stringify({
              schema: "pointbreak.inspect-identity",
              storeIdentity: "store:sha256:timeline-route",
              contextIdentity: "context:sha256:timeline-route",
              repository: "timeline-route-pointbreak",
              placement: { tier: "clone", label: "clone store" },
            }),
          ),
        );
      }
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        return Promise.resolve(new Response(JSON.stringify(currentProfile)));
      }
      if (path.startsWith("/api/v2/changes?")) {
        changesRequests += 1;
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      }
      if (path.startsWith("/api/v2/attention?")) {
        attentionRequests += 1;
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      }
      if (path.startsWith("/api/v2/history?")) {
        historyRequests += 1;
        const query = new URL(path, "https://pointbreak.invalid").searchParams;
        if (query.get("after") === "tail-token" && !traversalTailDeferred) {
          traversalTailDeferred = true;
          return new Promise<Response>((resolve) => {
            resolveTraversalTail = (response) => {
              traversalPageSettled = true;
              resolve(response);
            };
          });
        }
        return Promise.resolve(
          new Response(
            JSON.stringify(
              query.get("after") === "tail-token" ? currentTail : currentHead,
            ),
          ),
        );
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();

    const list = document.querySelector<HTMLOListElement>("#timeline");
    list?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "G", bubbles: true }),
    );
    await vi.waitFor(() => {
      expect(profileRequests).toBe(3);
      expect(historyRequests).toBe(3);
    });

    history.replaceState(null, "", "/#/changes?q=next-route");
    window.dispatchEvent(new Event("hashchange"));
    await vi.waitFor(() => {
      expect(profileRequests).toBe(5);
      expect(document.querySelector("#master")?.textContent).toContain(
        "change:sha256:one",
      );
    });
    const routeChangesRequests = changesRequests;
    const routeAttentionRequests = attentionRequests;

    await vi.advanceTimersByTimeAsync(3_000);
    expect(traversalPageSettled).toBe(false);
    expect(profileRequests).toBe(6);
    expect(changesRequests).toBe(routeChangesRequests);
    expect(attentionRequests).toBe(routeAttentionRequests);

    resolveTraversalTail(new Response(JSON.stringify(currentTail)));
    await Promise.resolve();
    await Promise.resolve();
    expect(location.hash).toBe("#/changes?q=next-route");
  });

  it("keeps a superseded global Timeline boundary completion inert", async () => {
    history.replaceState(null, "", "/#/timeline?limit=1&order=desc");
    const currentProfile = {
      ...profile,
      authorityCursor: authorityCursor(2),
    };
    const currentHead = boundaryHistoryPage({
      eventIds: ["evt:current-head"],
      next: "tail-token",
      offset: 0,
    });
    const currentTail = boundaryHistoryPage({
      eventIds: ["evt:current-tail"],
      offset: 1,
    });
    let profileRequests = 0;
    let resolveFirstBoundaryPreflight!: (response: Response) => void;
    const historyRequests: string[] = [];
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        if (profileRequests === 3) {
          return new Promise<Response>((resolve) => {
            resolveFirstBoundaryPreflight = resolve;
          });
        }
        return Promise.resolve(new Response(JSON.stringify(currentProfile)));
      }
      if (path.startsWith("/api/v2/changes?"))
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      if (path.startsWith("/api/v2/history?")) {
        historyRequests.push(path);
        const query = new URL(path, "https://pointbreak.invalid").searchParams;
        return Promise.resolve(
          new Response(
            JSON.stringify(
              query.get("after") === "tail-token" ? currentTail : currentHead,
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

    const list = document.querySelector<HTMLOListElement>("#timeline");
    list?.focus();
    list?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "G", bubbles: true }),
    );
    await vi.waitFor(() => expect(profileRequests).toBe(3));

    list?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "G", bubbles: true }),
    );
    await vi.waitFor(() => expect(location.hash).toContain("tail-token"));
    await vi.waitFor(() => expect(profileRequests).toBeGreaterThanOrEqual(7));
    const settledHash = location.hash;
    const settledHistoryRequests = [...historyRequests];

    resolveFirstBoundaryPreflight(new Response(JSON.stringify(currentProfile)));
    await vi.waitFor(() => expect(location.hash).toBe(settledHash));

    expect(historyRequests).toEqual(settledHistoryRequests);
    expect(document.querySelector("#detail-body")?.textContent).not.toContain(
      "Reader refused",
    );
  });

  it("reuses a coherent generation for exact navigation with the same query", async () => {
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isExactRevisionPath(path))
        return new Response(JSON.stringify(revisionDetail()));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    location.hash =
      "#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact";
    window.dispatchEvent(new Event("hashchange"));
    await vi.waitFor(() => expect(requests).toHaveLength(6));

    expect(requests).toHaveLength(6);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Loading exact Revision…",
    );
  });

  const stagedPageProfile = { ...profile, authorityCursor: authorityCursor(3) };
  const stagedPage = (): EventHistoryDocument =>
    boundaryHistoryPage({
      authoritySequence: 3,
      eventIds: ["evt:one", "evt:two", "evt:three"],
      offset: 0,
    });
  const routeTo = (hash: string): void => {
    location.hash = hash;
    window.dispatchEvent(new Event("hashchange"));
  };
  const detailEventId = (): string | null | undefined =>
    document.querySelector("#detail-body [data-event-id]")?.textContent;

  it("reuses the loaded history page for an exact event already on it", async () => {
    history.replaceState(
      null,
      "",
      "/#/timeline/events/evt%3Aone?q=review&limit=20",
    );
    const requests = serveComposition(stagedPage(), stagedPageProfile);
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    await vi.waitFor(() => expect(detailEventId()).toBe("evt:one"));
    const loaded = requests.length;

    routeTo("#/timeline/events/evt%3Atwo?q=review&limit=20");

    await vi.waitFor(() => expect(detailEventId()).toBe("evt:two"));
    expect(requests.slice(loaded)).toEqual([]);
    expect(
      document
        .querySelector('#timeline [aria-selected="true"]')
        ?.getAttribute("data-event-id"),
    ).toBe("evt:two");
  });

  it("re-centers the history page on an exact event the loaded page does not carry", async () => {
    history.replaceState(
      null,
      "",
      "/#/timeline/events/evt%3Aone?q=review&limit=20",
    );
    const requests = serveComposition(stagedPage(), stagedPageProfile);
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    await vi.waitFor(() => expect(detailEventId()).toBe("evt:one"));
    const loaded = requests.length;

    routeTo("#/timeline/events/evt%3Afour?q=review&limit=20");

    await vi.waitFor(() =>
      expect(
        requests
          .slice(loaded)
          .filter((path) => path.startsWith("/api/v2/history?")),
      ).toHaveLength(1),
    );
    expect(
      requests
        .slice(loaded)
        .find((path) => path.startsWith("/api/v2/history?")),
    ).toContain("at=evt%3Afour");
  });

  it("re-centers the history page when an exact event arrives under different filters", async () => {
    history.replaceState(
      null,
      "",
      "/#/timeline/events/evt%3Aone?q=review&limit=20",
    );
    const requests = serveComposition(stagedPage(), stagedPageProfile);
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    await vi.waitFor(() => expect(detailEventId()).toBe("evt:one"));
    const loaded = requests.length;

    routeTo("#/timeline/events/evt%3Atwo?q=other&limit=20");

    await vi.waitFor(() =>
      expect(
        requests
          .slice(loaded)
          .filter((path) => path.startsWith("/api/v2/history?")),
      ).toHaveLength(1),
    );
    const refetched = requests
      .slice(loaded)
      .find((path) => path.startsWith("/api/v2/history?"));
    expect(refetched).toContain("at=evt%3Atwo");
    expect(refetched).toContain("q=other");
  });

  const stagedActivationPage = (): EventHistoryDocument => {
    const base = stagedPage();
    return {
      ...base,
      completion: {
        ...base.completion,
        changeIds: ["change:sha256:one"],
        revisionRefs: [revision],
      },
      entries: base.entries.map((entry) => ({
        ...entry,
        changeIds: ["change:sha256:one"],
        revisionRefs: [revision],
      })),
    };
  };
  const pressKey = (key: string) => {
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key, bubbles: true }),
    );
  };

  it("leaves reader-held detail chrome focused while the cursor drives the detail", async () => {
    history.replaceState(
      null,
      "",
      "/#/timeline/events/evt%3Aone?q=review&limit=20",
    );
    serveComposition(stagedActivationPage(), stagedPageProfile);
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    await vi.waitFor(() => expect(detailEventId()).toBe("evt:one"));
    const close = document.querySelector<HTMLButtonElement>("#detail-close");
    close?.focus();

    pressKey("j");

    await vi.waitFor(() => expect(detailEventId()).toBe("evt:two"));
    expect(document.activeElement).toBe(close);
  });

  it("repairs focus onto the followed detail's own activation when the repaint destroys it", async () => {
    history.replaceState(
      null,
      "",
      "/#/timeline/events/evt%3Aone?q=review&limit=20",
    );
    serveComposition(stagedActivationPage(), stagedPageProfile);
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const activation = await vi.waitFor(() => {
      const control = document.querySelector<HTMLButtonElement>(
        "#detail-body [data-exact-diff-activation]",
      );
      expect(control).not.toBeNull();
      return control;
    });
    activation?.focus();
    expect(document.activeElement).toBe(activation);

    pressKey("j");

    await vi.waitFor(() => expect(detailEventId()).toBe("evt:two"));
    const followed = document.querySelector<HTMLButtonElement>(
      "#detail-body [data-exact-diff-activation]",
    );
    expect(followed).not.toBe(activation);
    expect(document.activeElement).toBe(followed);
  });

  it("focuses the exact activation when Enter opens an event from the Timeline", async () => {
    history.replaceState(null, "", "/#/timeline?q=review&limit=20");
    serveComposition(stagedActivationPage(), stagedPageProfile);
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    const list = await vi.waitFor(() => {
      const timeline = document.querySelector<HTMLOListElement>("#timeline");
      expect(timeline?.querySelectorAll("li.event").length).toBe(3);
      return timeline;
    });
    list?.focus();
    pressKey("j");
    pressKey("Enter");

    await vi.waitFor(() => expect(detailEventId()).toBe("evt:one"));
    expect(document.activeElement).toBe(
      document.querySelector("#detail-body [data-exact-diff-activation]"),
    );
  });

  const bootstrapStagedEvent = async (
    eventId: string,
    historyDocument = stagedActivationPage(),
  ) => {
    history.replaceState(
      null,
      "",
      `/#/timeline/events/${encodeURIComponent(eventId)}?q=review&limit=20`,
    );
    serveComposition(historyDocument, stagedPageProfile);
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    await vi.waitFor(() => expect(detailEventId()).toBe(eventId));
  };

  it("descends into the annotated diff from a followed exact event", async () => {
    await bootstrapStagedEvent("evt:one");

    pressKey("j");
    await vi.waitFor(() => expect(detailEventId()).toBe("evt:two"));
    pressKey("Enter");

    await vi.waitFor(() =>
      expect(parseChangeInspectorRoute(location.hash)).toEqual({
        kind: "diff",
        changeId: "change:sha256:one",
        revision,
        query: {},
      }),
    );
  });

  it("refuses to descend from a followed event with plural Changes", async () => {
    const plural = stagedActivationPage();
    await bootstrapStagedEvent("evt:one", {
      ...plural,
      completion: {
        ...plural.completion,
        changeIds: ["change:sha256:one", "change:sha256:two"],
      },
      entries: plural.entries.map((entry) => ({
        ...entry,
        changeIds: ["change:sha256:one", "change:sha256:two"],
      })),
    });

    pressKey("j");
    await vi.waitFor(() => expect(detailEventId()).toBe("evt:two"));
    const followed = parseChangeInspectorRoute(location.hash);
    pressKey("Enter");

    await vi.waitFor(() =>
      expect(document.activeElement).toBe(
        document.querySelector("[data-event-diff-refusal]"),
      ),
    );
    expect(parseChangeInspectorRoute(location.hash)).toEqual(followed);
  });

  it("descends from the last staged event when the cursor cannot move further", async () => {
    await bootstrapStagedEvent("evt:three");

    pressKey("j");
    await vi.waitFor(() => expect(detailEventId()).toBe("evt:three"));
    pressKey("Enter");

    await vi.waitFor(() =>
      expect(parseChangeInspectorRoute(location.hash)).toEqual({
        kind: "diff",
        changeId: "change:sha256:one",
        revision,
        query: {},
      }),
    );
  });

  it("polls the history page as loaded while an exact event is read", async () => {
    vi.useFakeTimers();
    history.replaceState(
      null,
      "",
      "/#/timeline/events/evt%3Aone?q=review&limit=20",
    );
    const pollProfile = { ...stagedPageProfile };
    const pollHistory = stagedPage();
    const requests = serveComposition(pollHistory, pollProfile);
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    await vi.waitFor(() => expect(detailEventId()).toBe("evt:one"));

    routeTo("#/timeline/events/evt%3Atwo?q=review&limit=20");
    await vi.waitFor(() => expect(detailEventId()).toBe("evt:two"));
    const beforePoll = requests.length;
    pollProfile.authorityCursor = authorityCursor(4);
    pollHistory.authorityCursor = authorityCursor(4);
    pollHistory.eventCount = 4;
    await vi.advanceTimersByTimeAsync(3_000);

    await vi.waitFor(() =>
      expect(
        requests
          .slice(beforePoll)
          .filter((path) => path.startsWith("/api/v2/history?")),
      ).toHaveLength(1),
    );
    const polled = requests.filter((path) =>
      path.startsWith("/api/v2/history?"),
    );
    expect(polled.every((path) => path.includes("at=evt%3Aone"))).toBe(true);
    expect(polled.some((path) => path.includes("at=evt%3Atwo"))).toBe(false);
    // The pinned Timeline window deliberately does not advance while an exact
    // event is read: a follow never leaves the event route, and the monitor
    // only observes timeline routes. That is existing behavior, not a
    // consequence of reading from the loaded page.
    expect(
      Array.from(
        document.querySelectorAll("#timeline li.event[data-event-id]"),
      ).map((row) => row.getAttribute("data-event-id")),
    ).toEqual(["evt:one", "evt:two", "evt:three"]);
    expect(
      document
        .querySelector('#timeline [aria-selected="true"]')
        ?.getAttribute("data-event-id"),
    ).toBe("evt:two");
  });

  it("rehydrates the same exact route when polling publishes a newer projection", async () => {
    vi.useFakeTimers();
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let generation = 1;
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      const stamp = `sha256:generation-${generation}`;
      if (path === "/api/v2/profile")
        return new Response(
          JSON.stringify({
            ...profile,
            authorityCursor: authorityCursor(generation),
          }),
        );
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes", stamp)));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention", stamp)));
      if (isExactRevisionPath(path))
        return new Response(JSON.stringify(revisionDetail(stamp)));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    expect(requests.filter((path) => isExactRevisionPath(path))).toHaveLength(
      1,
    );

    generation = 2;
    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() =>
      expect(requests.filter((path) => isExactRevisionPath(path))).toHaveLength(
        2,
      ),
    );
    expect(
      document.querySelector<HTMLElement>("#detail-body")?.dataset
        .changeReadingKey,
    ).toContain("sha256:generation-2");
  });

  it("coalesces overlapping ticks behind one slow generation poll", async () => {
    vi.useFakeTimers();
    let authoritySequence = 1;
    let changesRequests = 0;
    let activeProjectionStamp = "sha256:generation-0";
    let resolveSlowChanges!: (response: Response) => void;
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile")
        return Promise.resolve(
          new Response(
            JSON.stringify({
              ...profile,
              authorityCursor: authorityCursor(authoritySequence),
            }),
          ),
        );
      if (path.startsWith("/api/v2/changes?")) {
        changesRequests += 1;
        activeProjectionStamp = `sha256:generation-${changesRequests}`;
        if (changesRequests === 2) {
          return new Promise<Response>((resolve) => {
            resolveSlowChanges = resolve;
          });
        }
        return Promise.resolve(
          new Response(JSON.stringify(page("changes", activeProjectionStamp))),
        );
      }
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(
          new Response(
            JSON.stringify(page("attention", activeProjectionStamp)),
          ),
        );
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();

    authoritySequence = 2;
    await vi.advanceTimersByTimeAsync(3_000);
    expect(changesRequests).toBe(2);
    await vi.advanceTimersByTimeAsync(6_000);
    expect(changesRequests).toBe(2);

    resolveSlowChanges(
      new Response(JSON.stringify(page("changes", "sha256:generation-2"))),
    );
    await vi.waitFor(() =>
      expect(document.querySelector("#stat-hash")?.textContent).toBe(
        "sha256:generation-2",
      ),
    );
    authoritySequence = 3;
    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() => {
      expect(changesRequests).toBe(3);
      expect(document.querySelector("#stat-hash")?.textContent).toBe(
        "sha256:generation-3",
      );
    });
  });

  it("times out a hung exact postflight and releases its coalesced successor", async () => {
    vi.useFakeTimers();
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let generation = 1;
    let profileRequests = 0;
    let changesRequests = 0;
    let exactRequests = 0;
    let markHungPostflightStarted!: () => void;
    const hungPostflightStarted = new Promise<void>((resolve) => {
      markHungPostflightStarted = resolve;
    });
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      const stamp = `sha256:generation-${generation}`;
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        if (profileRequests === 6) {
          markHungPostflightStarted();
          return new Promise<Response>(() => {});
        }
        return Promise.resolve(
          new Response(
            JSON.stringify({
              ...profile,
              authorityCursor: authorityCursor(generation),
            }),
          ),
        );
      }
      if (path.startsWith("/api/v2/changes?")) {
        changesRequests += 1;
        return Promise.resolve(
          new Response(JSON.stringify(page("changes", stamp))),
        );
      }
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(
          new Response(JSON.stringify(page("attention", stamp))),
        );
      if (isExactRevisionPath(path)) {
        exactRequests += 1;
        return Promise.resolve(
          new Response(JSON.stringify(revisionDetail(stamp))),
        );
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();

    generation = 2;
    await vi.advanceTimersByTimeAsync(3_000);
    await hungPostflightStarted;
    expect(changesRequests).toBe(2);
    expect(exactRequests).toBe(2);
    await vi.advanceTimersByTimeAsync(3_000);
    await vi.advanceTimersByTimeAsync(3_000);
    expect(changesRequests).toBe(2);
    expect(exactRequests).toBe(2);

    const detail = document.querySelector<HTMLElement>("#detail-body");
    if (detail === null) throw new Error("missing exact detail body");
    await vi.advanceTimersByTimeAsync(10_000);
    expect(changesRequests).toBe(3);
    expect(exactRequests).toBe(3);
    expect(
      document.querySelector<HTMLElement>("#detail-body")?.dataset
        .changeReadingKey,
    ).toContain("sha256:generation-2");
  });

  it("preserves a focused uncommitted search draft across poll paints", async () => {
    vi.useFakeTimers();
    let generation = 1;
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      const stamp = `sha256:generation-${generation}`;
      if (path === "/api/v2/profile")
        return new Response(
          JSON.stringify({
            ...profile,
            authorityCursor: authorityCursor(generation),
          }),
        );
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes", stamp)));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention", stamp)));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();

    const search = document.querySelector<HTMLInputElement>("#filter-text");
    if (search === null) throw new Error("missing Change search input");
    search.focus();
    search.value = "uncommitted draft";
    search.setSelectionRange(4, 11, "forward");
    expect(document.activeElement).toBe(search);

    const statHash = document.querySelector<HTMLElement>("#stat-hash");
    if (statHash === null) throw new Error("missing projection stamp status");
    const repainted = new Promise<void>((resolve) => {
      const observer = new MutationObserver(() => {
        if (statHash.textContent === "sha256:generation-2") {
          observer.disconnect();
          resolve();
        }
      });
      observer.observe(statHash, {
        childList: true,
        characterData: true,
        subtree: true,
      });
    });
    generation = 2;
    await vi.advanceTimersByTimeAsync(3_000);
    await repainted;

    expect(location.hash).toBe("#/changes");
    expect(search.value).toBe("uncommitted draft");
    expect(document.activeElement).toBe(search);
    expect(search.selectionStart).toBe(4);
    expect(search.selectionEnd).toBe(11);
    expect(search.selectionDirection).toBe("forward");
  });

  it("preserves an incomplete Timeline draft and its completions across poll paints", async () => {
    vi.useFakeTimers();
    history.replaceState(null, "", "/#/timeline?limit=20");
    let generation = 1;
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      const stamp = `sha256:generation-${generation}`;
      if (path === "/api/v2/profile") {
        return new Response(
          JSON.stringify({
            ...profile,
            authorityCursor: authorityCursor(generation),
          }),
        );
      }
      if (path.startsWith("/api/v2/changes?")) {
        const document = page("changes", stamp);
        return new Response(
          JSON.stringify({
            ...document,
            changes:
              generation === 1
                ? document.changes
                : [
                    ...document.changes,
                    {
                      ...document.changes[0],
                      changeId: "change:sha256:two",
                    },
                  ],
          }),
        );
      }
      if (path.startsWith("/api/v2/attention?")) {
        const document = page("attention", stamp);
        return new Response(
          JSON.stringify({
            ...document,
            changes:
              generation === 1
                ? document.changes
                : [
                    ...document.changes,
                    {
                      ...document.changes[0],
                      changeId: "change:sha256:two",
                    },
                  ],
          }),
        );
      }
      if (path.startsWith("/api/v2/history?")) {
        return new Response(
          JSON.stringify({
            ...searchableHistoryPage(stamp),
            authorityCursor: authorityCursor(generation),
            eventCount: generation,
            timelineProjectionStamp: `sha256:timeline-${generation}`,
          }),
        );
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();

    const search = document.querySelector<HTMLInputElement>("#filter-text");
    const suggestions = document.querySelector<HTMLElement>(
      "#filter-suggestions",
    );
    if (!search || !suggestions) throw new Error("missing Timeline search UI");
    search.focus();
    search.value = "revision:";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    const suggestionTexts = () =>
      Array.from(
        suggestions.querySelectorAll<HTMLElement>("[role='option']"),
      ).map((option) => option.textContent);
    const expected = [
      "revision:revision:sha256:one",
      "revision:revision:sha256:unresolved",
    ];
    expect(suggestionTexts()).toEqual(expected);
    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
    );
    const activeSuggestion = search.getAttribute("aria-activedescendant");
    expect(activeSuggestion).not.toBeNull();

    generation = 2;
    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() =>
      expect(
        requests.filter((request) => request === "/api/v2/profile"),
      ).toHaveLength(4),
    );
    await vi.waitFor(() =>
      expect(document.querySelector("#stat-units")?.textContent).toBe(
        "2 Changes",
      ),
    );

    expect(location.hash).toBe("#/timeline?limit=20");
    expect(search.value).toBe("revision:");
    expect(document.activeElement).toBe(search);
    expect(suggestionTexts()).toEqual(expected);
    expect(search.getAttribute("aria-activedescendant")).toBe(activeSuggestion);
    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    expect(search.value).toBe("revision:revision:sha256:one ");
    expect(parseChangeInspectorRoute(location.hash)).toEqual({
      kind: "timeline",
      historyQuery: {
        limit: 20,
        q: "revision:revision:sha256:one",
      },
    });
  });

  it("preserves a search draft started after a background poll begins", async () => {
    vi.useFakeTimers();
    let generation = 1;
    let profileRequests = 0;
    let markPollStarted!: () => void;
    let releasePoll!: () => void;
    const pollStarted = new Promise<void>((resolve) => {
      markPollStarted = resolve;
    });
    const pollGate = new Promise<void>((resolve) => {
      releasePoll = resolve;
    });
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      const stamp = `sha256:generation-${generation}`;
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        if (profileRequests === 3) {
          markPollStarted();
          await pollGate;
        }
        return new Response(
          JSON.stringify({
            ...profile,
            authorityCursor: authorityCursor(generation),
          }),
        );
      }
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes", stamp)));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention", stamp)));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();

    const search = document.querySelector<HTMLInputElement>("#filter-text");
    if (search === null) throw new Error("missing Change search input");
    await vi.advanceTimersByTimeAsync(3_000);
    await pollStarted;
    search.focus();
    search.value = "draft started during poll";
    search.setSelectionRange(6, 13, "backward");
    generation = 2;
    releasePoll();

    await vi.waitFor(() =>
      expect(document.querySelector("#stat-hash")?.textContent).toBe(
        "sha256:generation-2",
      ),
    );
    expect(location.hash).toBe("#/changes");
    expect(search.value).toBe("draft started during poll");
    expect(document.activeElement).toBe(search);
    expect(search.selectionStart).toBe(6);
    expect(search.selectionEnd).toBe(13);
    expect(search.selectionDirection).toBe("backward");
  });

  it("does not let an older same-query detail failure restart a route the user left", async () => {
    let rejectOldDetail!: (reason?: unknown) => void;
    const requests: string[] = [];
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile")
        return Promise.resolve(new Response(JSON.stringify(profile)));
      if (path.startsWith("/api/v2/changes?"))
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      if (isExactRevisionPath(path)) {
        return new Promise<Response>((_resolve, reject) => {
          rejectOldDetail = reject;
        });
      }
      if (isExactResourcePath(path))
        return Promise.resolve(
          new Response(JSON.stringify(revisionDetail().exactRevisionDocument)),
        );
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    location.hash =
      "#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact";
    window.dispatchEvent(new Event("hashchange"));
    await vi.waitFor(() => expect(requests).toHaveLength(7));

    location.hash =
      "#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone/resource?artifactHash=sha256%3Aartifact";
    window.dispatchEvent(new Event("hashchange"));
    await vi.waitFor(() => {
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Authoritative captured diff",
      );
    });
    rejectOldDetail(new Error("old detail request failed"));
    await Promise.resolve();
    await Promise.resolve();

    expect(
      requests.filter((path) => path.startsWith("/api/v2/changes?")).length,
    ).toBe(1);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Authoritative captured diff",
    );
  });

  it("retains exact detail chrome through resource loading and accepted paints", async () => {
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let resolveResource!: (response: Response) => void;
    let markResourceStarted!: () => void;
    const resourceStarted = new Promise<void>((resolve) => {
      markResourceStarted = resolve;
    });
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") {
        return Promise.resolve(
          new Response(
            JSON.stringify({
              schema: "pointbreak.inspect-identity",
              storeIdentity: "store:sha256:focus",
              contextIdentity: "context:sha256:focus",
              repository: "focus-pointbreak",
              placement: { tier: "clone", label: "clone store" },
            }),
          ),
        );
      }
      if (path === "/api/v2/profile")
        return Promise.resolve(new Response(JSON.stringify(profile)));
      if (path.startsWith("/api/v2/changes?"))
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      if (isExactResourcePath(path)) {
        markResourceStarted();
        return new Promise<Response>((resolve) => {
          resolveResource = resolve;
        });
      }
      if (isExactRevisionPath(path))
        return Promise.resolve(new Response(JSON.stringify(revisionDetail())));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    const activation = Array.from(
      document.querySelectorAll<HTMLButtonElement>("#detail-body button"),
    ).find(
      (button) => button.textContent === "Open authoritative captured diff",
    );
    if (activation === undefined) {
      throw new Error("missing captured-resource activation");
    }
    activation.focus();
    expect(document.activeElement).toBe(activation);
    activation.click();
    await resourceStarted;

    await vi.waitFor(() =>
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Loading captured resource",
      ),
    );
    expect(activation.isConnected).toBe(false);
    expect(document.activeElement).toBe(
      document.querySelector("#detail-close"),
    );

    resolveResource(
      new Response(JSON.stringify(revisionDetail().exactRevisionDocument)),
    );
    await vi.waitFor(() =>
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Authoritative captured diff",
      ),
    );
    expect(
      document.querySelector<HTMLElement>("#detail-body")?.dataset
        .changeReadingKey,
    ).toContain("/resource?");
    expect(document.activeElement).toBe(
      document.querySelector("#detail-close"),
    );
  });

  it("restores exact selection through Back, Forward, and a fresh bootstrap", async () => {
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isExactRevisionPath(path))
        return new Response(JSON.stringify(revisionDetail()));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const reader = await import("../src/change-inspector");
    await reader.bootstrapChangeInspector({ poll: false });
    const exactHash =
      "#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact";

    location.hash = exactHash;
    await vi.waitFor(() => {
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Exact Revision",
      );
    });
    location.hash = "#/attention";
    await vi.waitFor(() => {
      expect(document.querySelector("#master h1")?.textContent).toContain(
        "Attention",
      );
    });

    history.back();
    await vi.waitFor(() => {
      expect(location.hash).toBe(exactHash);
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Exact Revision",
      );
    });
    history.forward();
    await vi.waitFor(() => {
      expect(location.hash).toBe("#/attention");
      expect(document.querySelector("#master h1")?.textContent).toContain(
        "Attention",
      );
    });
    history.back();
    await vi.waitFor(() => expect(location.hash).toBe(exactHash));

    const requestCount = requests.length;
    reader.stopChangeInspector();
    await reader.bootstrapChangeInspector({ poll: false });
    expect(requests).toHaveLength(requestCount + 7);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Exact Revision",
    );
    const viewToggle =
      document.querySelector<HTMLButtonElement>("#view-toggle");
    const viewPanel = document.querySelector("#view-panel");
    viewToggle?.click();
    expect(viewPanel?.classList).not.toContain("hidden");
    viewToggle?.click();
    expect(viewPanel?.classList).toContain("hidden");
  });

  it("dismisses lightweight control disclosures when route intent changes", async () => {
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    const viewToggle =
      document.querySelector<HTMLButtonElement>("#view-toggle");
    const viewPanel = document.querySelector("#view-panel");
    viewToggle?.click();
    expect(viewPanel?.classList).not.toContain("hidden");

    location.hash = "#/attention";
    window.dispatchEvent(new Event("hashchange"));
    await vi.waitFor(() =>
      expect(document.querySelector("#master h1")?.textContent).toContain(
        "Attention",
      ),
    );
    expect(viewPanel?.classList).toContain("hidden");
    expect(viewToggle?.getAttribute("aria-expanded")).toBe("false");

    const filtersToggle =
      document.querySelector<HTMLButtonElement>("#filters-toggle");
    const filtersPanel = document.querySelector("#filters-panel");
    filtersToggle?.click();
    expect(filtersPanel?.classList).not.toContain("hidden");

    location.hash = "#/changes";
    window.dispatchEvent(new Event("hashchange"));
    await vi.waitFor(() =>
      expect(document.querySelector("#master h1")?.textContent).toContain(
        "Changes",
      ),
    );
    expect(filtersPanel?.classList).toContain("hidden");
    expect(filtersToggle?.getAttribute("aria-expanded")).toBe("false");
  });

  it("retries one profile-generation mismatch exactly once", async () => {
    let profileRequests = 0;
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        const eventCount = profileRequests === 2 ? 2 : 1;
        return new Response(
          JSON.stringify({
            ...profile,
            authorityCursor: authorityCursor(eventCount),
          }),
        );
      }
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isExactRevisionPath(path))
        return new Response(JSON.stringify(revisionDetail()));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );

    await bootstrapChangeInspector({ poll: false });

    expect(profileRequests).toBe(4);
    expect(
      requests.filter((path) => path.startsWith("/api/v2/changes?")).length,
    ).toBe(2);
    expect(
      requests.filter((path) => path.startsWith("/api/v2/attention?")).length,
    ).toBe(2);
    expect(document.querySelector("#master")?.textContent).toContain(
      "change:sha256:one",
    );
  });

  it("shares one projection restart across generation and exact-detail reads", async () => {
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let changesRequests = 0;
    let attentionRequests = 0;
    let exactRequests = 0;
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?")) {
        changesRequests += 1;
        return changesRequests === 1
          ? staleProjectionResponse()
          : new Response(JSON.stringify(page("changes")));
      }
      if (path.startsWith("/api/v2/attention?")) {
        attentionRequests += 1;
        return new Response(JSON.stringify(page("attention")));
      }
      if (isExactRevisionPath(path)) {
        exactRequests += 1;
        return staleProjectionResponse();
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );

    await bootstrapChangeInspector({ poll: false });

    expect(changesRequests).toBe(2);
    expect(attentionRequests).toBe(2);
    expect(exactRequests).toBe(1);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "server response error",
    );
  });

  it("keeps a route-origin reading active at the soft budget and aborts it at the hard budget", async () => {
    vi.useFakeTimers();
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let exactSignal: AbortSignal | null | undefined;
    let markExactStarted!: () => void;
    const exactStarted = new Promise<void>((resolve) => {
      markExactStarted = resolve;
    });
    globalThis.fetch = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input);
      if (path === "/api/identity")
        return Promise.resolve(
          new Response(
            JSON.stringify({
              schema: "pointbreak.inspect-identity",
              storeIdentity: "store:sha256:budget",
              contextIdentity: "context:sha256:budget",
              repository: "budget-pointbreak",
              placement: { tier: "clone", label: "clone store" },
            }),
          ),
        );
      if (path === "/api/v2/profile")
        return Promise.resolve(new Response(JSON.stringify(profile)));
      if (path.startsWith("/api/v2/changes?"))
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      if (isExactRevisionPath(path)) {
        exactSignal = init?.signal;
        markExactStarted();
        return new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener("abort", () =>
            reject(new DOMException("aborted", "AbortError")),
          );
        });
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    const bootstrap = bootstrapChangeInspector({ poll: false });
    await exactStarted;

    await vi.advanceTimersByTimeAsync(10_000);
    expect(exactSignal?.aborted).toBe(false);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Still loading a large exact reading",
    );
    expect(
      document.querySelector("[data-exact-reading-cancel]"),
    ).not.toBeNull();
    expect(document.querySelector("#detail-body")?.textContent).not.toContain(
      "Reader refused",
    );

    await vi.advanceTimersByTimeAsync(20_000);
    await bootstrap;
    expect(exactSignal?.aborted).toBe(true);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "exact reading timed out",
    );
    expect(document.querySelector("[data-exact-reading-retry]")).not.toBeNull();
    expect(
      document.querySelector("#refresh")?.getAttribute("data-state"),
    ).not.toBe("degraded");
  });

  it("does not overlap a slow route reading with background polling", async () => {
    vi.useFakeTimers();
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let authoritySequence = 1;
    let changesRequests = 0;
    let exactRequests = 0;
    let markExactStarted!: () => void;
    const exactStarted = new Promise<void>((resolve) => {
      markExactStarted = resolve;
    });
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity")
        return Promise.reject(new Error("identity is presentation-only"));
      if (path === "/api/v2/profile")
        return Promise.resolve(
          new Response(
            JSON.stringify({
              ...profile,
              authorityCursor: authorityCursor(authoritySequence),
            }),
          ),
        );
      if (path.startsWith("/api/v2/changes?")) {
        changesRequests += 1;
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      }
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      if (isExactRevisionPath(path)) {
        exactRequests += 1;
        markExactStarted();
        return new Promise<Response>((resolve) => {
          setTimeout(
            () => resolve(new Response(JSON.stringify(revisionDetail()))),
            12_000,
          );
        });
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    const bootstrap = bootstrapChangeInspector();
    await exactStarted;

    await vi.advanceTimersByTimeAsync(10_000);
    expect(exactRequests).toBe(1);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Still loading a large exact reading",
    );

    await vi.advanceTimersByTimeAsync(2_000);
    await bootstrap;
    expect(exactRequests).toBe(1);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Exact Revision",
    );

    authoritySequence = 2;
    await vi.advanceTimersByTimeAsync(3_000);
    expect(changesRequests).toBe(2);
    expect(exactRequests).toBe(1);
  });

  it("keeps a Back-target Revision reading ahead of a due poll", async () => {
    vi.useFakeTimers();
    const revisionHash =
      "#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact";
    history.replaceState(null, "", `/${revisionHash}`);
    let profileRequests = 0;
    let changesRequests = 0;
    let attentionRequests = 0;
    let revisionRequests = 0;
    let resourceRequests = 0;
    let resourceSignal: AbortSignal | null | undefined;
    let backRevisionSignal: AbortSignal | null | undefined;
    let resolveBackRevision!: (response: Response) => void;
    let markResourceStarted!: () => void;
    let markBackRevisionStarted!: () => void;
    const resourceStarted = new Promise<void>((resolve) => {
      markResourceStarted = resolve;
    });
    const backRevisionStarted = new Promise<void>((resolve) => {
      markBackRevisionStarted = resolve;
    });
    globalThis.fetch = vi.fn(
      (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
        const path = String(input);
        if (path === "/api/identity") {
          return Promise.resolve(
            new Response(
              JSON.stringify({
                schema: "pointbreak.inspect-identity",
                storeIdentity: "store:sha256:exact-back",
                contextIdentity: "context:sha256:exact-back",
                repository: "exact-back-pointbreak",
                placement: { tier: "clone", label: "clone store" },
              }),
            ),
          );
        }
        if (path === "/api/v2/profile") {
          profileRequests += 1;
          return Promise.resolve(new Response(JSON.stringify(profile)));
        }
        if (path.startsWith("/api/v2/changes?")) {
          changesRequests += 1;
          return Promise.resolve(new Response(JSON.stringify(page("changes"))));
        }
        if (path.startsWith("/api/v2/attention?")) {
          attentionRequests += 1;
          return Promise.resolve(
            new Response(JSON.stringify(page("attention"))),
          );
        }
        if (isExactResourcePath(path)) {
          resourceRequests += 1;
          resourceSignal = init?.signal;
          markResourceStarted();
          return new Promise<Response>((_resolve, reject) => {
            init?.signal?.addEventListener(
              "abort",
              () => reject(new DOMException("aborted", "AbortError")),
              { once: true },
            );
          });
        }
        if (isExactRevisionPath(path)) {
          revisionRequests += 1;
          if (revisionRequests === 1) {
            return Promise.resolve(
              new Response(JSON.stringify(revisionDetail())),
            );
          }
          backRevisionSignal = init?.signal;
          markBackRevisionStarted();
          return new Promise<Response>((resolve) => {
            resolveBackRevision = resolve;
          });
        }
        throw new Error(`unexpected ${path}`);
      },
    ) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();

    const activation = Array.from(
      document.querySelectorAll<HTMLButtonElement>("#detail-body button"),
    ).find(
      (button) => button.textContent === "Open authoritative captured diff",
    );
    if (activation === undefined) {
      throw new Error("missing captured-resource activation");
    }
    activation.click();
    await resourceStarted;
    expect(resourceRequests).toBe(1);

    history.back();
    await vi.advanceTimersByTimeAsync(0);
    await backRevisionStarted;
    expect(location.hash).toBe(revisionHash);
    expect(resourceSignal?.aborted).toBe(true);
    expect(backRevisionSignal?.aborted).toBe(false);
    expect(revisionRequests).toBe(2);
    const profileRequestsBeforeTick = profileRequests;
    const changesRequestsBeforeTick = changesRequests;
    const attentionRequestsBeforeTick = attentionRequests;

    await vi.advanceTimersByTimeAsync(3_000);
    expect(profileRequests).toBe(profileRequestsBeforeTick);
    expect(changesRequests).toBe(changesRequestsBeforeTick);
    expect(attentionRequests).toBe(attentionRequestsBeforeTick);
    expect(backRevisionSignal?.aborted).toBe(false);
    expect(revisionRequests).toBe(2);

    resolveBackRevision(new Response(JSON.stringify(revisionDetail())));
    await vi.waitFor(() =>
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Exact Revision",
      ),
    );
    const acceptedReadingKey =
      document.querySelector<HTMLElement>("#detail-body")?.dataset
        .changeReadingKey;
    expect(acceptedReadingKey).toContain("/revisions/");
    expect(acceptedReadingKey).not.toContain("/resource?");
    expect(acceptedReadingKey).toContain("sha256:generation");
    expect(backRevisionSignal?.aborted).toBe(false);
    expect(resourceRequests).toBe(1);
    expect(revisionRequests).toBe(2);

    const profileRequestsAfterReading = profileRequests;
    await vi.advanceTimersByTimeAsync(3_000);
    expect(profileRequests).toBe(profileRequestsAfterReading + 1);
    expect(changesRequests).toBe(changesRequestsBeforeTick);
    expect(attentionRequests).toBe(attentionRequestsBeforeTick);
  });

  it("keeps a hard-budget failure on manual Retry instead of polling it automatically", async () => {
    vi.useFakeTimers();
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let authoritySequence = 1;
    let changesRequests = 0;
    let exactRequests = 0;
    let markExactStarted!: () => void;
    const exactStarted = new Promise<void>((resolve) => {
      markExactStarted = resolve;
    });
    globalThis.fetch = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input);
      if (path === "/api/identity")
        return Promise.reject(new Error("identity is presentation-only"));
      if (path === "/api/v2/profile")
        return Promise.resolve(
          new Response(
            JSON.stringify({
              ...profile,
              authorityCursor: authorityCursor(authoritySequence),
            }),
          ),
        );
      if (path.startsWith("/api/v2/changes?")) {
        changesRequests += 1;
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      }
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      if (isExactRevisionPath(path)) {
        exactRequests += 1;
        markExactStarted();
        return new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener("abort", () =>
            reject(new DOMException("aborted", "AbortError")),
          );
        });
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    const bootstrap = bootstrapChangeInspector();
    await exactStarted;

    await vi.advanceTimersByTimeAsync(30_000);
    await bootstrap;
    expect(exactRequests).toBe(1);
    expect(document.querySelector("[data-exact-reading-retry]")).not.toBeNull();

    authoritySequence = 2;
    await vi.advanceTimersByTimeAsync(3_000);
    expect(changesRequests).toBe(2);
    expect(exactRequests).toBe(1);
    expect(document.querySelector("[data-exact-reading-retry]")).not.toBeNull();
  });

  it("keeps an empty-body route failure on manual Retry instead of polling it automatically", async () => {
    vi.useFakeTimers();
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let authoritySequence = 1;
    let changesRequests = 0;
    let exactRequests = 0;
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity")
        return Promise.reject(new Error("identity is presentation-only"));
      if (path === "/api/v2/profile")
        return Promise.resolve(
          new Response(
            JSON.stringify({
              ...profile,
              authorityCursor: authorityCursor(authoritySequence),
            }),
          ),
        );
      if (path.startsWith("/api/v2/changes?")) {
        changesRequests += 1;
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      }
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      if (isExactRevisionPath(path)) {
        exactRequests += 1;
        return Promise.resolve(
          exactRequests === 1
            ? new Response("", { status: 503 })
            : new Response(JSON.stringify(revisionDetail())),
        );
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    expect(exactRequests).toBe(1);

    authoritySequence = 2;
    await vi.advanceTimersByTimeAsync(3_000);
    expect(changesRequests).toBe(2);
    expect(exactRequests).toBe(1);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "server response error",
    );
    expect(document.querySelector("[data-exact-reading-retry]")).not.toBeNull();

    document
      .querySelector<HTMLButtonElement>("[data-exact-reading-retry]")
      ?.click();
    await vi.waitFor(() => expect(exactRequests).toBe(2));
    await vi.waitFor(() =>
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Exact Revision",
      ),
    );
  });

  it("retries a hard-budget failure as a fresh route reading", async () => {
    vi.useFakeTimers();
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    const signals: AbortSignal[] = [];
    let exactRequests = 0;
    let markFirstExactStarted!: () => void;
    const firstExactStarted = new Promise<void>((resolve) => {
      markFirstExactStarted = resolve;
    });
    globalThis.fetch = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input);
      if (path === "/api/identity")
        return Promise.reject(new Error("identity is presentation-only"));
      if (path === "/api/v2/profile")
        return Promise.resolve(new Response(JSON.stringify(profile)));
      if (path.startsWith("/api/v2/changes?"))
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      if (isExactRevisionPath(path)) {
        exactRequests += 1;
        if (init?.signal) signals.push(init.signal);
        if (exactRequests === 1) markFirstExactStarted();
        if (exactRequests > 1)
          return Promise.resolve(
            new Response(JSON.stringify(revisionDetail())),
          );
        return new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener("abort", () =>
            reject(new DOMException("aborted", "AbortError")),
          );
        });
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    const bootstrap = bootstrapChangeInspector({ poll: false });
    await firstExactStarted;
    await vi.advanceTimersByTimeAsync(30_000);
    await bootstrap;

    document
      .querySelector<HTMLButtonElement>("[data-exact-reading-retry]")
      ?.click();
    await vi.waitFor(() => expect(exactRequests).toBe(2));
    expect(signals).toHaveLength(2);
    expect(signals[0]).not.toBe(signals[1]);
    await vi.waitFor(() =>
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Exact Revision",
      ),
    );
  });

  it("accepts a 9.9 second document followed by a 2.5 second postflight", async () => {
    vi.useFakeTimers();
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let profileRequests = 0;
    let markExactStarted!: () => void;
    const exactStarted = new Promise<void>((resolve) => {
      markExactStarted = resolve;
    });
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity")
        return Promise.reject(new Error("identity is presentation-only"));
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        const delay = profileRequests === 3 ? 2_500 : 0;
        if (delay === 0)
          return Promise.resolve(new Response(JSON.stringify(profile)));
        return new Promise<Response>((resolve) => {
          setTimeout(
            () => resolve(new Response(JSON.stringify(profile))),
            delay,
          );
        });
      }
      if (path.startsWith("/api/v2/changes?"))
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      if (isExactRevisionPath(path)) {
        markExactStarted();
        return new Promise<Response>((resolve) => {
          setTimeout(
            () => resolve(new Response(JSON.stringify(revisionDetail()))),
            9_900,
          );
        });
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    const bootstrap = bootstrapChangeInspector({ poll: false });

    await exactStarted;
    await vi.advanceTimersByTimeAsync(9_900);
    expect(profileRequests).toBe(3);
    await vi.advanceTimersByTimeAsync(2_500);
    await bootstrap;

    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Exact Revision",
    );
    expect(document.querySelector("#detail-body")?.textContent).not.toContain(
      "Still loading",
    );
  });

  it("aborts a route postflight at its own budget and offers Retry", async () => {
    vi.useFakeTimers();
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let profileRequests = 0;
    let postflightSignal: AbortSignal | null | undefined;
    globalThis.fetch = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input);
      if (path === "/api/identity")
        return Promise.reject(new Error("identity is presentation-only"));
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        if (profileRequests === 3) {
          postflightSignal = init?.signal;
          return new Promise<Response>((_resolve, reject) => {
            init?.signal?.addEventListener("abort", () =>
              reject(new DOMException("aborted", "AbortError")),
            );
          });
        }
        return Promise.resolve(new Response(JSON.stringify(profile)));
      }
      if (path.startsWith("/api/v2/changes?"))
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      if (isExactRevisionPath(path))
        return Promise.resolve(new Response(JSON.stringify(revisionDetail())));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    const bootstrap = bootstrapChangeInspector({ poll: false });

    await vi.advanceTimersByTimeAsync(3_000);
    await bootstrap;

    expect(postflightSignal?.aborted).toBe(true);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "exact reading postflight timed out",
    );
    expect(document.querySelector("[data-exact-reading-retry]")).not.toBeNull();
  });

  it("aborts a superseded route reading silently", async () => {
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let exactSignal: AbortSignal | null | undefined;
    let markExactStarted!: () => void;
    const exactStarted = new Promise<void>((resolve) => {
      markExactStarted = resolve;
    });
    globalThis.fetch = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input);
      if (path === "/api/identity")
        return Promise.reject(new Error("identity is presentation-only"));
      if (path === "/api/v2/profile")
        return Promise.resolve(new Response(JSON.stringify(profile)));
      if (path.startsWith("/api/v2/changes?"))
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      if (isExactRevisionPath(path)) {
        exactSignal = init?.signal;
        markExactStarted();
        return new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener("abort", () =>
            reject(new DOMException("aborted", "AbortError")),
          );
        });
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    const bootstrap = bootstrapChangeInspector({ poll: false });
    await exactStarted;

    location.hash = "#/attention";
    window.dispatchEvent(new Event("hashchange"));
    await bootstrap;
    await vi.waitFor(() =>
      expect(document.querySelector("#master h1")?.textContent).toContain(
        "Attention",
      ),
    );

    expect(exactSignal?.aborted).toBe(true);
    expect(document.querySelector("#detail-body")?.textContent).not.toContain(
      "Reader refused",
    );
    expect(
      document.querySelector("#refresh")?.getAttribute("data-state"),
    ).not.toBe("degraded");
  });

  it("Cancel aborts the reading and leaves a cancelled presentation with Retry", async () => {
    vi.useFakeTimers();
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let exactSignal: AbortSignal | null | undefined;
    globalThis.fetch = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input);
      if (path === "/api/identity")
        return Promise.reject(new Error("identity is presentation-only"));
      if (path === "/api/v2/profile")
        return Promise.resolve(new Response(JSON.stringify(profile)));
      if (path.startsWith("/api/v2/changes?"))
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      if (isExactRevisionPath(path)) {
        exactSignal = init?.signal;
        return new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener("abort", () =>
            reject(new DOMException("aborted", "AbortError")),
          );
        });
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    const bootstrap = bootstrapChangeInspector({ poll: false });
    await vi.advanceTimersByTimeAsync(10_000);

    document
      .querySelector<HTMLButtonElement>("[data-exact-reading-cancel]")
      ?.click();
    await bootstrap;

    expect(exactSignal?.aborted).toBe(true);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "exact reading cancelled",
    );
    expect(document.querySelector("[data-exact-reading-retry]")).not.toBeNull();
  });

  it("an unchanged healthy poll performs one profile probe and does not paint", async () => {
    vi.useFakeTimers();
    const control = servePollComposition();
    const stateModule = await import("../src/change-inspector-state");
    const createState = stateModule.createChangeInspectorState;
    let publishCalls = 0;
    vi.spyOn(stateModule, "createChangeInspectorState").mockImplementation(
      (route) => {
        const state = createState(route);
        const publish = state.publish;
        state.publish = (...args) => {
          publishCalls += 1;
          return publish(...args);
        };
        return state;
      },
    );
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    expect(publishCalls).toBe(1);

    const master = document.querySelector<HTMLElement>("#master");
    if (master === null) throw new Error("missing master pane");
    const sentinel = document.createElement("details");
    sentinel.dataset.pollSentinel = "retained";
    sentinel.open = true;
    master.append(sentinel);
    master.scrollTop = 41;
    const requestBoundary = control.requests.length;

    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() =>
      expect(
        control.requests.filter((path) => path === "/api/v2/profile"),
      ).toHaveLength(3),
    );

    expect(control.requests.slice(requestBoundary)).toEqual([
      "/api/v2/profile",
    ]);
    expect(sentinel.isConnected).toBe(true);
    expect(sentinel.open).toBe(true);
    expect(master.scrollTop).toBe(41);
    expect(document.querySelector("[data-poll-sentinel='retained']")).toBe(
      sentinel,
    );
    expect(publishCalls).toBe(1);
  });

  it("a changed profile reuses its probe and performs one full coherent load", async () => {
    vi.useFakeTimers();
    const control = servePollComposition();
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    const requestBoundary = control.requests.length;
    control.generation = 2;

    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() =>
      expect(document.querySelector("#stat-hash")?.textContent).toBe(
        "sha256:generation-2",
      ),
    );

    const cycleRequests = control.requests.slice(requestBoundary);
    expect(
      cycleRequests.filter((path) => path === "/api/v2/profile"),
    ).toHaveLength(2);
    expect(
      cycleRequests.filter((path) => path.startsWith("/api/v2/changes?")),
    ).toHaveLength(1);
    expect(
      cycleRequests.filter((path) => path.startsWith("/api/v2/attention?")),
    ).toHaveLength(1);
  });

  it("a failed full cycle backs off and forces full validation before quiet polling resumes", async () => {
    vi.useFakeTimers();
    const control = servePollComposition();
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    control.generation = 2;
    control.changesMode = "failure";

    await vi.advanceTimersByTimeAsync(3_000);
    expect(document.querySelector("#refresh-status")?.textContent).toBe(
      "response error",
    );
    expect(
      control.requests.filter((path) => path.startsWith("/api/v2/changes?")),
    ).toHaveLength(2);

    control.generation = 1;
    control.changesMode = "ok";
    await vi.advanceTimersByTimeAsync(5_999);
    expect(
      control.requests.filter((path) => path.startsWith("/api/v2/changes?")),
    ).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(
      control.requests.filter((path) => path.startsWith("/api/v2/changes?")),
    ).toHaveLength(3);

    await vi.advanceTimersByTimeAsync(3_000);
    expect(
      control.requests.filter((path) => path === "/api/v2/profile"),
    ).toHaveLength(6);
    expect(
      control.requests.filter((path) => path.startsWith("/api/v2/changes?")),
    ).toHaveLength(3);
  });

  it("failed cycles double their completion delay to the 30 second cap", async () => {
    vi.useFakeTimers();
    const control = servePollComposition();
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    control.generation = 2;
    control.changesMode = "failure";

    for (const delay of [3_000, 6_000, 12_000, 24_000, 30_000, 30_000]) {
      await vi.advanceTimersByTimeAsync(delay);
    }

    const profileStarts = control.requests.flatMap((path, index) =>
      path === "/api/v2/profile" ? [control.requestTimes[index]] : [],
    );
    const startedAt = profileStarts[0] ?? 0;
    expect(profileStarts.slice(2).map((time) => time - startedAt)).toEqual([
      3_000, 9_000, 21_000, 45_000, 75_000, 105_000,
    ]);
  });

  it("a projection retry forwards the cycle signal and cannot become quiet", async () => {
    vi.useFakeTimers();
    const control = servePollComposition();
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    control.generation = 2;
    control.changesMode = "stale_once";
    control.onStaleChanges = () => {
      control.generation = 1;
    };

    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() =>
      expect(
        control.requests.filter((path) => path.startsWith("/api/v2/changes?")),
      ).toHaveLength(3),
    );

    expect(control.signals.length).toBeGreaterThan(0);
    expect(new Set(control.signals).size).toBe(1);
  });

  it.each([
    "migration_required",
    "migration_in_progress",
  ] as const)("treats the %s non-ready profile branch as a failed backoff cycle", async (availability) => {
    vi.useFakeTimers();
    const control = servePollComposition();
    control.availability = availability;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    expect(
      control.requests.filter((path) => path === "/api/v2/profile"),
    ).toHaveLength(1);

    await vi.advanceTimersByTimeAsync(3_000);
    expect(
      control.requests.filter((path) => path === "/api/v2/profile"),
    ).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(5_999);
    expect(
      control.requests.filter((path) => path === "/api/v2/profile"),
    ).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(
      control.requests.filter((path) => path === "/api/v2/profile"),
    ).toHaveLength(3);
  });

  it("retains an accepted generation while a non-ready poll backs off and latches full validation", async () => {
    vi.useFakeTimers();
    const control = servePollComposition();
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    control.availability = "migration_in_progress";

    await vi.advanceTimersByTimeAsync(3_000);
    expect(
      control.requests.filter((path) => path === "/api/v2/profile"),
    ).toHaveLength(3);
    expect(
      control.requests.filter((path) => path.startsWith("/api/v2/changes?")),
    ).toHaveLength(1);
    expect(document.querySelector(".unit-card[data-change-id]")).not.toBeNull();

    control.availability = "ready";
    await vi.advanceTimersByTimeAsync(5_999);
    expect(
      control.requests.filter((path) => path === "/api/v2/profile"),
    ).toHaveLength(3);
    await vi.advanceTimersByTimeAsync(1);
    expect(
      control.requests.filter((path) => path === "/api/v2/profile"),
    ).toHaveLength(5);
    expect(
      control.requests.filter((path) => path.startsWith("/api/v2/changes?")),
    ).toHaveLength(2);
  });

  it("credential movement cannot take the quiet profile path", async () => {
    vi.useFakeTimers();
    const control = servePollComposition();
    const reader = await import("../src/change-inspector");
    const auth = await import("../src/auth");
    await reader.bootstrapChangeInspector();
    auth.setSessionToken("rotated-poll-session");

    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() =>
      expect(
        control.requests.filter((path) => path.startsWith("/api/v2/changes?")),
      ).toHaveLength(2),
    );
    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() =>
      expect(
        control.requests.filter((path) => path === "/api/v2/profile"),
      ).toHaveLength(5),
    );
    expect(
      control.requests.filter((path) => path.startsWith("/api/v2/changes?")),
    ).toHaveLength(2);
  });

  it("a cycle timeout aborts in-flight requests and requires full validation", async () => {
    vi.useFakeTimers();
    const control = servePollComposition();
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    control.generation = 2;
    control.changesMode = "hang";

    await vi.advanceTimersByTimeAsync(3_000);
    expect(
      control.requests.filter((path) => path.startsWith("/api/v2/changes?")),
    ).toHaveLength(2);
    const cycleSignal = control.signals[0];
    expect(cycleSignal).toBeDefined();
    expect(cycleSignal?.aborted).toBe(false);

    await vi.advanceTimersByTimeAsync(15_000);
    expect(cycleSignal?.aborted).toBe(true);
    expect(document.querySelector("#refresh-status")?.textContent).toBe(
      "response error",
    );

    control.generation = 1;
    control.changesMode = "ok";
    const profileRequests = () =>
      control.requests.filter((path) => path === "/api/v2/profile").length;
    const beforeRecovery = profileRequests();
    await vi.advanceTimersByTimeAsync(5_999);
    expect(profileRequests()).toBe(beforeRecovery);
    await vi.advanceTimersByTimeAsync(1);
    expect(profileRequests()).toBe(beforeRecovery + 2);
    expect(
      control.requests.filter((path) => path.startsWith("/api/v2/changes?")),
    ).toHaveLength(3);
  });

  it("an invalid route consumes a tick without stalling the loop", async () => {
    vi.useFakeTimers();
    const suppressSyntheticHashchange = (event: Event): void => {
      event.stopImmediatePropagation();
    };
    window.addEventListener("hashchange", suppressSyntheticHashchange, true);
    try {
      const control = servePollComposition();
      const { bootstrapChangeInspector } = await import(
        "../src/change-inspector"
      );
      await bootstrapChangeInspector();
      history.replaceState(null, "", "/#/changes?unknown=value");

      await vi.advanceTimersByTimeAsync(3_000);
      expect(
        control.requests.filter((path) => path === "/api/v2/profile"),
      ).toHaveLength(2);
      history.replaceState(null, "", "/#/changes");
      await vi.advanceTimersByTimeAsync(3_000);
      expect(
        control.requests.filter((path) => path === "/api/v2/profile"),
      ).toHaveLength(3);
    } finally {
      window.removeEventListener(
        "hashchange",
        suppressSyntheticHashchange,
        true,
      );
    }
  });

  it("a poll refresh keeps its reading painted and aborts at the 10 second refresh expiry", async () => {
    vi.useFakeTimers();
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let generation = 1;
    let exactRequests = 0;
    const refreshSignals: AbortSignal[] = [];
    globalThis.fetch = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input);
      const stamp = `sha256:generation-${generation}`;
      if (path === "/api/identity")
        return Promise.reject(new Error("identity is presentation-only"));
      if (path === "/api/v2/profile")
        return Promise.resolve(
          new Response(
            JSON.stringify({
              ...profile,
              authorityCursor: authorityCursor(generation),
            }),
          ),
        );
      if (path.startsWith("/api/v2/changes?"))
        return Promise.resolve(
          new Response(JSON.stringify(page("changes", stamp))),
        );
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(
          new Response(JSON.stringify(page("attention", stamp))),
        );
      if (isExactRevisionPath(path)) {
        exactRequests += 1;
        if (exactRequests === 1)
          return Promise.resolve(
            new Response(JSON.stringify(revisionDetail(stamp))),
          );
        if (init?.signal) refreshSignals.push(init.signal);
        return new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener("abort", () =>
            reject(new DOMException("aborted", "AbortError")),
          );
        });
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    const readingKey =
      document.querySelector<HTMLElement>("#detail-body")?.dataset
        .changeReadingKey;
    generation = 2;

    await vi.advanceTimersByTimeAsync(3_000);
    await vi.waitFor(() => expect(exactRequests).toBe(2));
    await vi.advanceTimersByTimeAsync(10_000);

    expect(refreshSignals[0]?.aborted).toBe(true);
    expect(
      document.querySelector<HTMLElement>("#detail-body")?.dataset
        .changeReadingKey,
    ).toBe(readingKey);
    expect(document.querySelector("#detail-body")?.textContent).not.toContain(
      "Still loading",
    );
    expect(document.querySelector("#refresh")?.getAttribute("data-state")).toBe(
      "degraded",
    );
  });

  it("a recovery refresh has the same 10 second reading expiry without a cycle bound", async () => {
    vi.useFakeTimers();
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let generation = 1;
    let exactRequests = 0;
    const recoverySignals: AbortSignal[] = [];
    globalThis.fetch = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input);
      const stamp = `sha256:generation-${generation}`;
      if (path === "/api/identity")
        return Promise.resolve(
          new Response(
            JSON.stringify({
              schema: "pointbreak.inspect-identity",
              storeIdentity: "store:sha256:recovery",
              contextIdentity: "context:sha256:recovery",
              repository: "recovery-pointbreak",
              placement: { tier: "clone", label: "clone store" },
            }),
          ),
        );
      if (path === "/api/v2/profile")
        return Promise.resolve(
          new Response(
            JSON.stringify({
              ...profile,
              authorityCursor: authorityCursor(generation),
            }),
          ),
        );
      if (path.startsWith("/api/v2/changes?"))
        return Promise.resolve(
          new Response(JSON.stringify(page("changes", stamp))),
        );
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(
          new Response(JSON.stringify(page("attention", stamp))),
        );
      if (isExactRevisionPath(path)) {
        exactRequests += 1;
        if (exactRequests === 1)
          return Promise.resolve(
            new Response(JSON.stringify(revisionDetail(stamp))),
          );
        if (init?.signal) recoverySignals.push(init.signal);
        return new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener("abort", () =>
            reject(new DOMException("aborted", "AbortError")),
          );
        });
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    const readingKey =
      document.querySelector<HTMLElement>("#detail-body")?.dataset
        .changeReadingKey;
    generation = 2;

    document.querySelector<HTMLButtonElement>("#connection-action")?.click();
    await vi.waitFor(() => expect(exactRequests).toBe(2));
    await vi.advanceTimersByTimeAsync(8_000);
    expect(recoverySignals[0]?.aborted).toBe(false);
    expect(exactRequests).toBe(2);
    await vi.advanceTimersByTimeAsync(2_000);

    expect(recoverySignals[0]?.aborted).toBe(true);
    expect(
      document.querySelector<HTMLElement>("#detail-body")?.dataset
        .changeReadingKey,
    ).toBe(readingKey);
    expect(document.querySelector("#refresh")?.getAttribute("data-state")).toBe(
      "degraded",
    );
  });
});
