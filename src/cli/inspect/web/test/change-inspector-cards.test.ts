import { describe, expect, it } from "vitest";
import {
  changeCardPresentation,
  exactRevisionAccessibleIdentity,
} from "../src/change-inspector-cards";
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
        {
          revision: second,
          summarySource: "absent",
          absentSummaryCue: "No summary supplied",
        },
      ]),
    );

    expect(card.headline).toBe("Multiple current Revisions need selection");
    // The card-level cue belongs to a one-current headline only.
    expect(card.absentSummaryCue).toBeUndefined();
    expect(card.peers).toHaveLength(2);
    expect(card.peers[0]).toMatchObject({
      label: "First proposal",
      visibleIdentity: "revision:aaaaaaaa · sha256:11111111",
      title: exactRevisionAccessibleIdentity(first),
    });
    expect(card.peers[0]?.absentSummaryCue).toBeUndefined();
    expect(card.peers[1]?.label).toBeUndefined();
    expect(card.peers[1]).toMatchObject({
      absentSummaryCue: "No summary supplied",
      visibleIdentity: "revision:bbbbbbbb · sha256:22222222",
      title: exactRevisionAccessibleIdentity(second),
      accessibleName: `Current Revision — ${exactRevisionAccessibleIdentity(second)}; No summary supplied`,
    });
    expect(card.peers.map((peer) => peer.revision)).toEqual([first, second]);
    expect(card.primaryAction).toEqual({
      kind: "open_change",
      label: "Review current Revisions",
    });
  });

  it("explains when no exact current Revision is available", () => {
    const card = changeCardPresentation(summary([]), presentation([]));

    expect(card.headline).toBe("Current Revision unavailable");
    expect(card.absentSummaryCue).toBeUndefined();
    expect(card.peers).toEqual([]);
    expect(card.unavailableReason).toBe(
      "No exact current Revision is available for this Change.",
    );
    expect(card.primaryAction).toEqual({
      kind: "open_change",
      label: "Review Change",
    });
  });

  it("keeps the exact Revision id as the headline and shows the server absent-summary cue", () => {
    const card = changeCardPresentation(
      summary([first]),
      presentation([
        {
          revision: first,
          summarySource: "absent",
          absentSummaryCue: "No summary supplied",
        },
      ]),
    );

    // #752: the exact id is the headline; the cue is a separate state line.
    expect(card.headline).toBe("revision:aaaaaaaa · sha256:11111111");
    expect(card.absentSummaryCue).toBe("No summary supplied");
    expect(card.peers[0]?.label).toBeUndefined();
    expect(card.peers[0]?.absentSummaryCue).toBe("No summary supplied");
    // The accessible name stays identity-led and names the absence.
    expect(card.peers[0]?.accessibleName).toBe(
      `Current Revision — ${exactRevisionAccessibleIdentity(first)}; No summary supplied`,
    );
    expect(card.accessibleName).toContain(first.revisionId);
    expect(card.accessibleName).toContain(first.objectArtifactContentHash);
    expect(card.accessibleName).not.toContain("Current Revision;");
  });

  it("never invents a headline for an absent summary from an older server", () => {
    // A pre-#752 server sent a label for an absent summary and no cue; an
    // older one sent neither. Neither becomes the headline.
    for (const entry of [
      {
        revision: first,
        summarySource: "absent" as const,
        label: "No summary at capture",
      },
      { revision: first, summarySource: "absent" as const },
    ]) {
      const card = changeCardPresentation(
        summary([first]),
        presentation([entry]),
      );
      expect(card.headline).toBe("revision:aaaaaaaa · sha256:11111111");
      expect(card.absentSummaryCue).toBeUndefined();
      expect(card.peers[0]?.label).toBeUndefined();
      expect(card.peers[0]?.accessibleName).toBe(
        `Current Revision — ${exactRevisionAccessibleIdentity(first)}`,
      );
    }
  });

  it("sources the supplied-case accessible name from the server label, not the raw summary", () => {
    // Future-proofing: if server policy ever lets the finished label diverge
    // from the stored proposal summary, visible and accessible copy must agree.
    const card = changeCardPresentation(
      summary([first]),
      presentation([
        {
          revision: first,
          revisionProposalSummary: "raw stored summary",
          summarySource: "revision_proposal_summary",
          label: "finished server label",
        },
      ]),
    );

    expect(card.peers[0]?.label).toBe("finished server label");
    expect(card.peers[0]?.accessibleName).toBe(
      `Current Revision — finished server label; ${exactRevisionAccessibleIdentity(first)}`,
    );
  });

  it("leads with a supplied summary when an older server sends no label", () => {
    const card = changeCardPresentation(
      summary([first]),
      presentation([
        {
          revision: first,
          revisionProposalSummary: "Older server summary",
          summarySource: "revision_proposal_summary",
        },
      ]),
    );

    expect(card.headline).toBe("Older server summary");
    expect(card.absentSummaryCue).toBeUndefined();
  });
});

describe("Change card Attention", () => {
  it("passes through distinct server-authored ask, reason, evidence, and next action", () => {
    const cause: ChangeAttentionReason = {
      kind: "current_revisions_need_assessment",
      revisions: [first],
    };
    const attention = {
      primaryReason: cause,
      reasons: [cause],
      reasonPresentations: [
        {
          cause,
          ask: "ASK SENTINEL",
          reason: "REASON SENTINEL",
          evidence: "EVIDENCE SENTINEL",
          nextAction: "NEXT ACTION SENTINEL",
        },
      ],
    } satisfies NonNullable<ChangePresentation["attention"]>;
    const card = changeCardPresentation(
      summary([first]),
      presentation([], attention),
    );

    expect(card.attention).toMatchObject({
      ask: "ASK SENTINEL",
      reason: "REASON SENTINEL",
      evidence: "EVIDENCE SENTINEL",
      nextAction: "NEXT ACTION SENTINEL",
    });
  });

  it("retains the server order for every additional Attention reason", () => {
    const reasons: ChangeAttentionReason[] = [
      { kind: "incomplete" },
      { kind: "no_current_revision" },
      { kind: "conflicted" },
    ];
    const reasonPresentations = reasons.map((cause, index) => ({
      cause,
      ask: `ASK ${index}`,
      reason: `REASON ${index}`,
      evidence: `EVIDENCE ${index}`,
      nextAction: `NEXT ${index}`,
    }));
    const card = changeCardPresentation(
      summary([first]),
      presentation([], {
        primaryReason: reasons[0],
        reasons,
        reasonPresentations,
        diagnostics: ["server diagnostic"],
      }),
    );

    expect(card.attention).toMatchObject({
      reason: "REASON 0",
      ask: "ASK 0",
      evidence: "EVIDENCE 0",
      nextAction: "NEXT 0",
      diagnostics: ["server diagnostic"],
    });
    expect(card.attention?.additionalReasons).toMatchObject([
      {
        kind: "no_current_revision",
        reason: "REASON 1",
        ask: "ASK 1",
        evidence: "EVIDENCE 1",
        nextAction: "NEXT 1",
      },
      {
        kind: "conflicted",
        reason: "REASON 2",
        ask: "ASK 2",
        evidence: "EVIDENCE 2",
        nextAction: "NEXT 2",
      },
    ]);
  });
});
