/**
 * Readable presentation for the closed Change-aware Timeline vocabulary.
 *
 * The server owns event typing and semantic correlation. These helpers only
 * name the already-typed fields; they never infer a Change or choose a Revision.
 */

import type {
  EventHistoryEntry,
  EventHistoryEventType,
  EventHistoryRevisionRef,
  EventHistorySubject,
  FactRef,
  FactTarget,
} from "./change-protocol";

export interface EventPresentationField {
  label: string;
  value: string;
}

export interface EventPresentation {
  label: string;
  title: string;
  body?: string;
  fields: EventPresentationField[];
}

function words(value: string): string {
  return value.replaceAll("_", " ");
}

function revision(reference: EventHistoryRevisionRef): string {
  return `${reference.revisionId} · ${reference.objectArtifactContentHash}`;
}

function fact(reference: FactRef): string {
  return reference.kind === "observation"
    ? `observation ${reference.observationId ?? "unknown"}`
    : `input request ${reference.inputRequestId ?? "unknown"}`;
}

export function eventTargetLabel(target: FactTarget): string {
  switch (target.kind) {
    case "revision":
      return `Revision ${target.revisionId}`;
    case "file":
      return `File ${target.filePath ?? "unknown"} in ${target.revisionId}`;
    case "range":
      return `${target.filePath ?? "unknown"}:${target.startLine ?? "?"}-${target.endLine ?? "?"} (${target.side ?? "?"}) in ${target.revisionId}`;
    case "observation":
      return `Observation ${target.observationId ?? "unknown"} in ${target.revisionId}`;
    case "input_request":
      return `Input request ${target.inputRequestId ?? "unknown"} in ${target.revisionId}`;
    case "assessment":
      return `Assessment ${target.assessmentId ?? "unknown"} in ${target.revisionId}`;
    case "event":
      return `Event ${target.eventId ?? "unknown"} in ${target.revisionId}`;
  }
}

export function eventSubjectLabel(subject: EventHistorySubject): string {
  switch (subject.kind) {
    case "journal":
      return `Journal ${subject.journalId}`;
    case "review":
      return eventTargetLabel(subject.target);
    case "change":
      return `Change ${subject.changeId}`;
    case "change_membership_claim":
      return `Change membership claim ${subject.membershipClaimId}`;
    case "change_link_claim":
      return `Change link claim ${subject.linkClaimId}`;
    case "change_revision_relation_claim":
      return `Change Revision relation claim ${subject.relationClaimId}`;
    case "revision_relation_attestation":
      return `Revision relation attestation ${subject.relationAttestationId} for ${revision(subject.revision)}`;
    case "review_fact_port":
      return `Fact port ${subject.portId} from ${fact(subject.originFact)} on ${revision(subject.originRevision)}`;
  }
}

export function eventAttributionLines(entry: EventHistoryEntry): string[] {
  const lines = [
    `${entry.writer.actorId} · ${entry.writer.producer.name} ${entry.writer.producer.version}`,
    `${words(entry.assertionMode)} assertion`,
  ];
  if (entry.signer) lines.push(`signed by ${entry.signer}`);
  if (entry.sourceRef) {
    lines.push(
      `source ${entry.sourceRef.sourceSystem} · ${entry.sourceRef.sourceId}`,
    );
  }
  if (entry.ingest) {
    lines.push(
      `ingested via ${entry.ingest.via} at ${entry.ingest.receivedAt}`,
    );
  }
  return lines;
}

export function eventTypeColor(eventType: EventHistoryEventType): string {
  switch (eventType) {
    case "review_initialized":
    case "review_note_imported":
      return "var(--evt-init)";
    case "work_object_proposed":
    case "change_declared":
    case "change_membership_asserted":
      return "var(--evt-capture)";
    case "review_observation_recorded":
    case "revision_ref_associated":
    case "revision_commit_associated":
    case "review_fact_ported":
      return "var(--evt-observation)";
    case "review_assessment_recorded":
    case "revision_relation_attested":
      return "var(--evt-assessment)";
    case "input_request_opened":
    case "revision_ref_withdrawn":
    case "revision_commit_withdrawn":
    case "change_membership_withdrawn":
    case "change_revision_relation_withdrawn":
      return "var(--evt-request)";
    case "input_request_responded":
    case "change_link_asserted":
    case "change_revision_relation_asserted":
      return "var(--evt-response)";
    case "validation_check_recorded":
      return "var(--evt-validation)";
  }
}

