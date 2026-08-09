import { beforeEach, describe, expect, expectTypeOf, it, vi } from "vitest";
import type { ChangeInspectorReading } from "../src/change-inspector-reading";
import {
  type ChangeInspectorRenderActions,
  prepareChangeInspectorShell,
  renderChangeInspectorRefusal,
  renderChangeInspector as renderChangeInspectorSource,
} from "../src/change-inspector-render";
import { formatChangeInspectorRoute } from "../src/change-inspector-router";
import {
  createChangeInspectorState,
  stageGeneration,
} from "../src/change-inspector-state";
import type {
  AttentionPage,
  ChangeDetail,
  ChangesPage,
  EventHistoryDocument,
  ReaderProfile,
  RevisionResource,
} from "../src/change-protocol";
import { authorityCursor } from "./support/authority";
import { mountInspectorDom, resetDom } from "./support/dom";

type TestRenderActions = Omit<ChangeInspectorRenderActions, "replace"> &
  Partial<Pick<ChangeInspectorRenderActions, "replace">>;

function renderChangeInspector(
  snapshot: Parameters<typeof renderChangeInspectorSource>[0],
  actions: TestRenderActions,
  presentation?: Parameters<typeof renderChangeInspectorSource>[2],
): void {
  renderChangeInspectorSource(
    snapshot,
    { replace: actions.navigate, ...actions },
    presentation,
  );
}

const profile: ReaderProfile = {
  schema: "pointbreak.inspect-reader-profile",
  version: 1,
  availability: "ready",
  authorityCursor: authorityCursor(2),
  commitGraphStamp: "sha256:stamp",
  minimumReaderProfile: "review_change_revision_v1",
  documents: {},
};
const revision = {
  revisionId: "revision:sha256:one",
  objectArtifactContentHash: "sha256:artifact",
};
const changes: ChangesPage = {
  schema: "pointbreak.inspect-changes-page",
  version: 1,
  projectionStamp: "sha256:generation",
  next: null,
  changes: [
    {
      changeId: "change:sha256:one",
      declarationState: "authoritative",
      titleAssertions: [],
      memberCount: 1,
      topology: "parallel_current",
      lifecycle: "in_progress",
      attentionSummary: "conflicted",
      availabilitySummary: "incomplete",
      currentRevisionRefs: [revision],
      projectionStamp: "sha256:generation",
    },
  ],
  presentations: {
    "change:sha256:one": {
      currentRevisions: [
        {
          revision,
          revisionProposalSummary: "Server proposal",
          summarySource: "revision_proposal_summary",
        },
      ],
    },
  },
};
const attention: AttentionPage = {
  ...changes,
  schema: "pointbreak.inspect-attention",
  version: 2,
  presentations: {
    "change:sha256:one": {
      ...changes.presentations?.["change:sha256:one"],
      currentRevisions:
        changes.presentations?.["change:sha256:one"]?.currentRevisions ?? [],
      attention: {
        primaryReason: {
          kind: "current_revisions_need_assessment",
          revisions: [revision],
        },
        reasons: [
          { kind: "current_revisions_need_assessment", revisions: [revision] },
        ],
        reasonPresentations: [
          {
            cause: {
              kind: "current_revisions_need_assessment",
              revisions: [revision],
            },
            ask: "ASK SENTINEL",
            reason: "REASON SENTINEL",
            evidence: "EVIDENCE SENTINEL",
            nextAction: "NEXT ACTION SENTINEL",
          },
        ],
        diagnostics: [
          "assessment coverage is incomplete for revision:sha256:one",
        ],
      },
    },
  },
};

function eventHistory(): EventHistoryDocument {
  return {
    schema: "pointbreak.inspect-event-history",
    version: 1,
    authorityCursor: authorityCursor(2),
    sourceChangeProjectionStamp: "sha256:generation",
    timelineProjectionStamp: "sha256:timeline",
    order: "desc",
    eventCount: 2,
    matchCount: 1,
    offset: 0,
    facets: { validation_check_recorded: 1, change_declared: 0 },
    completion: {
      eventTypes: ["validation_check_recorded", "change_declared"],
      trackIds: ["author"],
      changeIds: ["change:sha256:one"],
      revisionRefs: [revision],
      unresolvedRevisionIds: [],
    },
    diagnostics: [],
    queryNotices: [],
    entries: [
      {
        eventId: "evt:sha256:one",
        eventType: "validation_check_recorded",
        occurredAt: "2026-08-08T00:00:00Z",
        payloadHash: "sha256:payload",
        journalId: "journal:sha256:one",
        writer: {
          actorId: "actor:author",
          producer: { name: "pointbreak", version: "0.10.0" },
        },
        verificationStatus: "valid",
        assertionMode: "advisory",
        subject: {
          kind: "review",
          target: { kind: "revision", revisionId: revision.revisionId },
        },
        changeIds: ["change:sha256:one"],
        revisionRefs: [revision],
        unresolvedRevisionIds: [],
        sourceRef: {
          sourceSystem: "legacy-review-journal",
          sourceId: "event:legacy:one",
        },
        ingest: {
          via: "ingest-events",
          receivedAt: "2026-08-08T00:00:01Z",
        },
        summary: {
          kind: "validation_check_recorded",
          details: {
            validationCheckId: "validation:sha256:web",
            target: { kind: "revision", revisionId: revision.revisionId },
            checkName: "Web checks",
            status: "passed",
            trigger: "manual",
            summary: "The Inspector presentation checks passed.",
          },
        },
      },
    ],
  };
}

function exactResource(): RevisionResource {
  return {
    schema: "pointbreak.review-revision-resource" as const,
    version: 1 as const,
    resource: { revision, objectId: "obj:sha256:one" },
    projection: { includeBody: true },
    availability: "available",
    capturedDocumentHash: "sha256:captured-document",
    capturedDocument: {
      schema: "pointbreak.review-snapshot" as const,
      version: 1 as const,
      contentHash: revision.objectArtifactContentHash,
      snapshot: {
        review_id: "review:sha256:one",
        object_id: "obj:sha256:one",
        files: [
          {
            status: "modified",
            new_path: "src/lib.rs",
            hunks: [
              {
                header: "@@ -1 +1 @@",
                rows: [
                  {
                    kind: "added",
                    old_line: null,
                    new_line: 1,
                    text: "pub fn restored() {}",
                  },
                ],
              },
            ],
          },
        ],
      },
    },
    diagnostics: [],
    projectionStamp: "sha256:generation",
    cacheKey: "sha256:resource",
  };
}

function revisionReading(): Extract<
  ChangeInspectorReading,
  { kind: "revision" }
> {
  return {
    kind: "revision",
    document: {
      schema: "pointbreak.review-change-revision",
      version: 1,
      changeId: "change:sha256:one",
      revision,
      membershipSupport: [],
      revisionCurrency: "current",
      relationClassification: "current",
      availability: "available",
      exactRevisionDocument: exactResource(),
      factPresentations: [
        {
          factId: "obs:sha256:focused",
          family: "observation",
          originRevision: revision,
          target: {
            kind: "range",
            revisionId: revision.revisionId,
            filePath: "src/lib.rs",
            side: "new",
            startLine: 1,
            endLine: 1,
          },
          actorId: "author",
          revisionCurrency: "current",
          familyState: "current",
          availability: "available",
        },
      ],
      factContentPresentations: {
        "obs:sha256:focused": {
          contentType: "text/markdown",
          bodyContentState: "present",
          content: {
            kind: "observation",
            title: "Readable fact",
            body: "**Rendered** fact body",
          },
        },
      },
      factPorts: [],
      associations: [],
      diagnostics: [],
      projectionStamp: "sha256:generation",
      inspectorPresentation: {
        factGraph: {
          nodes: [
            {
              id: `fact:${revision.revisionId}@${revision.objectArtifactContentHash}:observation:obs:sha256:focused`,
              kind: "fact",
              revision,
              factId: "obs:sha256:focused",
              family: "observation",
              displayLabel: "observation · obs:focused",
              x: 80,
              y: 30,
              w: 140,
              h: 34,
              contextAvailability: "available",
              activationRevision: revision,
            },
          ],
          observationSupersedes: [],
          assessmentReplaces: [],
          factPorts: [],
          bounds: { w: 160, h: 64 },
        },
      },
    },
  };
}

