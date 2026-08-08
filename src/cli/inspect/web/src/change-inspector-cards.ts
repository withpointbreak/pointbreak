/** Server-owned Change card presentation. No Change title or prose is inferred here. */

import type {
  ChangeAttentionPresentation,
  ChangeAttentionReason,
  ChangePresentation,
  ChangeSummary,
  RevisionRef,
} from "./change-protocol";

export interface ChangeCardStateAxis {
  label: "Topology" | "Lifecycle" | "Attention" | "Availability";
  value: string;
}

export interface ChangeCardAttentionReason {
  kind: ChangeAttentionReason["kind"];
  reason: string;
  ask: string;
  actionLabel: string;
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
  actionLabel: ChangeCardAttentionReason["actionLabel"];
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

function shortened(value: string, limit: number): string {
  return value.length > limit ? `${value.slice(0, limit)}…` : value;
}

function shortExact(revision: RevisionRef): string {
  return `${shortened(revision.revisionId, 24)} · ${shortened(
    revision.objectArtifactContentHash,
    18,
  )}`;
}

function exactRevisionCopyText(revisions: RevisionRef[]): string {
  return revisions
    .map(
      (revision) =>
        `${revision.revisionId} ${revision.objectArtifactContentHash}`,
    )
    .join("\n");
}

/** One spoken form for the complete identity of an exact Revision resource. */
export function exactRevisionAccessibleIdentity(revision: RevisionRef): string {
  return `exact Revision ${revision.revisionId}; artifact ${revision.objectArtifactContentHash}`;
}

function attentionReasonCopy(
  reason: ChangeAttentionReason,
): ChangeCardAttentionReason {
  switch (reason.kind) {
    case "conflicted":
      return {
        kind: reason.kind,
        reason: "Conflicting Change state",
        ask: "Resolve the conflicting Change state.",
        actionLabel: "Review conflict",
        accessibleName:
          "Conflicting Change state. Resolve the conflicting Change state.",
        title: "Conflicting Change state",
        copyText: "conflicted",
      };
    case "incomplete":
      return {
        kind: reason.kind,
        reason: "Incomplete Change state",
        ask: "Complete the missing Change state.",
        actionLabel: "Review incomplete Change",
        accessibleName:
          "Incomplete Change state. Complete the missing Change state.",
        title: "Incomplete Change state",
        copyText: "incomplete",
      };
    case "no_current_revision":
      return {
        kind: reason.kind,
        reason: "No current Revision",
        ask: "Establish one exact current Revision before review can continue.",
        actionLabel: "Review Change",
        accessibleName:
          "No current Revision. Establish one exact current Revision before review can continue.",
        title: "No current Revision",
        copyText: "no_current_revision",
      };
    case "unresolved_operative_requests": {
      const visibleRequestIds = reason.requestIds.map((id) =>
        shortened(id, 24),
      );
      const requestList = visibleRequestIds.join(", ");
      const fullRequestList = reason.requestIds.join(", ");
      return {
        kind: reason.kind,
        reason: "Unresolved operative requests",
        ask: `Respond to operative requests: ${requestList}.`,
        actionLabel: "Respond to requests",
        accessibleName: `Unresolved operative requests. Respond to operative requests: ${fullRequestList}.`,
        title: `Operative requests: ${fullRequestList}`,
        copyText: reason.requestIds.join("\n"),
      };
    }
    case "current_revisions_need_assessment": {
      const visibleRevisions = reason.revisions.map(shortExact).join(", ");
      const fullRevisions = reason.revisions
        .map(exactRevisionAccessibleIdentity)
        .join("; ");
      return {
        kind: reason.kind,
        reason: "Current Revisions need assessment",
        ask: `Assess current Revisions: ${visibleRevisions}.`,
        actionLabel: "Assess current Revisions",
        accessibleName: `Current Revisions need assessment. Assess ${fullRevisions}.`,
        title: fullRevisions,
        copyText: exactRevisionCopyText(reason.revisions),
      };
    }
  }
}

function attentionPresentation(
  attention: ChangeAttentionPresentation | undefined,
): ChangeCardAttention | undefined {
  if (attention === undefined) return undefined;
  const primary = attentionReasonCopy(attention.primaryReason);
  return {
    primary,
    reason: primary.reason,
    ask: primary.ask,
    actionLabel: primary.actionLabel,
    additionalReasons: attention.reasons.slice(1).map(attentionReasonCopy),
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
      visibleIdentity: shortExact(revision),
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
    visibleChangeId: shortened(summary.changeId, 28),
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