function field(label: string, value: string | undefined | null) {
  return value ? { label, value } : null;
}

function fields(
  ...values: Array<EventPresentationField | null>
): EventPresentationField[] {
  return values.filter(
    (value): value is EventPresentationField => value !== null,
  );
}

/** Every admitted event kind has one explicit primary reading. */
export function presentEvent(entry: EventHistoryEntry): EventPresentation {
  const summary = entry.summary;
  switch (summary.kind) {
    case "review_initialized":
      return {
        label: "review initialized",
        title: "Review initialized",
        body: "The review journal was initialized.",
        fields: [],
      };
    case "work_object_proposed": {
      const detail = summary.details;
      return {
        label: "Revision proposed",
        title: detail.summary || `Revision ${detail.revision.id} proposed`,
        body: `Captured ${detail.revision.objectId} as an exact Revision artifact.`,
        fields: fields(
          field("Revision", detail.revision.id),
          field("object", detail.revision.objectId),
          field("artifact", detail.objectArtifactContentHash),
          field("engagement", detail.engagementId),
          field(
            "supersedes",
            detail.supersedes.length ? detail.supersedes.join("; ") : "none",
          ),
        ),
      };
    }
    case "review_observation_recorded": {
      const detail = summary.details;
      return {
        label: "observation",
        title: detail.title,
        body: detail.body,
        fields: fields(
          field("observation", detail.observationId),
          field("target", eventTargetLabel(detail.target)),
          field("confidence", detail.confidence),
          field("tags", detail.tags?.join(", ")),
          field("supersedes", detail.supersedesObservationIds?.join("; ")),
          field("responds to", detail.respondsToObservationIds?.join("; ")),
        ),
      };
    }
    case "review_assessment_recorded": {
      const detail = summary.details;
      return {
        label: "assessment",
        title: `Assessment: ${words(detail.assessment)}`,
        body: detail.summary,
        fields: fields(
          field("assessment", detail.assessmentId),
          field("target", eventTargetLabel(detail.target)),
          field("replaces", detail.replacesAssessmentIds?.join("; ")),
          field("observations", detail.relatedObservationIds?.join("; ")),
          field("input requests", detail.relatedInputRequestIds?.join("; ")),
        ),
      };
    }
    case "input_request_opened": {
      const detail = summary.details;
      return {
        label: "input requested",
        title: detail.title,
        body: detail.body,
        fields: fields(
          field("input request", detail.inputRequestId),
          field("reason", words(detail.reasonCode)),
          field("target", eventTargetLabel(detail.target)),
        ),
      };
    }
    case "input_request_responded": {
      const detail = summary.details;
      return {
        label: "input response",
        title: `Input request ${words(detail.outcome)}`,
        body: detail.reason,
        fields: fields(
          field("response", detail.inputRequestResponseId),
          field("input request", detail.inputRequestId),
          field("Revision", detail.revisionId),
        ),
      };
    }
    case "review_note_imported":
      return {
        label: "imported note",
        title: "Legacy review note imported",
        body: "A note from the retired import path remains in this journal.",
        fields: [],
      };
    case "revision_ref_associated": {
      const detail = summary.details;
      return {
        label: "Git ref associated",
        title: `Associated ${detail.refName}`,
        fields: fields(
          field("association", detail.refAssociationId),
          field("target", eventTargetLabel(detail.target)),
          field("head", detail.headOid),
        ),
      };
    }
    case "revision_ref_withdrawn": {
      const detail = summary.details;
      return {
        label: "Git ref withdrawn",
        title: "Withdrew a Git ref association",
        fields: fields(
          field("withdrawal", detail.refWithdrawalId),
          field("association", detail.refAssociationId),
          field("target", eventTargetLabel(detail.target)),
        ),
      };
    }
    case "revision_commit_associated": {
      const detail = summary.details;
      const endpoint =
        detail.commit.kind === "git_commit"
          ? `commit ${detail.commit.commitOid} · tree ${detail.commit.treeOid}`
          : detail.commit.kind === "git_working_tree"
            ? `working tree ${detail.commit.worktreeRoot}`
            : `${words(detail.commit.kind)} ${detail.commit.treeOid}`;
      return {
        label: "commit associated",
        title: `Associated ${endpoint}`,
        fields: fields(
          field("association", detail.commitAssociationId),
          field("target", eventTargetLabel(detail.target)),
          field("endpoint", endpoint),
        ),
      };
    }
    case "revision_commit_withdrawn": {
      const detail = summary.details;
      return {
        label: "commit withdrawn",
        title: "Withdrew a commit association",
        fields: fields(
          field("withdrawal", detail.commitWithdrawalId),
          field("association", detail.commitAssociationId),
          field("target", eventTargetLabel(detail.target)),
        ),
      };
    }
    case "validation_check_recorded": {
      const detail = summary.details;
      return {
        label: "validation",
        title: `${detail.checkName}: ${words(detail.status)}`,
        body: detail.summary,
        fields: fields(
          field("validation", detail.validationCheckId),
          field("target", eventTargetLabel(detail.target)),
          field("command", detail.command),
          field("trigger", words(detail.trigger)),
          field(
            "exit code",
            detail.exitCode === undefined ? undefined : String(detail.exitCode),
          ),
        ),
      };
    }
    case "change_declared": {
      const detail = summary.details;
      const root =
        detail.identityDescriptor.kind === "root_revision"
          ? detail.identityDescriptor.revision_id
          : `opaque nonce ${detail.identityDescriptor.nonce}`;
      return {
        label: "Change declared",
        title: `Declared ${detail.changeId}`,
        body: `The stable Change identity is rooted in ${root}.`,
        fields: fields(
          field("declaration", detail.declarationClaimId),
          field("identity", root),
        ),
      };
    }
    case "change_membership_asserted": {
      const detail = summary.details;
      return {
        label: "Change membership",
        title: `Added ${detail.revisionId} to a Change`,
        fields: fields(
          field("Change", detail.changeId),
          field("Revision", detail.revisionId),
          field("claim", detail.membershipClaimId),
        ),
      };
    }
    case "change_membership_withdrawn": {
      const detail = summary.details;
      return {
        label: "membership withdrawn",
        title: "Withdrew a Change membership claim",
        fields: fields(
          field("claim", detail.membershipClaimId),
          field("withdrawal", detail.membershipWithdrawalId),
        ),
      };
    }
    case "change_link_asserted": {
      const detail = summary.details;
      return {
        label: "Change link",
        title: `Linked Changes as ${words(detail.relation)}`,
        fields: fields(
          field("left Change", detail.leftChangeId),
          field("right Change", detail.rightChangeId),
          field("claim", detail.linkClaimId),
        ),
      };
    }
    case "change_revision_relation_asserted": {
      const detail = summary.details;
      return {
        label: "Revision relation",
        title: "Asserted Revision supersession",
        body: `${detail.successor.revisionId} supersedes ${detail.predecessor.revisionId}.`,
        fields: fields(
          field("Change", detail.changeId),
          field("successor", revision(detail.successor)),
          field("predecessor", revision(detail.predecessor)),
          field("claim", detail.relationClaimId),
        ),
      };
    }
    case "change_revision_relation_withdrawn": {
      const detail = summary.details;
      return {
        label: "relation withdrawn",
        title: "Withdrew a Revision-relation claim",
        fields: fields(
          field("claim", detail.relationClaimId),
          field("withdrawal", detail.relationWithdrawalId),
        ),
      };
    }
    case "revision_relation_attested": {
      const detail = summary.details;
      return {
        label: "relation attested",
        title: `${words(detail.semanticRelation)}: ${words(detail.proofStatus)}`,
        body: `Proof method ${detail.proofMethod} (${detail.proofAlgorithmVersion}).`,
        fields: fields(
          field("Revision", revision(detail.revision)),
          field("commit association", detail.commitAssociationId),
          field("attestation", detail.relationAttestationId),
          field(
            "capture scope",
            detail.captureScope.join(", ") || "whole capture",
          ),
          field("comparison base", detail.comparisonBaseOrParent),
          field("endpoints", detail.endpointOids.join("; ")),
          field("evidence", detail.evidenceContentHash),
          field("result", detail.resultDigest),
        ),
      };
    }
    case "review_fact_ported": {
      const detail = summary.details;
      return {
        label: "fact ported",
        title: `${words(detail.relation)} ${fact(detail.originFact)}`,
        body: `Ported review context from ${detail.originRevision.revisionId} to ${detail.targetRevision.revisionId}.`,
        fields: fields(
          field("port", detail.portId),
          field("origin Revision", revision(detail.originRevision)),
          field("origin fact", fact(detail.originFact)),
          field("target Revision", revision(detail.targetRevision)),
          field("target fact", detail.targetFact && fact(detail.targetFact)),
          field("Change context", detail.contextChangeId),
          field("rationale", detail.rationaleContentHash),
        ),
      };
    }
  }
}
