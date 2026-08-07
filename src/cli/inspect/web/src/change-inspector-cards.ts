/** Server-owned Change card presentation. No Change title or prose is inferred here. */

import type {
  ChangePresentation,
  ChangeSummary,
  RevisionRef,
} from "./change-protocol";

export interface ChangeCardPresentation {
  changeId: string;
  badges: string[];
  peers: Array<{ revision: RevisionRef; label: string; copyText: string }>;
}

function words(value: string): string {
  return value.replaceAll("_", " ");
}

function shortExact(revision: RevisionRef): string {
  const revisionId =
    revision.revisionId.length > 24
      ? `${revision.revisionId.slice(0, 24)}…`
      : revision.revisionId;
  const artifact =
    revision.objectArtifactContentHash.length > 18
      ? `${revision.objectArtifactContentHash.slice(0, 18)}…`
      : revision.objectArtifactContentHash;
  return `${revisionId} · ${artifact}`;
}

export function changeCardPresentation(
  summary: ChangeSummary,
  presentation: ChangePresentation | undefined,
): ChangeCardPresentation {
  const byExactIdentity = new Map(
    (presentation?.currentRevisions ?? []).map((entry) => [
      `${entry.revision.revisionId}\u0000${entry.revision.objectArtifactContentHash}`,
      entry,
    ]),
  );
  return {
    changeId: summary.changeId,
    badges: [
      summary.topology,
      summary.lifecycle,
      summary.attentionSummary,
      summary.availabilitySummary,
    ].map(words),
    peers: summary.currentRevisionRefs.map((revision) => {
      const entry = byExactIdentity.get(
        `${revision.revisionId}\u0000${revision.objectArtifactContentHash}`,
      );
      const summaryLabel =
        entry?.summarySource === "revision_proposal_summary"
          ? entry.revisionProposalSummary
          : undefined;
      return {
        revision,
        label: summaryLabel
          ? `Current Revision — ${summaryLabel}`
          : `Current Revision — ${shortExact(revision)}`,
        copyText: `${revision.revisionId} ${revision.objectArtifactContentHash}`,
      };
    }),
  };
}
