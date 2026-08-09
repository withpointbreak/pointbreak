import { describe, expect, it } from "vitest";
import { changeCardPresentation } from "../src/change-inspector-cards";
import type {
  ChangeAttentionReason,
  ChangePresentation,
  ChangeSummary,
  RevisionRef,
} from "../src/change-protocol";

const longChangeId = "change:sha256:012345678901234567890123456789abcdef";
const first: RevisionRef = {
  revisionId: "revision:sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  objectArtifactContentHash: "sha256:11111111111111111111111111111111",
};
const second: RevisionRef = {
  revisionId: "revision:sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
  objectArtifactContentHash: "sha256:22222222222222222222222222222222",
};

function summary(currentRevisionRefs: RevisionRef[]): ChangeSummary {
  return {
    changeId: longChangeId,
    declarationState: "authoritative",
    titleAssertions: [],
    memberCount: 2,
    topology: "parallel_current",
    lifecycle: "in_progress",
    attentionSummary: "conflicted",
    availabilitySummary: "incomplete",
    currentRevisionRefs,
    projectionStamp: "sha256:generation",
  };
}

function presentation(
  currentRevisions: ChangePresentation["currentRevisions"],
  attention?: ChangePresentation["attention"],
): ChangePresentation {
  return {
    currentRevisions,
    ...(attention === undefined ? {} : { attention }),
  };
}

describe("Change cards", () => {
  it("leads a one-current-Revision card with the server proposal summary", () => {
    const card = changeCardPresentation(
      summary([first]),
      presentation([
        {
          revision: first,
          revisionProposalSummary: "Preserve atomic captures",
          summarySource: "revision_proposal_summary",
        },
      ]),
    );

    expect(card.headline).toBe("Preserve atomic captures");
    expect(card.primaryAction).toEqual({
      kind: "open_change",
      label: "Review Change",
    });
    expect(card.peers[0]).toMatchObject({
      label: "Preserve atomic captures",
      copyText: `${first.revisionId} ${first.objectArtifactContentHash}`,
      title: `exact Revision ${first.revisionId}; artifact ${first.objectArtifactContentHash}`,
    });
    expect(card.visibleChangeId).toBe("change:01234567");
    expect(card.peers[0]?.visibleIdentity).toBe(
      "revision:aaaaaaaa · sha256:11111111",
    );
    expect(card.accessibleName).toContain(longChangeId);
    expect(card.peers[0]?.accessibleName).toContain(first.revisionId);
  });

  it("exposes every state axis with a label", () => {
    const card = changeCardPresentation(summary([first]), presentation([]));

    expect(card.stateAxes).toEqual([
      { label: "Topology", value: "parallel current" },
      { label: "Lifecycle", value: "in progress" },
      { label: "Attention", value: "conflicted" },
      { label: "Availability", value: "incomplete" },
    ]);
  });

  it("keeps parallel peers explicit and opens the Change instead of selecting one", () => {
    const card = changeCardPresentation(
      summary([first, second]),
      presentation([
        {
          revision: first,
          revisionProposalSummary: "First proposal",
          summarySource: "revision_proposal_summary",
        },
        { revision: second, summarySource: "absent" },
      ]),
    );

    expect(card.headline).toBe("Multiple current Revisions need selection");
    expect(card.peers).toHaveLength(2);
    expect(card.peers.map((peer) => peer.revision)).toEqual([first, second]);
    expect(card.primaryAction).toEqual({
      kind: "open_change",
      label: "Review current Revisions",
    });
  });

  it("explains when no exact current Revision is available", () => {
    const card = changeCardPresentation(summary([]), presentation([]));

    expect(card.headline).toBe("Current Revision unavailable");
    expect(card.unavailableReason).toBe(
      "No exact current Revision is available for this Change.",
    );
    expect(card.primaryAction).toEqual({
      kind: "open_change",
      label: "Review Change",
    });
  });
});

describe("Change card Attention", () => {
  const cases: Array<{
    reason: ChangeAttentionReason;
    expected: {
      reason: string;
      ask: string;
      actionLabel: string;
      copyText: string;
    };
  }> = [
    {
      reason: { kind: "conflicted" },
      expected: {
        reason: "Conflicting Change state",
        ask: "Resolve the conflicting Change state.",
        actionLabel: "Review conflict",
        copyText: "conflicted",
      },
    },
    {
      reason: { kind: "incomplete" },
      expected: {
        reason: "Incomplete Change state",
        ask: "Complete the missing Change state.",
        actionLabel: "Review incomplete Change",
        copyText: "incomplete",
      },
    },
    {
      reason: { kind: "no_current_revision" },
      expected: {
        reason: "No current Revision",
        ask: "Establish one exact current Revision before review can continue.",
        actionLabel: "Review Change",
        copyText: "no_current_revision",
      },
    },
    {
      reason: {
        kind: "unresolved_operative_requests",
        requestIds: [
          "request:sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
          "request:sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ],
      },
      expected: {
        reason: "Unresolved operative requests",
        ask: "Respond to operative requests: request:aaaaaaaa, request:bbbbbbbb.",
        actionLabel: "Respond to requests",
        copyText:
          "request:sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaa\nrequest:sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      },
    },
    {
      reason: { kind: "current_revisions_need_assessment", revisions: [first] },
      expected: {
        reason: "Current Revisions need assessment",
        ask: "Assess current Revisions: revision:aaaaaaaa · sha256:11111111.",
        actionLabel: "Assess current Revisions",
        copyText: `${first.revisionId} ${first.objectArtifactContentHash}`,
      },
    },
  ];

  it.each(
    cases,
  )("uses the server primary $reason.kind without client ranking", ({
    reason,
    expected,
  }) => {
    const card = changeCardPresentation(
      summary([first]),
      presentation([], {
        primaryReason: reason,
        reasons: [reason, { kind: "conflicted" }],
        diagnostics: ["server diagnostic"],
      }),
    );

    expect(card.attention).toMatchObject({
      reason: expected.reason,
      ask: expected.ask,
      actionLabel: expected.actionLabel,
      diagnostics: ["server diagnostic"],
      primary: expected,
    });
    expect(card.attention?.additionalReasons).toHaveLength(1);
    expect(card.attention?.additionalReasons[0]?.kind).toBe("conflicted");
  });

  it("retains the server order for every additional Attention reason", () => {
    const card = changeCardPresentation(
      summary([first]),
      presentation([], {
        primaryReason: { kind: "incomplete" },
        reasons: [
          { kind: "incomplete" },
          { kind: "no_current_revision" },
          { kind: "conflicted" },
        ],
      }),
    );

    expect(card.attention?.additionalReasons.map(({ kind }) => kind)).toEqual([
      "no_current_revision",
      "conflicted",
    ]);
  });
});