function changeReading(): ChangeInspectorReading {
  const predecessor = {
    revisionId: "revision:sha256:predecessor",
    objectArtifactContentHash: "sha256:predecessor",
  };
  const summary = changes.changes[0];
  if (!summary) throw new Error("fixture needs Change summary");
  const document: ChangeDetail = {
    schema: "pointbreak.review-change",
    version: 1,
    summary: { ...summary, memberCount: 2, topology: "replacement" },
    memberRevisions: [
      { revision, supportingClaimIds: [] },
      { revision: predecessor, supportingClaimIds: [] },
    ],
    unavailableMemberRevisions: [],
    membershipClaims: [],
    membershipWithdrawals: [],
    relationClaims: [],
    relationWithdrawals: [],
    links: [],
    effectiveSupersedes: [[revision, predecessor]],
    pendingOrConflictingEdges: [],
    currentRevisionRefs: [revision],
    perCurrentRevisionQualification: [],
    operativeObligations: [],
    diagnostics: [],
    projectionStamp: "sha256:generation",
    inspectorPresentation: {
      revisionGraph: {
        nodes: [
          {
            id: `revision:${revision.revisionId}@${revision.objectArtifactContentHash}`,
            revision,
            displayLabel: "current · revision:one",
            x: 80,
            y: 30,
            w: 128,
            h: 34,
            isCurrent: true,
            isMember: true,
            contextAvailability: "available",
            activationRevision: revision,
          },
          {
            id: `revision:${predecessor.revisionId}@${predecessor.objectArtifactContentHash}`,
            revision: predecessor,
            displayLabel: "revision:predecessor",
            x: 80,
            y: 110,
            w: 128,
            h: 34,
            isCurrent: false,
            isMember: true,
            contextAvailability: "available",
            activationRevision: predecessor,
          },
        ],
        effectiveSupersedes: [
          {
            from: `revision:${revision.revisionId}@${revision.objectArtifactContentHash}`,
            to: `revision:${predecessor.revisionId}@${predecessor.objectArtifactContentHash}`,
            successor: revision,
            predecessor,
            path: [
              [80, 110],
              [80, 30],
            ],
          },
        ],
        pendingOrConflictingClaims: [],
        bounds: { w: 160, h: 144 },
      },
    },
  };
  return { kind: "change", document };
}

function interdiffReading(): Extract<
  ChangeInspectorReading,
  { kind: "interdiff" }
> {
  const otherRevision = {
    revisionId: "revision:sha256:two",
    objectArtifactContentHash: "sha256:other-artifact",
  };
  return {
    kind: "interdiff",
    document: {
      schema: "pointbreak.review-revision-interdiff",
      version: 1,
      interdiff: {
        from: revision,
        to: otherRevision,
        algorithmVersion: "rows-v1",
        scope: ["src/lib.rs"],
      },
      availability: "available",
      comparison: { files: [] },
      diagnostics: [],
      projectionStamp: "sha256:generation",
      cacheKey: "sha256:interdiff",
    },
  };
}

beforeEach(() => mountInspectorDom());

