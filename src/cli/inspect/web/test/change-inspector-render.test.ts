import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChangeInspectorReading } from "../src/change-inspector-reading";
import {
  prepareChangeInspectorShell,
  renderChangeInspector,
  renderChangeInspectorRefusal,
} from "../src/change-inspector-render";
import {
  createChangeInspectorState,
  stageGeneration,
} from "../src/change-inspector-state";
import type {
  AttentionPage,
  ChangeDetail,
  ChangesPage,
  ReaderProfile,
  RevisionResource,
} from "../src/change-protocol";
import { mountInspectorDom, resetDom } from "./support/dom";

const profile: ReaderProfile = {
  schema: "pointbreak.inspect-reader-profile",
  version: 1,
  availability: "ready",
  authorityCursor: { eventCount: 2 },
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
};

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
    },
  };
}

function changeReading(): ChangeInspectorReading {
  const predecessor = {
    revisionId: "revision:sha256:predecessor",
    objectArtifactContentHash: "sha256:predecessor",
  };
  const successor = {
    revisionId: "revision:sha256:successor",
    objectArtifactContentHash: "sha256:successor",
  };
  const summary = changes.changes[0];
  if (!summary) throw new Error("fixture needs Change summary");
  const document: ChangeDetail = {
    schema: "pointbreak.review-change",
    version: 1,
    summary,
    memberRevisions: [],
    unavailableMemberRevisions: [],
    membershipClaims: [],
    membershipWithdrawals: [],
    relationClaims: [],
    relationWithdrawals: [],
    links: [],
    effectiveSupersedes: [[successor, predecessor]],
    pendingOrConflictingEdges: [],
    currentRevisionRefs: [revision],
    perCurrentRevisionQualification: [],
    operativeObligations: [],
    diagnostics: [],
    projectionStamp: "sha256:generation",
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
      firstCard?.querySelector(".change-card-badges")?.textContent,
    ).toContain("conflicted");
    expect(document.querySelector("#stat-threads")?.textContent).toBe(
      "1 need attention",
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
      "Current Revision — Server proposal; exact Revision revision:sha256:one; artifact sha256:artifact; Change change:sha256:one",
    );
    expect(
      document
        .querySelector(".change-card-peer button:last-child")
        ?.getAttribute("aria-label"),
    ).toBe(
      "Copy exact Revision revision:sha256:one; artifact sha256:artifact; for Change change:sha256:one",
    );
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
    ).find((button) => button.textContent === revision.revisionId);
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

  it("renders exact fact Markdown and focuses the requested fact without selecting a peer", () => {
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
      '[data-anno="obs:sha256:focused"]',
    );
    expect(fact?.dataset.exactFocus).toBe("true");
    expect(
      document.querySelector<HTMLElement>(
        '.anno[data-anno="obs:sha256:focused"]',
      )?.textContent,
    ).toContain("Rendered fact body");
    expect(
      document
        .querySelector<HTMLElement>('[data-fact-id="obs:sha256:focused"]')
        ?.querySelector("strong")?.textContent,
    ).toBe("Rendered");
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
      revision,
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
      revisionId: "revision:sha256:origin",
      objectArtifactContentHash: "sha256:origin-artifact",
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
    expect(detail?.textContent).toContain(route.from.revisionId);
    expect(detail?.textContent).toContain(route.to.revisionId);
    expect(detail?.textContent).not.toContain("Decision context");
    expect(detail?.querySelector(".diff-decision-context")).toBeNull();

    const capturedDiffButtons = Array.from(
      detail?.querySelectorAll<HTMLButtonElement>("button") ?? [],
    ).filter((button) =>
      button.textContent?.startsWith("Open authoritative captured diff:"),
    );
    expect(capturedDiffButtons).toHaveLength(2);
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
      "revision:sha256:predecessor · sha256:predecessor → revision:sha256:successor · sha256:successor",
    );
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
  });
});

resetDom();
