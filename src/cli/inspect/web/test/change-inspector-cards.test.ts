import { describe, expect, it } from "vitest";
import { changeCardPresentation } from "../src/change-inspector-cards";
import type { ChangeSummary } from "../src/change-protocol";

const summary: ChangeSummary = {
  changeId: "change:sha256:one",
  declarationState: "authoritative",
  titleAssertions: [],
  memberCount: 2,
  topology: "parallel_current",
  lifecycle: "in_progress",
  attentionSummary: "conflicted",
  availabilitySummary: "incomplete",
  currentRevisionRefs: [
    {
      revisionId: "revision:sha256:aaa",
      objectArtifactContentHash: "sha256:a",
    },
    {
      revisionId: "revision:sha256:bbb",
      objectArtifactContentHash: "sha256:b",
    },
  ],
  projectionStamp: "sha256:generation",
};

describe("Change cards", () => {
  it("uses server presentations and exposes every current peer explicitly", () => {
    const [first, second] = summary.currentRevisionRefs;
    if (first === undefined || second === undefined)
      throw new Error("fixture peers missing");
    const card = changeCardPresentation(summary, {
      currentRevisions: [
        {
          revision: first,
          revisionProposalSummary: "Review parser",
          summarySource: "revision_proposal_summary",
        },
        { revision: second, summarySource: "absent" },
      ],
    });
    expect(card.peers).toHaveLength(2);
    expect(card.peers[0]?.label).toContain("Review parser");
    expect(card.peers[1]?.label).toContain("revision:sha256");
    expect(card.accessibleName).toBe(
      "Current Revisions — Review parser; exact Revision revision:sha256:aaa; artifact sha256:a; exact Revision revision:sha256:bbb; artifact sha256:b; Change change:sha256:one",
    );
    expect(card.peers[0]?.accessibleName).toBe(
      "Current Revision — Review parser; exact Revision revision:sha256:aaa; artifact sha256:a",
    );
    expect(card.peers[0]?.copyText).toBe("revision:sha256:aaa sha256:a");
    expect(card.badges).toEqual([
      "parallel current",
      "in progress",
      "conflicted",
      "incomplete",
    ]);
  });
});