describe("Change inspector render", () => {
  it("requires replacement semantics for renderer-owned query edits", () => {
    expectTypeOf<ChangeInspectorRenderActions["replace"]>().toEqualTypeOf<
      ChangeInspectorRenderActions["navigate"]
    >();
  });

  it("renders native lens links with the active lens as the only current page", () => {
    const cases = [
      {
        route: { kind: "timeline" as const, historyQuery: {} },
        current: "timeline",
        heading: "Timeline",
        meta: "1 event · newest first",
      },
      {
        route: { kind: "lens" as const, lens: "changes" as const, query: {} },
        current: "changes",
        heading: "Changes",
        meta: "1 Change on this page · Change ID order",
      },
      {
        route: {
          kind: "lens" as const,
          lens: "attention" as const,
          query: {},
        },
        current: "attention",
        heading: "Attention",
        meta: "1 Change on this page · Change ID order",
      },
    ];

    for (const { route, current, heading, meta } of cases) {
      const state = createChangeInspectorState(route);
      state.publish(
        stageGeneration(profile, changes, attention, profile, eventHistory()),
      );
      renderChangeInspector(state.snapshot(), { navigate: vi.fn() });

      const links = Array.from(
        document.querySelectorAll<HTMLAnchorElement>(
          "#lens-switcher a[data-lens]",
        ),
      );
      expect(links).toHaveLength(3);
      expect(document.querySelectorAll("#master h1")).toHaveLength(1);
      expect(
        document.querySelector("#master h1.lens-heading")?.textContent,
      ).toBe(heading);
      expect(document.querySelector("#master .lens-meta")?.textContent).toBe(
        meta,
      );
      expect(
        links.map((link) => ({
          lens: link.dataset.lens,
          href: link.getAttribute("href"),
          current: link.getAttribute("aria-current"),
        })),
      ).toEqual([
        {
          lens: "timeline",
          href: "#/timeline",
          current: current === "timeline" ? "page" : null,
        },
        {
          lens: "changes",
          href: "#/changes",
          current: current === "changes" ? "page" : null,
        },
        {
          lens: "attention",
          href: "#/attention",
          current: current === "attention" ? "page" : null,
        },
      ]);
      const attentionLink = links.find(
        (link) => link.dataset.lens === "attention",
      );
      expect(attentionLink?.querySelector(".lens-count")?.textContent).toBe(
        "1 shown",
      );
      expect(attentionLink?.getAttribute("aria-label")).toBe(
        "Attention, 1 Change shown on this page",
      );
    }
  });

  it("refreshes the bounded Attention count from each accepted generation", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "lens",
      lens: "changes",
      query: {},
    });
    const second = {
      ...attention.changes[0],
      changeId: "change:sha256:two",
    };
    state.publish(
      stageGeneration(
        profile,
        changes,
        { ...attention, changes: [...attention.changes, second] },
        profile,
      ),
    );
    renderChangeInspector(state.snapshot(), { navigate });
    expect(
      document.querySelector('[data-lens="attention"] .lens-count')
        ?.textContent,
    ).toBe("2 shown");

    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(state.snapshot(), { navigate });
    expect(
      document.querySelector('[data-lens="attention"] .lens-count')
        ?.textContent,
    ).toBe("1 shown");
  });

  it("qualifies bounded Attention counts as page-local when more results remain", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "lens",
      lens: "attention",
      query: { limit: 1 },
    });
    state.publish(
      stageGeneration(
        profile,
        changes,
        { ...attention, next: "opaque-next-page" },
        profile,
      ),
    );
    renderChangeInspector(state.snapshot(), { navigate });

    expect(document.querySelector("#master .lens-meta")?.textContent).toMatch(
      /on this page/i,
    );
    expect(
      document
        .querySelector('[data-lens="attention"]')
        ?.getAttribute("aria-label"),
    ).toMatch(/on this page/i);
    expect(document.querySelector("#stat-threads")?.textContent).toBe(
      "1 shown on this page",
    );
    expect(document.querySelector("#stat-threads")?.getAttribute("title")).toBe(
      "Changes shown on this Attention page",
    );
  });

  it("offers completion-backed Timeline filters and removable applied filters", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "timeline",
      historyQuery: {
        q: "web",
        type: "validation_check_recorded",
        track: "author",
        change: "change:sha256:one",
        revision: revision.revisionId,
        artifactHash: revision.objectArtifactContentHash,
        limit: 20,
        order: "asc",
      },
    });
    state.publish(
      stageGeneration(profile, changes, attention, profile, eventHistory()),
    );
    renderChangeInspector(state.snapshot(), { navigate });

    expect(
      document.querySelector<HTMLSelectElement>("#timeline-filter-track")
        ?.value,
    ).toBe("author");
    expect(
      document.querySelector<HTMLSelectElement>("#timeline-filter-change")
        ?.value,
    ).toBe("change:sha256:one");
    expect(
      document.querySelector<HTMLSelectElement>("#timeline-filter-revision")
        ?.selectedOptions[0]?.dataset.artifactHash,
    ).toBe(revision.objectArtifactContentHash);
    expect(
      document
        .querySelector<HTMLButtonElement>(
          '[data-event-type="validation_check_recorded"]',
        )
        ?.getAttribute("aria-pressed"),
    ).toBe("true");
    expect(
      document.querySelector<HTMLButtonElement>(
        '[data-event-type="change_declared"]',
      )?.textContent,
    ).toContain("0");
    document
      .querySelector<HTMLButtonElement>(
        '[aria-label="Remove track filter: author"]',
      )
      ?.click();
    expect(navigate).toHaveBeenCalledWith({
      kind: "timeline",
      historyQuery: {
        q: "web",
        type: "validation_check_recorded",
        track: undefined,
        change: "change:sha256:one",
        revision: revision.revisionId,
        artifactHash: revision.objectArtifactContentHash,
        limit: 20,
        order: "asc",
        after: undefined,
        at: undefined,
      },
    });

    navigate.mockClear();
    document
      .querySelector<HTMLButtonElement>('[data-event-type="change_declared"]')
      ?.click();
    expect(navigate).toHaveBeenCalledWith({
      kind: "timeline",
      historyQuery: {
        q: "web",
        type: "change_declared,validation_check_recorded",
        track: "author",
        change: "change:sha256:one",
        revision: revision.revisionId,
        artifactHash: revision.objectArtifactContentHash,
        limit: 20,
        order: "asc",
        after: undefined,
        at: undefined,
      },
    });
  });

  it("renders one removable chip per Timeline q clause without disturbing outer exact filters", () => {
    const navigate = vi.fn();
    const replace = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "timeline",
      historyQuery: {
        q: "free revision:0123 rev:4567 change:fed change:fed",
        change: "change:sha256:one",
        revision: revision.revisionId,
        artifactHash: revision.objectArtifactContentHash,
        at: "evt:sha256:anchor",
        limit: 20,
        order: "asc",
      },
    });
    state.publish(
      stageGeneration(profile, changes, attention, profile, eventHistory()),
    );
    renderChangeInspector(state.snapshot(), { navigate, replace });

    const chips = Array.from(
      document.querySelectorAll<HTMLButtonElement>("#filter-chips button"),
    );
    expect(chips).toHaveLength(6);
    expect(chips.map((chip) => chip.textContent)).toEqual(
      expect.arrayContaining([
        "revision: 0123 ×",
        "revision: 4567 ×",
        "change: fed ×",
        "change: fed ×",
      ]),
    );
    expect(chips.map((chip) => chip.textContent).join(" ")).not.toContain(
      "free ×",
    );

    const duplicateChanges = chips.filter(
      (chip) => chip.textContent === "change: fed ×",
    );
    duplicateChanges[1]?.click();
    expect(replace).toHaveBeenCalledWith({
      kind: "timeline",
      historyQuery: {
        q: "free revision:0123 rev:4567 change:fed",
        change: "change:sha256:one",
        revision: revision.revisionId,
        artifactHash: revision.objectArtifactContentHash,
        at: undefined,
        after: undefined,
        limit: 20,
        order: "asc",
      },
    });
    expect(navigate).not.toHaveBeenCalled();
  });

  it("keeps duplicate Revision IDs distinct by their exact artifact hash", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const duplicate = {
      revisionId: revision.revisionId,
      objectArtifactContentHash: "sha256:other-artifact",
    };
    const history = eventHistory();
    history.completion.revisionRefs = [revision, duplicate];
    const state = createChangeInspectorState({
      kind: "timeline",
      historyQuery: {},
    });
    state.publish(
      stageGeneration(profile, changes, attention, profile, history),
    );
    renderChangeInspector(state.snapshot(), { navigate });

    const select = document.querySelector<HTMLSelectElement>(
      "#timeline-filter-revision",
    );
    const exactOptions = Array.from(select?.options ?? []).slice(1);
    expect(exactOptions.map((option) => option.value)).toHaveLength(2);
    expect(new Set(exactOptions.map((option) => option.value)).size).toBe(2);
    const second = exactOptions[1];
    if (!select || !second) throw new Error("missing duplicate exact option");
    select.value = second.value;
    select.dispatchEvent(new Event("change", { bubbles: true }));

    expect(navigate).toHaveBeenCalledWith({
      kind: "timeline",
      historyQuery: {
        after: undefined,
        at: undefined,
        revision: revision.revisionId,
        artifactHash: duplicate.objectArtifactContentHash,
      },
    });
  });

  it("renders a selected Timeline event as an exact readable surface", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "event",
      eventId: "evt:sha256:one",
      historyQuery: { q: "web" },
      query: {},
    });
    state.publish(
      stageGeneration(profile, changes, attention, profile, eventHistory()),
    );
    renderChangeInspector(state.snapshot(), { navigate });

    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Event",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "validation check recorded",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Web checks: passed",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "The Inspector presentation checks passed.",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Revision revision:sha256:one",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "actor:author",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "legacy-review-journal · event:legacy:one",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "ingest-events · 2026-08-08T00:00:01Z",
    );
    expect(document.querySelector("details.event-structured")).not.toBeNull();
    expect(
      document.querySelector("li.event")?.getAttribute("aria-selected"),
    ).toBe("true");
    const buttons = Array.from(
      document.querySelectorAll<HTMLButtonElement>("#detail-body button"),
    );
    buttons.find((button) => button.textContent === "Open Change")?.click();
    buttons
      .find((button) => button.textContent === "Open exact Revision")
      ?.click();
    expect(navigate).toHaveBeenNthCalledWith(1, {
      kind: "change",
      changeId: "change:sha256:one",
      query: {},
    });
    expect(navigate).toHaveBeenNthCalledWith(2, {
      kind: "revision",
      changeId: "change:sha256:one",
      revision,
      query: {},
    });
  });

  it("keeps plural Event Change choices short while their actions stay exact", () => {
    const navigate = vi.fn();
    const changeIds = [
      `change:sha256:${"a".repeat(64)}`,
      `change:sha256:${"b".repeat(64)}`,
    ];
    const history = eventHistory();
    const entry = history.entries[0];
    if (!entry) throw new Error("fixture needs an event");
    history.entries = [{ ...entry, changeIds }];

    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "event",
      eventId: entry.eventId,
      historyQuery: {},
      query: {},
    });
    state.publish(
      stageGeneration(profile, changes, attention, profile, history),
    );
    renderChangeInspector(state.snapshot(), { navigate });

    const choices = Array.from(
      document.querySelectorAll<HTMLButtonElement>(
        "#detail-body [data-event-change-choice]",
      ),
    );
    expect(choices.map((choice) => choice.textContent)).toEqual([
      "Open Change change:aaaaaaaa",
      "Open Change change:bbbbbbbb",
    ]);
    for (const [index, choice] of choices.entries()) {
      const changeId = changeIds[index];
      expect(changeId).toBeDefined();
      expect(choice.textContent).not.toContain(changeId);
      expect(choice.title).toBe(`Change ${changeId}`);
      expect(choice.getAttribute("aria-label")).toBe(`Open Change ${changeId}`);
      expect(choice.dataset.eventChangeChoice).toBe(changeId);
      expect(choice.dataset.changeId).toBe(changeId);
    }

    choices[1]?.click();
    expect(navigate).toHaveBeenCalledWith({
      kind: "change",
      changeId: changeIds[1],
      query: {},
    });
  });

  it("retains the bounded card DOM for an unchanged polling generation", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "lens",
      lens: "changes",
      query: {},
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(state.snapshot(), { navigate });
    const firstCard = document.querySelector(".unit-card[data-change-id]");
    expect(
      firstCard?.querySelector(".change-card-state")?.textContent,
    ).toContain("conflicted");
    expect(document.querySelector("#stat-threads")?.textContent).toBe(
      "1 shown on this page",
    );
    const createElement = vi.spyOn(document, "createElement");
    renderChangeInspector(state.snapshot(), { navigate });
    expect(document.querySelector(".unit-card[data-change-id]")).toBe(
      firstCard,
    );
    expect(
      document.querySelectorAll(".unit-card[data-change-id]").length,
    ).toBeLessThanOrEqual(150);
    expect(
      createElement.mock.calls.some(([tagName]) => tagName === "article"),
    ).toBe(false);
    createElement.mockRestore();
  });

  it("states the server-ranked Attention reason, ask, and next action", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "lens",
      lens: "attention",
      query: {},
    });
    state.publish(stageGeneration(profile, changes, attention, profile));

    renderChangeInspector(state.snapshot(), { navigate });

    const card = document.querySelector(".unit-card[data-change-id]");
    expect(
      card?.querySelector(".change-card-attention-reason")?.textContent,
    ).toBe("REASON SENTINEL");
    expect(card?.querySelector(".change-card-attention-ask")?.textContent).toBe(
      "ASK SENTINEL",
    );
    expect(
      card?.querySelector(".change-card-attention-evidence")?.textContent,
    ).toBe("EVIDENCE SENTINEL");
    expect(
      card?.querySelector(".change-card-attention-action")?.textContent,
    ).toBe("Next: NEXT ACTION SENTINEL");
    expect(
      card?.querySelector(".change-card-attention-diagnostics")?.textContent,
    ).toContain("assessment coverage is incomplete for revision:sha256:one");
    expect(card?.querySelectorAll("button")).toHaveLength(1);
    const primaryName = card
      ?.querySelector(".change-card-primary")
      ?.getAttribute("aria-label");
    expect(primaryName).toContain("REASON SENTINEL");
    expect(primaryName).toContain("ASK SENTINEL");
    expect(primaryName).toContain("EVIDENCE SENTINEL");
    expect(primaryName).toContain("NEXT ACTION SENTINEL");
    expect(primaryName).toContain("Change change:sha256:one");
  });

  it("rebinds a retained Change pager when the server rotates its opaque capability", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "lens",
      lens: "changes",
      query: { q: "review" },
    });
    const firstPage = { ...changes, next: "signed-next-one" };
    state.publish(stageGeneration(profile, firstPage, attention, profile));
    renderChangeInspector(state.snapshot(), { navigate });
    const firstPager = Array.from(
      document.querySelectorAll<HTMLButtonElement>("#master button"),
    ).find((button) => button.textContent === "Next page");
    expect(firstPager).toBeDefined();

    const rotatedPage = { ...changes, next: "signed-next-two" };
    state.publish(stageGeneration(profile, rotatedPage, attention, profile));
    renderChangeInspector(state.snapshot(), { navigate });
    const rotatedPager = Array.from(
      document.querySelectorAll<HTMLButtonElement>("#master button"),
    ).find((button) => button.textContent === "Next page");
    rotatedPager?.click();

    expect(rotatedPager).not.toBe(firstPager);
    expect(navigate).toHaveBeenLastCalledWith({
      kind: "lens",
      lens: "changes",
      query: { q: "review", after: "signed-next-two" },
    });
  });

  it("projects server-issued previous, next, and last capabilities into exact lens routes", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "lens",
      lens: "changes",
      query: { q: "review", after: "signed-current" },
    });
    state.publish(
      stageGeneration(
        profile,
        {
          ...changes,
          previous: "signed-previous",
          next: "signed-next",
          last: "signed-last",
        },
        attention,
        profile,
      ),
    );
    renderChangeInspector(state.snapshot(), { navigate });

    for (const [direction, continuation] of [
      ["previous", "signed-previous"],
      ["next", "signed-next"],
      ["last", "signed-last"],
    ] as const) {
      const button = document.querySelector<HTMLButtonElement>(
        `[data-change-page="${direction}"]`,
      );
      expect(button?.dataset.changeTargetRoute).toBe(
        formatChangeInspectorRoute({
          kind: "lens",
          lens: "changes",
          query: { q: "review", after: continuation },
        }),
      );
      button?.click();
      expect(navigate).toHaveBeenLastCalledWith({
        kind: "lens",
        lens: "changes",
        query: { q: "review", after: continuation },
      });
    }
  });

  it("restores a same-generation list after a transient refusal replaced its DOM", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "lens",
      lens: "changes",
      query: {},
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(state.snapshot(), { navigate });
    expect(
      document.querySelector<HTMLElement>("#master")?.dataset.changeListKey,
    ).toBeDefined();

    renderChangeInspectorRefusal(new Error("temporary outage"));
    expect(
      document.querySelector<HTMLElement>("#master")?.dataset.changeListKey,
    ).toBe(undefined);
    expect(document.querySelector("#master")?.textContent).toContain(
      "temporary outage",
    );

    renderChangeInspector(state.snapshot(), { navigate });
    expect(
      document
        .querySelector(".unit-card[data-change-id]")
        ?.getAttribute("data-change-id"),
    ).toBe("change:sha256:one");
  });

  it("restores an exact event Timeline after its loading plane is replaced", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "event",
      eventId: "evt:sha256:one",
      historyQuery: { q: "web" },
      query: {},
    });
    const generation = stageGeneration(
      profile,
      changes,
      attention,
      profile,
      eventHistory(),
    );
    state.publish(generation);
    renderChangeInspector(state.snapshot(), { navigate });
    expect(
      document.querySelector<HTMLElement>("#master")?.dataset.timelineKey,
    ).toBeDefined();

    state.clearGeneration();
    renderChangeInspector(state.snapshot(), { navigate });
    expect(document.querySelector("#master")?.textContent).toContain(
      "Loading Change generation",
    );
    expect(
      document.querySelector<HTMLElement>("#master")?.dataset.timelineKey,
    ).toBe(undefined);

    state.publish(generation);
    renderChangeInspector(state.snapshot(), { navigate });
    const selected = document.querySelector<HTMLElement>(
      '#timeline [data-event-id="evt:sha256:one"]',
    );
    expect(selected?.getAttribute("aria-selected")).toBe("true");
    expect(
      document
        .querySelector("#timeline")
        ?.getAttribute("aria-activedescendant"),
    ).toBe(selected?.id);
    expect(document.querySelector("#master")?.textContent).not.toContain(
      "Loading Change generation",
    );
  });

  it("projects server-owned cards and exact placeholder selection into the retained shell", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "revision",
      changeId: "change:sha256:one",
      revision,
      query: {},
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(state.snapshot(), { navigate });
    expect(document.querySelector("#master")?.textContent).toContain(
      "Server proposal",
    );
    expect(document.querySelector("#master")?.textContent).toContain(
      "parallel current",
    );
    expect(
      document
        .querySelector(".unit-card[data-change-id]")
        ?.getAttribute("aria-label"),
    ).toBe(
      "Server proposal; Current Revision — Server proposal; exact Revision revision:sha256:one; artifact sha256:artifact; Change change:sha256:one",
    );
    expect(
      document
        .querySelector(".unit-card[data-change-id]")
        ?.hasAttribute("aria-labelledby"),
    ).toBe(false);
    expect(
      document
        .querySelector(".change-card-primary")
        ?.getAttribute("aria-label"),
    ).toContain(
      "Server proposal; Current Revision — Server proposal; exact Revision revision:sha256:one; artifact sha256:artifact; Change change:sha256:one",
    );
    expect(
      document.querySelectorAll(".unit-card[data-change-id] button"),
    ).toHaveLength(1);
    expect(
      document
        .querySelector(".change-card-current code")
        ?.getAttribute("title"),
    ).toContain("exact Revision revision:sha256:one; artifact sha256:artifact");
    const currentRevision = document.querySelector<HTMLElement>(
      ".change-card-current code",
    );
    expect(currentRevision?.getAttribute("aria-label")).toBe(
      "exact Revision revision:sha256:one; artifact sha256:artifact",
    );
    expect(currentRevision?.dataset.revisionId).toBe("revision:sha256:one");
    expect(currentRevision?.dataset.artifactHash).toBe("sha256:artifact");
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Exact reading surface is loading",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "sha256:artifact",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Copy link",
    );
  });

  it("renders one exact current Revision as a native secondary anchor without changing the card primary", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "lens",
      lens: "changes",
      query: { q: "review" },
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(state.snapshot(), { navigate });

    const card = document.querySelector<HTMLElement>(
      '.unit-card[data-change-id="change:sha256:one"]',
    );
    const anchor = card?.querySelector<HTMLAnchorElement>(
      ".change-card-current a[data-revision-id][data-artifact-hash]",
    );
    expect(anchor?.getAttribute("href")).toBe(
      "#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?q=review&artifactHash=sha256%3Aartifact",
    );
    expect(anchor?.getAttribute("aria-label")).toBe(
      "Open exact Revision revision:sha256:one; artifact sha256:artifact; for Change change:sha256:one",
    );
    expect(anchor?.dataset.revisionId).toBe(revision.revisionId);
    expect(anchor?.dataset.artifactHash).toBe(
      revision.objectArtifactContentHash,
    );
    card?.querySelector<HTMLButtonElement>(".change-card-primary")?.click();
    expect(navigate).toHaveBeenCalledWith({
      kind: "change",
      changeId: "change:sha256:one",
      query: { q: "review" },
    });
  });

  it("keeps plural current Revisions as explicit controls without a synthesized anchor", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const secondRevision = {
      revisionId: "revision:sha256:two",
      objectArtifactContentHash: "sha256:artifact-two",
    };
    const pluralChanges: ChangesPage = {
      ...changes,
      changes: [
        {
          ...changes.changes[0],
          currentRevisionRefs: [revision, secondRevision],
        },
      ],
      presentations: {
        "change:sha256:one": {
          currentRevisions: [
            ...(changes.presentations?.["change:sha256:one"]
              ?.currentRevisions ?? []),
            {
              revision: secondRevision,
              revisionProposalSummary: "Parallel proposal",
              summarySource: "revision_proposal_summary",
            },
          ],
        },
      },
    };
    const state = createChangeInspectorState({
      kind: "lens",
      lens: "changes",
      query: { q: "parallel" },
    });
    state.publish(stageGeneration(profile, pluralChanges, attention, profile));
    renderChangeInspector(state.snapshot(), { navigate });

    expect(document.querySelector(".change-card-current a")).toBeNull();
    const peerActions = [
      ...document.querySelectorAll<HTMLButtonElement>(".change-card-peer-open"),
    ];
    expect(peerActions).toHaveLength(2);
    for (const action of peerActions)
      expect(action.textContent).toMatch(/^Open current Revision · /);
    const secondPeer = peerActions.find(
      (action) => action.dataset.revisionId === secondRevision.revisionId,
    );
    secondPeer?.click();
    expect(navigate).toHaveBeenCalledWith({
      kind: "revision",
      changeId: "change:sha256:one",
      revision: secondRevision,
      query: { q: "parallel" },
    });
  });

  it("keeps an off-page exact deep link visible without claiming list absence is refusal", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "revision",
      changeId: "change:sha256:off-page",
      revision: {
        revisionId: "revision:sha256:stale-but-readable",
        objectArtifactContentHash: "sha256:off-page-artifact",
      },
      query: { q: "narrowed" },
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(state.snapshot(), { navigate });
    expect(document.querySelector("#route-diagnostic")?.classList).toContain(
      "hidden",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "revision:sha256:stale-but-readable",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Exact reading surface is loading",
    );
  });

  it.each([
    {
      name: "successful",
      clipboard: { writeText: vi.fn(async () => undefined) },
      expected: "Copied",
    },
    {
      name: "failed",
      clipboard: { writeText: vi.fn(async () => Promise.reject()) },
      expected: "Copy failed",
    },
    {
      name: "unavailable",
      clipboard: undefined,
      expected: "Copy failed",
    },
  ])("reports $name exact-link copy feedback without navigation", async ({
    clipboard,
    expected,
  }) => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: clipboard,
    });
    const navigate = vi.fn();
    const state = createChangeInspectorState({
      kind: "change",
      changeId: "change:sha256:one",
      query: {},
    });
    state.publish(
      stageGeneration(profile, changes, attention, profile, eventHistory()),
    );
    renderChangeInspector(state.snapshot(), { navigate });
    const copy = Array.from(
      document.querySelectorAll<HTMLButtonElement>("#detail-body button"),
    ).find((button) => button.textContent === "Copy link");
    if (!copy) throw new Error("missing exact-link copy control");

    copy.click();
    await vi.waitFor(() => expect(copy.textContent).toBe(expected));

    expect(navigate).not.toHaveBeenCalled();
    if (clipboard)
      expect(clipboard.writeText).toHaveBeenCalledWith(location.href);
    Reflect.deleteProperty(navigator, "clipboard");
  });

  it("names the Change-detail chooser before its exact Revision and Change identities", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "change",
      changeId: "change:sha256:one",
      query: {},
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(state.snapshot(), { navigate });
    const chooser = Array.from(
      document.querySelectorAll<HTMLButtonElement>("#detail-body button"),
    ).find((button) => button.title?.includes(revision.revisionId));
    expect(chooser?.getAttribute("aria-label")).toBe(
      "Current Revision: open exact Revision revision:sha256:one; artifact sha256:artifact; for Change change:sha256:one",
    );
  });

  it("renders the bound captured snapshot through the retained diff renderer and focuses an exact file", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "resource",
      changeId: "change:sha256:one",
      revision,
      query: {},
      focus: { filePath: "src/lib.rs" },
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(
      state.snapshot(),
      { navigate },
      {
        reading: { kind: "resource", document: exactResource() },
        refusal: null,
      },
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "pub fn restored() {}",
    );
    const file = document.querySelector<HTMLElement>(
      '[data-file-path="src/lib.rs"]',
    );
    expect(file?.dataset.exactFocus).toBe("true");
  });

  it("renders exact fact Markdown and focuses the requested fact without embedding the diff or selecting a peer", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "revision",
      changeId: "change:sha256:one",
      revision,
      query: {},
      focus: { factId: "obs:sha256:focused" },
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(
      state.snapshot(),
      { navigate },
      { reading: revisionReading(), refusal: null },
    );
    const fact = document.querySelector<HTMLElement>(
      '#detail-body .detail-facts [data-fact-id="obs:sha256:focused"]',
    );
    expect(fact?.dataset.exactFocus).toBe("true");
    expect(fact?.textContent).toContain("Rendered fact body");
    expect(document.querySelector("#detail-body .captured-diff")).toBeNull();
    expect(
      document.querySelector("#detail-body .fact-relationship-graph"),
    ).not.toBeNull();
    navigate.mockClear();
    document
      .querySelector<SVGGElement>(
        '#detail-body .fact-relationship-node[data-graph-fact-id="obs:sha256:focused"]',
      )
      ?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(navigate).toHaveBeenCalledWith({
      kind: "revision",
      changeId: "change:sha256:one",
      revision,
      query: {},
      focus: { factId: "obs:sha256:focused" },
    });
    expect(
      document
        .querySelector<HTMLElement>(
          '#detail-body .detail-facts [data-fact-id="obs:sha256:focused"]',
        )
        ?.querySelector("strong")?.textContent,
    ).toBe("Rendered");
  });

  it("renders a full-frame annotated diff from the contextual exact Revision document", () => {
    const navigate = vi.fn();
    const replace = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "diff",
      changeId: "change:sha256:one",
      revision,
      query: {},
      focus: {
        filePath: "src/lib.rs",
        factId: "obs:sha256:focused",
        fileQuery: "has:facts",
      },
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    const revisionDocument = revisionReading().document;
    renderChangeInspector(
      state.snapshot(),
      { navigate, replace },
      { reading: { kind: "diff", document: revisionDocument }, refusal: null },
    );

    expect(document.querySelector("#diff-page")?.classList).not.toContain(
      "hidden",
    );
    expect(document.querySelector(".split")?.classList).toContain("hidden");
    expect(document.querySelectorAll("#diff-page h1")).toHaveLength(1);
    expect(document.querySelector("#diff-page-body")?.textContent).toContain(
      "pub fn restored() {}",
    );
    const focusedFact = document.querySelector<HTMLElement>(
      '.anno[data-anno="obs:sha256:focused"]',
    );
    expect(focusedFact?.dataset.exactFocus).toBe("true");
    expect(focusedFact?.tabIndex).toBe(-1);
    expect(document.activeElement).toBe(focusedFact);
    expect(
      document.querySelector<HTMLInputElement>("#diff-file-query")?.value,
    ).toBe("has:facts");
    document
      .querySelector<HTMLInputElement>("#diff-file-query")
      ?.dispatchEvent(new Event("input", { bubbles: true }));
    expect(replace).toHaveBeenCalled();

    // The persistent full-frame body replaces, rather than accumulates, its
    // delegated route closure on polling repaints.
    renderChangeInspector(
      state.snapshot(),
      { navigate, replace },
      { reading: { kind: "diff", document: revisionDocument }, refusal: null },
    );
    const gutter = document.querySelector<HTMLElement>(
      '#diff-page-body .drow-noted[data-anno="obs:sha256:focused"]',
    );
    navigate.mockClear();
    gutter?.click();
    expect(navigate).toHaveBeenCalledOnce();
    navigate.mockClear();
    gutter?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    expect(navigate).toHaveBeenCalledOnce();
  });

  it("offers the first-class annotated-diff route from exact Revision detail", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "revision",
      changeId: "change:sha256:one",
      revision,
      query: {},
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(
      state.snapshot(),
      { navigate },
      { reading: revisionReading(), refusal: null },
    );
    Array.from(
      document.querySelectorAll<HTMLButtonElement>("#detail-body button"),
    )
      .find((button) => button.textContent === "Open annotated diff")
      ?.click();
    expect(navigate).toHaveBeenCalledWith({
      kind: "diff",
      changeId: "change:sha256:one",
      revision,
      query: {},
    });
  });

  it("retains an unchanged exact detail surface across a polling repaint", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "revision",
      changeId: "change:sha256:one",
      revision,
      query: {},
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    const reading = revisionReading();
    renderChangeInspector(
      state.snapshot(),
      { navigate },
      { reading, refusal: null },
    );
    const detail = document.querySelector<HTMLElement>("#detail-body");
    const fact = document.querySelector("[data-fact-id]");
    if (detail) detail.scrollTop = 19;
    renderChangeInspector(
      state.snapshot(),
      { navigate },
      { reading, refusal: null },
    );
    expect(document.querySelector("[data-fact-id]")).toBe(fact);
    expect(detail?.scrollTop).toBe(19);
  });

  it("re-renders an unchanged exact reading after closing and reopening its route", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const exactRoute = {
      kind: "revision" as const,
      changeId: "change:sha256:one",
      revision,
      query: {},
    };
    const state = createChangeInspectorState(exactRoute);
    state.publish(stageGeneration(profile, changes, attention, profile));
    const reading = revisionReading();
    renderChangeInspector(
      state.snapshot(),
      { navigate },
      { reading, refusal: null },
    );
    expect(document.querySelector("[data-fact-id]")).not.toBeNull();

    state.setRoute({ kind: "lens", lens: "changes", query: {} });
    renderChangeInspector(state.snapshot(), { navigate });
    expect(
      document.querySelector<HTMLElement>("#detail-body")?.dataset
        .changeReadingKey,
    ).toBeUndefined();
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Select a Change",
    );

    state.setRoute(exactRoute);
    renderChangeInspector(
      state.snapshot(),
      { navigate },
      { reading, refusal: null },
    );
    expect(document.querySelector("[data-fact-id]")).not.toBeNull();
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Readable fact",
    );
  });

  it("opens a collapsed captured file through the retained model-neutral handler", () => {
    const navigate = vi.fn();
    const resource = exactResource();
    const exactRevision = {
      revisionId: `revision:sha256:${"a".repeat(64)}`,
      objectArtifactContentHash: `sha256:${"b".repeat(64)}`,
    };
    resource.resource.revision = exactRevision;
    if (!resource.capturedDocument)
      throw new Error("fixture needs captured bytes");
    resource.capturedDocument.snapshot.files = [
      {
        status: "modified",
        new_path: "Cargo.toml",
        is_mode_only: true,
        metadata_rows: [{ text: "mode 100644 → 100755" }],
      },
    ];
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "resource",
      changeId: "change:sha256:one",
      revision: exactRevision,
      query: {},
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(
      state.snapshot(),
      { navigate },
      {
        reading: { kind: "resource", document: resource },
        refusal: null,
      },
    );
    const file = document.querySelector<HTMLElement>(
      '[data-file-path="Cargo.toml"]',
    );
    expect(file?.dataset.expanded).toBe("false");
    file?.querySelector<HTMLElement>(".dfile-head")?.click();
    expect(file?.dataset.expanded).toBe("true");
    expect(file?.textContent).toContain("mode 100644 → 100755");
    const identity = document.querySelector<HTMLElement>(
      "#detail-body > p.mono",
    );
    const fullIdentity = `${exactRevision.revisionId} · ${exactRevision.objectArtifactContentHash}`;
    expect(identity?.textContent).toBe("revision:aaaaaaaa · sha256:bbbbbbbb");
    expect(identity?.title).toBe(fullIdentity);
    expect(identity?.getAttribute("aria-label")).toBe(fullIdentity);
  });

  it("focuses an old rename path without substituting a live diff", () => {
    const navigate = vi.fn();
    const resource = exactResource();
    if (!resource.capturedDocument)
      throw new Error("fixture needs captured bytes");
    resource.capturedDocument.snapshot.files = [
      {
        status: "renamed",
        old_path: "src/old-name.rs",
        new_path: "src/new-name.rs",
        hunks: [],
      },
    ];
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "resource",
      changeId: "change:sha256:one",
      revision,
      query: {},
      focus: { filePath: "src/old-name.rs" },
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(
      state.snapshot(),
      { navigate },
      {
        reading: { kind: "resource", document: resource },
        refusal: null,
      },
    );
    expect(
      document.querySelector<HTMLElement>(
        '[data-old-file-path="src/old-name.rs"]',
      )?.dataset.exactFocus,
    ).toBe("true");
  });

  it("labels a ported fact from its applicable sibling carrier", () => {
    const navigate = vi.fn();
    const reading = revisionReading();
    const origin = {
      revisionId: `revision:sha256:${"1".repeat(64)}`,
      objectArtifactContentHash: `sha256:${"2".repeat(64)}`,
    };
    const fact = reading.document.factPresentations[0];
    if (!fact) throw new Error("fixture needs a fact");
    fact.originRevision = origin;
    fact.presentedInRevision = revision;
    delete fact.target;
    reading.document.factPorts.push({
      portId: "fact-port:sha256:one",
      originRevision: origin,
      originFact: { kind: "observation", observationId: fact.factId },
      targetRevision: revision,
      relation: "carried_open_as",
      actorId: "author",
      trackId: "track:review",
      sourceEventIds: ["event:sha256:one"],
      applicability: "applicable",
      diagnostics: [],
    });
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "revision",
      changeId: "change:sha256:one",
      revision,
      query: {},
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(
      state.snapshot(),
      { navigate },
      { reading, refusal: null },
    );
    expect(
      document.querySelector<HTMLElement>('[data-fact-id="obs:sha256:focused"]')
        ?.textContent,
    ).toContain("port: carried open as (fact-port:sha256:one)");
    const originLine = Array.from(
      document.querySelectorAll<HTMLElement>(
        '[data-fact-id="obs:sha256:focused"] p',
      ),
    ).find((line) => line.textContent?.startsWith("origin:"));
    expect(originLine?.textContent).toContain(
      "revision:11111111 · sha256:22222222",
    );
    expect(originLine?.title).toContain(origin.revisionId);
    expect(originLine?.title).toContain(origin.objectArtifactContentHash);
    expect(originLine?.getAttribute("aria-label")).toContain(origin.revisionId);
  });

  it("retains all exact-local fact families in the captured diff presentation", () => {
    const navigate = vi.fn();
    const reading = revisionReading();
    reading.document.factPresentations.push(
      {
        factId: "request:sha256:one",
        family: "input_request",
        originRevision: revision,
        target: {
          kind: "file",
          revisionId: revision.revisionId,
          filePath: "src/lib.rs",
        },
        actorId: "author",
        revisionCurrency: "current",
        familyState: "current",
        availability: "available",
      },
      {
        factId: "assessment:sha256:one",
        family: "assessment",
        originRevision: revision,
        target: { kind: "revision", revisionId: revision.revisionId },
        actorId: "reviewer",
        revisionCurrency: "current",
        familyState: "current",
        availability: "available",
      },
      {
        factId: "validation:sha256:one",
        family: "validation",
        originRevision: revision,
        actorId: "author",
        revisionCurrency: "current",
        familyState: "current",
        availability: "available",
      },
    );
    const contents = reading.document.factContentPresentations;
    if (!contents) throw new Error("fixture needs fact content");
    Object.assign(contents, {
      "request:sha256:one": {
        contentType: "text/plain",
        bodyContentState: "present",
        content: {
          kind: "input_request",
          title: "Need confirmation",
          status: "answered",
          body: "Question body",
          responses: [
            {
              responseId: "response:sha256:one",
              outcome: "approved",
              reason: "Proceed with the change.",
              contentType: "text/plain",
              bodyContentState: "present",
              availability: "available",
            },
          ],
        },
      },
      "assessment:sha256:one": {
        contentType: "text/plain",
        bodyContentState: "present",
        content: {
          kind: "assessment",
          assessment: "accepted",
          summary: "Decision context assessment",
        },
      },
      "validation:sha256:one": {
        contentType: "text/plain",
        bodyContentState: "present",
        content: {
          kind: "validation",
          checkName: "just test",
          status: "passed",
          command: "just test",
          summary: "All tests passed",
        },
      },
    });
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "diff",
      changeId: "change:sha256:one",
      revision,
      query: {},
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(
      state.snapshot(),
      { navigate },
      { reading: { kind: "diff", document: reading.document }, refusal: null },
    );
    expect(
      document.querySelector(".anno-input-request")?.textContent,
    ).toContain("approved");
    expect(
      document.querySelector(".anno-input-request")?.textContent,
    ).toContain("Proceed with the change.");
    const decisionContext = document.querySelector(".diff-decision-context");
    expect(decisionContext?.textContent).toContain("accepted");
    expect(decisionContext?.textContent).toContain("just test");
    expect(decisionContext?.textContent).toContain("passed");
  });

  it("keeps an interdiff distinct from either authoritative captured Revision", () => {
    const navigate = vi.fn();
    const reading = interdiffReading();
    reading.document.interdiff.from = {
      revisionId: `revision:sha256:${"c".repeat(64)}`,
      objectArtifactContentHash: `sha256:${"d".repeat(64)}`,
    };
    reading.document.interdiff.to = {
      revisionId: `revision:sha256:${"e".repeat(64)}`,
      objectArtifactContentHash: `sha256:${"f".repeat(64)}`,
    };
    const route = {
      kind: "interdiff" as const,
      changeId: "change:sha256:one",
      from: reading.document.interdiff.from,
      to: reading.document.interdiff.to,
      query: {},
    };
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState(route);
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(
      state.snapshot(),
      { navigate },
      { reading, refusal: null },
    );

    const detail = document.querySelector("#detail-body");
    expect(detail?.textContent).toContain("Ordered Revision interdiff");
    expect(detail?.textContent).toContain(
      "This is a comparison, not the authoritative captured diff.",
    );
    expect(detail?.textContent).toContain(
      "revision:cccccccc · sha256:dddddddd → revision:eeeeeeee · sha256:ffffffff",
    );
    expect(detail?.textContent).not.toContain(route.from.revisionId);
    expect(detail?.textContent).not.toContain(route.to.revisionId);
    expect(detail?.textContent).not.toContain("Decision context");
    expect(detail?.querySelector(".diff-decision-context")).toBeNull();

    const capturedDiffButtons = Array.from(
      detail?.querySelectorAll<HTMLButtonElement>("button") ?? [],
    ).filter((button) =>
      button.textContent?.startsWith("Open authoritative captured diff:"),
    );
    expect(capturedDiffButtons).toHaveLength(2);
    expect(capturedDiffButtons[0]?.textContent).toBe(
      "Open authoritative captured diff: revision:cccccccc · sha256:dddddddd",
    );
    expect(capturedDiffButtons[0]?.title).toBe(
      `exact Revision ${route.from.revisionId}; artifact ${route.from.objectArtifactContentHash}`,
    );
    expect(capturedDiffButtons[0]?.dataset.revisionId).toBe(
      route.from.revisionId,
    );
    expect(capturedDiffButtons[0]?.dataset.artifactHash).toBe(
      route.from.objectArtifactContentHash,
    );
    capturedDiffButtons[0]?.click();
    expect(navigate).toHaveBeenCalledWith({
      kind: "resource",
      changeId: route.changeId,
      revision: route.from,
      query: {},
    });
  });

  it("renders effective supersession in predecessor-to-successor reading order", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "change",
      changeId: "change:sha256:one",
      query: {},
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(
      state.snapshot(),
      { navigate },
      {
        reading: changeReading(),
        refusal: null,
      },
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "revision:sha256:predecessor · sha256:predecessor → revision:sha256:one · sha256:artifact",
    );
    expect(
      document.querySelector(
        '#detail-body [data-edge-kind="effective-supersedes"]',
      ),
    ).not.toBeNull();
    const chooser = document.querySelector<HTMLButtonElement>(
      "#detail-body .detail-current-revisions button",
    );
    expect(chooser?.getAttribute("aria-label")).toBe(
      "Current Revision: open exact Revision revision:sha256:one; artifact sha256:artifact; for Change change:sha256:one",
    );
    chooser?.click();
    expect(navigate).toHaveBeenCalledWith({
      kind: "revision",
      changeId: "change:sha256:one",
      revision,
      query: {},
    });
    navigate.mockClear();
    document
      .querySelector<SVGGElement>(
        `#detail-body .change-revision-node[data-revision-id="${revision.revisionId}"]`,
      )
      ?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    expect(navigate).toHaveBeenCalledWith({
      kind: "revision",
      changeId: "change:sha256:one",
      revision,
      query: {},
    });
  });

  it("keeps supplemental Timeline filter labels short while their values stay exact", () => {
    const navigate = vi.fn();
    const changeId = `change:sha256:${"a".repeat(64)}`;
    const exactRevision = {
      revisionId: `revision:sha256:${"b".repeat(64)}`,
      objectArtifactContentHash: `sha256:${"c".repeat(64)}`,
    };
    const history = eventHistory();
    history.completion.changeIds = [changeId];
    history.completion.revisionRefs = [exactRevision];
    const entry = history.entries[0];
    if (!entry) throw new Error("fixture needs an event");
    history.entries = [
      {
        ...entry,
        changeIds: [changeId],
        revisionRefs: [exactRevision],
      },
    ];

    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "timeline",
      historyQuery: {
        change: changeId,
        revision: exactRevision.revisionId,
        artifactHash: exactRevision.objectArtifactContentHash,
      },
    });
    state.publish(
      stageGeneration(profile, changes, attention, profile, history),
    );
    renderChangeInspector(state.snapshot(), { navigate });

    const change = document.querySelector<HTMLSelectElement>(
      "#timeline-filter-change",
    )?.selectedOptions[0];
    expect(change?.textContent).toBe("change:aaaaaaaa");
    expect(change?.value).toBe(changeId);
    expect(change?.getAttribute("title")).toBe(changeId);
    expect(change?.getAttribute("aria-label")).toBe(`Change ${changeId}`);

    const exact = document.querySelector<HTMLSelectElement>(
      "#timeline-filter-revision",
    )?.selectedOptions[0];
    expect(exact?.textContent).toBe("revision:bbbbbbbb · sha256:cccccccc");
    expect(exact?.value).toBe(
      JSON.stringify([
        exactRevision.revisionId,
        exactRevision.objectArtifactContentHash,
      ]),
    );
    expect(exact?.dataset.revisionId).toBe(exactRevision.revisionId);
    expect(exact?.dataset.artifactHash).toBe(
      exactRevision.objectArtifactContentHash,
    );
    expect(exact?.getAttribute("title")).toBe(
      `exact Revision ${exactRevision.revisionId}; artifact ${exactRevision.objectArtifactContentHash}`,
    );
    expect(exact?.getAttribute("aria-label")).toBe(
      `exact Revision ${exactRevision.revisionId}; artifact ${exactRevision.objectArtifactContentHash}`,
    );

    const chips = Array.from(
      document.querySelectorAll<HTMLButtonElement>("#filter-chips button"),
    );
    expect(chips.map((chip) => chip.textContent)).toEqual([
      "change: change:aaaaaaaa ×",
      "revision: revision:bbbbbbbb · sha256:cccccccc ×",
    ]);
    expect(chips[0]?.title).toBe(changeId);
    expect(chips[0]?.getAttribute("aria-label")).toBe(
      `Remove change filter: ${changeId}`,
    );
    expect(chips[1]?.title).toBe(
      `${exactRevision.revisionId} · ${exactRevision.objectArtifactContentHash}`,
    );
    expect(chips[1]?.getAttribute("aria-label")).toBe(
      `Remove revision filter: ${exactRevision.revisionId}; artifact ${exactRevision.objectArtifactContentHash}`,
    );
  });

  it("keeps loading and refusal identities visually short and fully accessible", () => {
    const navigate = vi.fn();
    const changeId = `change:sha256:${"a".repeat(64)}`;
    const exactRevision = {
      revisionId: `revision:sha256:${"b".repeat(64)}`,
      objectArtifactContentHash: `sha256:${"c".repeat(64)}`,
    };
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "revision",
      changeId,
      revision: exactRevision,
      query: {},
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(state.snapshot(), { navigate });

    const identity = document.querySelector<HTMLElement>("#detail-body .mono");
    const full = `Revision ID: ${exactRevision.revisionId} · artifact hash: ${exactRevision.objectArtifactContentHash}`;
    expect(identity?.textContent).toBe(
      "Revision ID: revision:bbbbbbbb · artifact hash: sha256:cccccccc",
    );
    expect(identity?.getAttribute("title")).toBe(full);
    expect(identity?.getAttribute("aria-label")).toBe(full);

    const refusal = `cannot load ${exactRevision.revisionId} with ${exactRevision.objectArtifactContentHash}`;
    renderChangeInspector(
      state.snapshot(),
      { navigate },
      { reading: null, refusal },
    );
    const message = document.querySelector<HTMLElement>("#detail-body .empty");
    const fullMessage = `Reader refused this exact surface: ${refusal}`;
    expect(message?.textContent).toBe(
      "Reader refused this exact surface: cannot load revision:bbbbbbbb with sha256:cccccccc",
    );
    expect(message?.getAttribute("title")).toBe(fullMessage);
    expect(message?.getAttribute("aria-label")).toBe(fullMessage);
  });

  it("keeps Event, journal, and artifact record values short without losing their exact data", () => {
    const navigate = vi.fn();
    const eventId = `evt:sha256:${"d".repeat(64)}`;
    const journalId = `journal:sha256:${"e".repeat(64)}`;
    const payloadHash = `sha256:${"f".repeat(64)}`;
    const history = eventHistory();
    const entry = history.entries[0];
    if (!entry) throw new Error("fixture needs an event");
    history.entries = [{ ...entry, eventId, journalId, payloadHash }];

    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "event",
      eventId,
      historyQuery: {},
      query: {},
    });
    state.publish(
      stageGeneration(profile, changes, attention, profile, history),
    );
    renderChangeInspector(state.snapshot(), { navigate });

    const identity = document.querySelector<HTMLElement>(
      "#detail-body > .mono",
    );
    expect(identity?.textContent).toBe("evt:dddddddd");
    expect(identity?.getAttribute("title")).toBe(eventId);
    expect(identity?.getAttribute("aria-label")).toBe(`event ${eventId}`);
    expect(identity?.dataset.eventId).toBe(eventId);

    const recordValue = (label: string): HTMLElement | null => {
      const term = Array.from(
        document.querySelectorAll<HTMLElement>(
          "#detail-body .event-detail-record dt",
        ),
      ).find((candidate) => candidate.textContent === label);
      return term?.nextElementSibling as HTMLElement | null;
    };
    const payload = recordValue("event payload");
    expect(payload?.textContent).toBe("sha256:ffffffff");
    expect(payload?.getAttribute("title")).toBe(payloadHash);
    expect(payload?.getAttribute("aria-label")).toBe(`artifact ${payloadHash}`);
    expect(payload?.dataset.artifactHash).toBe(payloadHash);

    const journal = recordValue("journal");
    expect(journal?.textContent).toBe("journal:eeeeeeee");
    expect(journal?.getAttribute("title")).toBe(journalId);
    expect(journal?.getAttribute("aria-label")).toBe(`journal ${journalId}`);
    expect(journal?.dataset.journalId).toBe(journalId);
  });
});

resetDom();
