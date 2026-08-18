import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { parseChangeInspectorRoute } from "../src/change-inspector-router";
import type { ChangeInspectorSnapshot } from "../src/change-inspector-state";
import {
  CHANGE_READER_DOCUMENTS,
  type EventHistoryDocument,
  type EventHistoryEntry,
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
    history.replaceState(
      null,
      "",
      "/#/attention?after=attention-page&limit=20&order=change_id_asc",
    );
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
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

    pollTick();
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
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
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

    pollTick();
    await vi.waitFor(() =>
      expect(
        requests.filter((path) => path === "/api/v2/profile"),
      ).toHaveLength(4),
    );
    expect(requests.filter((path) => path === "/api/identity")).toHaveLength(1);
  });

  it("does not let a hung identity request gate semantic paint or poll installation", async () => {
    let identityResolve!: (response: Response) => void;
    const identityResponse = new Promise<Response>((resolve) => {
      identityResolve = resolve;
    });
    const interval = vi
      .spyOn(globalThis, "setInterval")
      .mockImplementation(() => 1 as unknown as ReturnType<typeof setInterval>);
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/identity") return identityResponse;
      if (path === "/api/v2/profile")
        return Promise.resolve(new Response(JSON.stringify(profile)));
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
      expect(interval).toHaveBeenCalledWith(expect.any(Function), 3000);
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
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
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
    pollTick();
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
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
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
        if (mismatchPostflight && profileRequests === 8) {
          return new Response(
            JSON.stringify({
              ...profile,
              authorityCursor: authorityCursor(2),
            }),
          );
        }
        return new Response(JSON.stringify(profile));
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
    pollTick();
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

    pollTick();
    await vi.waitFor(() => expect(profileRequests).toBe(6));
    expect(document.querySelector("#refresh-status")?.textContent).toBe(
      "watching",
    );

    generation = 3;
    mismatchPostflight = true;
    pollTick();
    await vi.waitFor(() => expect(profileRequests).toBe(10));
    expect(document.querySelector("#refresh-status")?.textContent).not.toBe(
      "response error",
    );
  });

  it("does not publish a poll generation after its credential session changes", async () => {
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
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
    pollTick();
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
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
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
    pollTick();
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
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
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
    pollTick();
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

  it("rehydrates the same exact route when polling publishes a newer projection", async () => {
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let generation = 1;
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
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
    pollTick();
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
    let changesRequests = 0;
    let activeProjectionStamp = "sha256:generation-0";
    let resolveSlowChanges!: (response: Response) => void;
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile")
        return Promise.resolve(new Response(JSON.stringify(profile)));
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

    await vi.advanceTimersByTimeAsync(3_000);
    expect(changesRequests).toBe(2);
    await vi.advanceTimersByTimeAsync(6_000);
    expect(changesRequests).toBe(2);

    resolveSlowChanges(
      new Response(JSON.stringify(page("changes", "sha256:generation-2"))),
    );
    await vi.waitFor(() => {
      expect(changesRequests).toBe(3);
      expect(document.querySelector("#stat-hash")?.textContent).toBe(
        "sha256:generation-3",
      );
    });
  });

  it("times out a hung exact postflight and releases its coalesced successor", async () => {
    vi.useFakeTimers();
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
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
    pollTick();
    await hungPostflightStarted;
    expect(changesRequests).toBe(2);
    expect(exactRequests).toBe(2);
    pollTick();
    pollTick();
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
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
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
    pollTick();
    await repainted;

    expect(location.hash).toBe("#/changes");
    expect(search.value).toBe("uncommitted draft");
    expect(document.activeElement).toBe(search);
    expect(search.selectionStart).toBe(4);
    expect(search.selectionEnd).toBe(11);
    expect(search.selectionDirection).toBe("forward");
  });

  it("preserves an incomplete Timeline draft and its completions across poll paints", async () => {
    history.replaceState(null, "", "/#/timeline?limit=20");
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
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
    pollTick();
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
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
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
    pollTick();
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
});
