/** Server-owned Change card presentation. No Change title or prose is inferred here. */

import type {
  ChangeAttentionPresentation,
  ChangeAttentionReason,
  ChangeAttentionReasonPresentation,
  ChangePresentation,
  ChangeSummary,
  RevisionRef,
} from "./change-protocol";
import {
  exactRevisionAccessibleIdentity,
  shortExactRevision,
  shortRef,
} from "./refs";

export { exactRevisionAccessibleIdentity } from "./refs";

export interface ChangeCardStateAxis {
  label: "Topology" | "Lifecycle" | "Attention" | "Availability";
  value: string;
}

export interface ChangeCardAttentionReason {
  kind: ChangeAttentionReason["kind"];
  reason: string;
  ask: string;
  evidence: string;
  nextAction: string;
  /** Complete source identities for assistive technology and hover text. */
  accessibleName: string;
  title: string;
  /** Complete source identities suitable for a copy affordance. */
  copyText: string;
}

export interface ChangeCardAttention {
  /** The complete primary copy, including full identity-bearing values. */
  primary: ChangeCardAttentionReason;
  reason: ChangeCardAttentionReason["reason"];
  ask: ChangeCardAttentionReason["ask"];
  evidence: ChangeCardAttentionReason["evidence"];
  nextAction: ChangeCardAttentionReason["nextAction"];
  /** Additional causes retain the server's primary-first order. */
  additionalReasons: ChangeCardAttentionReason[];
  diagnostics?: string[];
}

export interface ChangeCardPeer {
  revision: RevisionRef;
  /** Proposal-first visible label, when the server supplied one. */
  label: string;
  /** Short exact identity for visual presentation. */
  visibleIdentity: string;
  accessibleName: string;
  title: string;
  copyText: string;
}

export interface ChangeCardPrimaryAction {
  kind: "open_change";
  label: "Review current Revisions" | "Review Change";
}

export interface ChangeCardPresentation {
  changeId: string;
  visibleChangeId: string;
  accessibleName: string;
  title: string;
  copyText: string;
  /** Meaning leads identity: a proposal summary when exactly one is current. */
  headline: string;
  stateAxes: ChangeCardStateAxis[];
  peers: ChangeCardPeer[];
  /** Explicitly explains why an exact Revision cannot be opened. */
  unavailableReason?: string;
  /** Exactly one work-object action; Revision peers are always a later choice. */
  primaryAction: ChangeCardPrimaryAction;
  /** Present only when the server supplied Attention presentation. */
  attention?: ChangeCardAttention;
}

function words(value: string): string {
  return value.replaceAll("_", " ");
}

function exactRevisionCopyText(revisions: RevisionRef[]): string {
  return revisions
    .map(
      (revision) =>
        `${revision.revisionId} ${revision.objectArtifactContentHash}`,
    )
    .join("\n");
}

function attentionReasonCopy(
  presentation: ChangeAttentionReasonPresentation,
): ChangeCardAttentionReason {
  const { cause, ask, reason, evidence, nextAction } = presentation;
  return {
    kind: cause.kind,
    reason,
    ask,
    evidence,
    nextAction,
    accessibleName: `${reason} ${ask} ${evidence} Next: ${nextAction}`,
    title: evidence,
    copyText: [reason, ask, evidence, nextAction].join("\n"),
  };
}

function attentionPresentation(
  attention: ChangeAttentionPresentation | undefined,
): ChangeCardAttention | undefined {
  if (attention === undefined) return undefined;
  const primary = attentionReasonCopy(attention.reasonPresentations[0]);
  return {
    primary,
    reason: primary.reason,
    ask: primary.ask,
    evidence: primary.evidence,
    nextAction: primary.nextAction,
    additionalReasons: attention.reasonPresentations
      .slice(1)
      .map(attentionReasonCopy),
    ...(attention.diagnostics === undefined
      ? {}
      : { diagnostics: attention.diagnostics }),
  };
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
  const peers = summary.currentRevisionRefs.map((revision) => {
    const entry = byExactIdentity.get(
      `${revision.revisionId}\u0000${revision.objectArtifactContentHash}`,
    );
    const summaryLabel =
      entry?.summarySource === "revision_proposal_summary"
        ? entry.revisionProposalSummary
        : undefined;
    const identity = exactRevisionAccessibleIdentity(revision);
    return {
      revision,
      label: summaryLabel || "Current Revision",
      visibleIdentity: shortExactRevision(revision),
      accessibleName: summaryLabel
        ? `Current Revision — ${summaryLabel}; ${identity}`
        : `Current Revision — ${identity}`,
      title: identity,
      copyText: exactRevisionCopyText([revision]),
    };
  });
  const onlyPeer = peers.length === 1 ? peers[0] : undefined;
  const headline =
    onlyPeer === undefined
      ? peers.length === 0
        ? "Current Revision unavailable"
        : "Multiple current Revisions need selection"
      : onlyPeer.label;
  const currentRevisionName =
    peers.length === 0
      ? "Current Revision unavailable"
      : peers.length === 1
        ? peers[0].accessibleName
        : `Current Revisions — ${peers
            .map((peer) =>
              peer.accessibleName.replace(/^Current Revision — /, ""),
            )
            .join("; ")}`;
  const unavailableReason =
    peers.length === 0
      ? "No exact current Revision is available for this Change."
      : undefined;
  return {
    changeId: summary.changeId,
    visibleChangeId: shortRef(summary.changeId),
    accessibleName: `${headline}; ${currentRevisionName}; Change ${summary.changeId}`,
    title: `Change ${summary.changeId}`,
    copyText: summary.changeId,
    headline,
    stateAxes: [
      { label: "Topology", value: words(summary.topology) },
      { label: "Lifecycle", value: words(summary.lifecycle) },
      { label: "Attention", value: words(summary.attentionSummary) },
      { label: "Availability", value: words(summary.availabilitySummary) },
    ],
    peers,
    ...(unavailableReason === undefined ? {} : { unavailableReason }),
    primaryAction: {
      kind: "open_change",
      label: peers.length > 1 ? "Review current Revisions" : "Review Change",
    },
    ...(presentation?.attention === undefined
      ? {}
      : { attention: attentionPresentation(presentation.attention) }),
  };
}
