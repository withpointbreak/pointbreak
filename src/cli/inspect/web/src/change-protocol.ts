/**
 * The Change-reader wire contract. The browser only constructs bounded requests
 * and validates server-owned projections; it never derives Change semantics.
 */

import changeReaderProfile from "../../../../documents/change_reader_profile_v1.json";

/**
 * Authority state for the reader's target store cohort. These states describe
 * whether Change semantics may be read at all; they do not describe projection
 * freshness or the bytes of an exact Revision resource.
 */
export type ReaderProfileAvailability =
  | "migration_required"
  | "migration_in_progress"
  | "ready";

/**
 * Complete identity of one authoritative store generation. Journal records
 * include capability records as well as review events, so neither record count
 * nor either set hash may be inferred from the event-only fields.
 */
export interface AuthorityCursorV2 {
  schema: "pointbreak.authority-cursor.v2";
  journalRecordCount: number;
  eventCount: number;
  journalRecordSetHash: string;
  eventSetHash: string;
  capabilitySetHash: string;
}

export type ChangeLens = "changes" | "attention";

/**
 * Query for the Change-aware event Timeline. This is intentionally separate
 * from the Change-card page query: Timeline chronology permits `asc`/`desc`,
 * while card pages retain their one `change_id_asc` order.
 */
export interface EventHistoryQuery {
  limit?: number;
  after?: string;
  at?: string;
  q?: string;
  type?: string;
  track?: string;
  change?: string;
  revision?: string;
  artifactHash?: string;
  order?: "asc" | "desc";
}

export interface EventHistoryRevisionRef {
  revisionId: string;
  objectArtifactContentHash: string;
}

/** The review-domain event kinds admitted by `/api/v2/history`. */
export const EVENT_HISTORY_EVENT_TYPES = [
  "review_initialized",
  "work_object_proposed",
  "review_observation_recorded",
  "review_assessment_recorded",
  "input_request_opened",
  "input_request_responded",
  "review_note_imported",
  "revision_ref_associated",
  "revision_ref_withdrawn",
  "revision_commit_associated",
  "revision_commit_withdrawn",
  "validation_check_recorded",
  "change_declared",
  "change_membership_asserted",
  "change_membership_withdrawn",
  "change_link_asserted",
  "change_revision_relation_asserted",
  "change_revision_relation_withdrawn",
  "revision_relation_attested",
  "review_fact_ported",
] as const;

export type EventHistoryEventType = (typeof EVENT_HISTORY_EVENT_TYPES)[number];

export interface EventHistoryWriter {
  actorId: string;
  producer: {
    name: string;
    version: string;
  };
}

export type EventHistoryAssertionMode = "advisory" | "operative";

export interface EventHistorySourceRef {
  sourceSystem: string;
  sourceId: string;
}

export interface EventHistoryIngest {
  via: "ingest-events" | "bundle-apply";
  receivedAt: string;
}

export type EventHistorySubject =
  | { kind: "journal"; journalId: string }
  | { kind: "review"; target: FactTarget }
  | { kind: "change"; changeId: string }
  | { kind: "change_membership_claim"; membershipClaimId: string }
  | { kind: "change_link_claim"; linkClaimId: string }
  | {
      kind: "change_revision_relation_claim";
      relationClaimId: string;
    }
  | {
      kind: "revision_relation_attestation";
      relationAttestationId: string;
      revision: EventHistoryRevisionRef;
    }
  | {
      kind: "review_fact_port";
      portId: string;
      originRevision: EventHistoryRevisionRef;
      originFact: FactRef;
    };

interface WorkObjectProposedSummary {
  engagementId: string;
  revision: {
    id: string;
    objectId: string;
    gitProvenance?: unknown;
  };
  summary: string | null;
  objectArtifactContentHash: string;
  supersedes: string[];
}

interface ObservationSummary {
  observationId: string;
  target: FactTarget;
  title: string;
  body?: string;
  tags?: string[];
  confidence?: string;
  supersedesObservationIds?: string[];
  respondsToObservationIds?: string[];
}

interface AssessmentSummary {
  assessmentId: string;
  target: FactTarget;
  assessment:
    | "accepted"
    | "accepted_with_follow_up"
    | "needs_changes"
    | "needs_clarification";
  summary?: string;
  replacesAssessmentIds?: string[];
  relatedObservationIds?: string[];
  relatedInputRequestIds?: string[];
}

interface InputRequestOpenedSummary {
  inputRequestId: string;
  target: FactTarget;
  reasonCode:
    | "ambiguous_state"
    | "unsafe_action"
    | "stale_revision"
    | "failed_gate"
    | "external_side_effect"
    | "conflicting_event"
    | "missing_permission"
    | "manual_decision_required"
    | "insufficient_evidence";
  title: string;
  body?: string;
}

interface InputRequestRespondedSummary {
  inputRequestResponseId: string;
  inputRequestId: string;
  revisionId: string;
  outcome: "approved" | "rejected" | "dismissed" | "superseded" | "abandoned";
  reason?: string;
}

interface RevisionRefAssociatedSummary {
  refAssociationId: string;
  target: FactTarget;
  refName: string;
  headOid: string;
}

interface RevisionRefWithdrawnSummary {
  refWithdrawalId: string;
  target: FactTarget;
  refAssociationId: string;
}

interface RevisionCommitAssociatedSummary {
  commitAssociationId: string;
  target: FactTarget;
  commit:
    | { kind: "git_commit"; commitOid: string; treeOid: string }
    | { kind: "git_tree"; treeOid: string }
    | { kind: "git_index"; treeOid: string }
    | { kind: "git_working_tree"; worktreeRoot: string };
}

interface RevisionCommitWithdrawnSummary {
  commitWithdrawalId: string;
  target: FactTarget;
  commitAssociationId: string;
}

interface ValidationCheckSummary {
  validationCheckId: string;
  target: { kind: "revision"; revisionId: string };
  checkName: string;
  command?: string;
  status: "passed" | "failed" | "errored" | "skipped";
  exitCode?: number;
  trigger: "manual" | "push" | "pull_request";
  summary?: string;
}

interface ChangeDeclaredSummary {
  schema: "pointbreak.change-declared";
  version: 1;
  declarationClaimId: string;
  changeId: string;
  identityDescriptor:
    | {
        kind: "opaque_nonce";
        schema: "pointbreak.change-identity.v1";
        nonce: string;
      }
    | {
        kind: "root_revision";
        schema: "pointbreak.change-identity.v1";
        revision_id: string;
      };
  claimNonce: string;
}

interface ChangeMembershipAssertedSummary {
  schema: "pointbreak.change-membership-asserted";
  version: 1;
  membershipClaimId: string;
  changeId: string;
  revisionId: string;
  claimNonce: string;
}

interface ChangeMembershipWithdrawnSummary {
  schema: "pointbreak.change-membership-withdrawn";
  version: 1;
  membershipWithdrawalId: string;
  membershipClaimId: string;
  claimNonce: string;
}

interface ChangeLinkAssertedSummary {
  schema: "pointbreak.change-link-asserted";
  version: 1;
  linkClaimId: string;
  leftChangeId: string;
  rightChangeId: string;
  relation: "same_work" | "related_work";
  claimNonce: string;
}

interface ChangeRevisionRelationAssertedSummary {
  schema: "pointbreak.change-revision-relation-asserted";
  version: 1;
  relationClaimId: string;
  changeId: string;
  successor: EventHistoryRevisionRef;
  predecessor: EventHistoryRevisionRef;
  relation: "supersedes";
  claimNonce: string;
}

interface ChangeRevisionRelationWithdrawnSummary {
  schema: "pointbreak.change-revision-relation-withdrawn";
  version: 1;
  relationWithdrawalId: string;
  relationClaimId: string;
  claimNonce: string;
}

interface RevisionRelationAttestedSummary {
  schema: "pointbreak.revision-relation-attested";
  version: 1;
  relationAttestationId: string;
  revision: EventHistoryRevisionRef;
  commitAssociationId: string;
  semanticRelation:
    | "exact_materialization"
    | "equivalent_rewrite"
    | "content_preserving_extension"
    | "landing_provenance"
    | "related_provenance"
    | "unknown";
  proofStatus:
    | "verified"
    | "asserted"
    | "unverified"
    | "indeterminate"
    | "refuted";
  proofMethod: string;
  proofAlgorithmVersion: string;
  captureScope: string[];
  comparisonBaseOrParent: string | null;
  endpointOids: string[];
  evidenceContentHash: string | null;
  resultDigest: string;
}

interface ReviewFactPortedSummary {
  schema: "pointbreak.review-fact-ported";
  version: 1;
  portId: string;
  originRevision: EventHistoryRevisionRef;
  originFact: FactRef;
  targetRevision: EventHistoryRevisionRef;
  relation:
    | "context_only"
    | "reanchored_as"
    | "carried_open_as"
    | "resolved_by";
  targetFact: FactRef | null;
  rationaleContentHash: string | null;
  contextChangeId: string | null;
}

export type EventHistorySummary =
  | { kind: "review_initialized" }
  | { kind: "work_object_proposed"; details: WorkObjectProposedSummary }
  | { kind: "review_observation_recorded"; details: ObservationSummary }
  | { kind: "review_assessment_recorded"; details: AssessmentSummary }
  | { kind: "input_request_opened"; details: InputRequestOpenedSummary }
  | { kind: "input_request_responded"; details: InputRequestRespondedSummary }
  | { kind: "review_note_imported" }
  | { kind: "revision_ref_associated"; details: RevisionRefAssociatedSummary }
  | { kind: "revision_ref_withdrawn"; details: RevisionRefWithdrawnSummary }
  | {
      kind: "revision_commit_associated";
      details: RevisionCommitAssociatedSummary;
    }
  | {
      kind: "revision_commit_withdrawn";
      details: RevisionCommitWithdrawnSummary;
    }
  | { kind: "validation_check_recorded"; details: ValidationCheckSummary }
  | { kind: "change_declared"; details: ChangeDeclaredSummary }
  | {
      kind: "change_membership_asserted";
      details: ChangeMembershipAssertedSummary;
    }
  | {
      kind: "change_membership_withdrawn";
      details: ChangeMembershipWithdrawnSummary;
    }
  | { kind: "change_link_asserted"; details: ChangeLinkAssertedSummary }
  | {
      kind: "change_revision_relation_asserted";
      details: ChangeRevisionRelationAssertedSummary;
    }
  | {
      kind: "change_revision_relation_withdrawn";
      details: ChangeRevisionRelationWithdrawnSummary;
    }
  | {
      kind: "revision_relation_attested";
      details: RevisionRelationAttestedSummary;
    }
  | { kind: "review_fact_ported"; details: ReviewFactPortedSummary };

export interface EventHistoryEntry {
  eventId: string;
  eventType: EventHistoryEventType;
  occurredAt: string;
  payloadHash: string;
  journalId: string;
  trackId?: string;
  writer: EventHistoryWriter;
  verificationStatus: "valid" | "invalid" | "untrusted_key" | "unsigned";
  assertionMode: EventHistoryAssertionMode;
  signer?: string;
  sourceRef?: EventHistorySourceRef;
  ingest?: EventHistoryIngest;
  subject: EventHistorySubject;
  changeIds: string[];
  revisionRefs: EventHistoryRevisionRef[];
  unresolvedRevisionIds: string[];
  summary: EventHistorySummary;
}

export interface EventHistoryDocument {
  schema: "pointbreak.inspect-event-history";
  version: 1;
  authorityCursor: AuthorityCursorV2;
  sourceChangeProjectionStamp: string;
  timelineProjectionStamp: string;
  order: "asc" | "desc";
  eventCount: number;
  matchCount: number;
  offset: number;
  matchIndex?: number;
  facets: Record<string, number>;
  completion: {
    eventTypes: EventHistoryEventType[];
    trackIds: string[];
    changeIds: string[];
    revisionRefs: EventHistoryRevisionRef[];
    unresolvedRevisionIds: string[];
  };
  diagnostics: string[];
  queryNotices: string[];
  entries: EventHistoryEntry[];
  previous?: string;
  next?: string;
}

export const CHANGE_PAGE_LIMIT = 50;
export const MAX_LIVE_CHANGE_ROWS = 150;

export const CHANGE_READER_PROFILE = changeReaderProfile.minimumReaderProfile;
export const CHANGE_READER_DOCUMENTS: Readonly<Record<string, number>> =
  changeReaderProfile.documents;

const TOPOLOGY_VALUES = new Set([
  "initial",
  "replacement",
  "replacement_divergent",
  "consolidation",
  "parallel_current",
  "mixed",
  "incomplete",
  "cycle_conflicted",
]);
const LIFECYCLE_VALUES = new Set([
  "incomplete",
  "conflicted",
  "in_progress",
  "accepted",
]);
const ATTENTION_VALUES = new Set([
  "clear",
  "in_progress",
  "incomplete",
  "conflicted",
]);
const AVAILABILITY_VALUES = new Set(["available", "incomplete"]);
const CONTENT_AVAILABILITY_VALUES = new Set([
  "available",
  "removed",
  "missing",
  "mismatch",
  "non_textual",
]);
const REVISION_CURRENCY_VALUES = new Set([
  "current",
  "stale_by_supersession",
  "membership_incomplete",
  "membership_conflicted",
]);
const FACT_FAMILY_STATE_VALUES = new Set([
  "current",
  "stale",
  "withdrawn",
  "conflicted",
  "unavailable",
]);
const ASSOCIATION_STATE_VALUES = new Set([
  "unknown",
  "exact",
  "equivalent",
  "extension",
  "unavailable",
]);
const ASSOCIATION_PROOF_VALUES = new Set([
  "available",
  "missing",
  "mismatch",
  "not_requested",
]);
const INTERDIFF_AVAILABILITY_VALUES = new Set([
  "available",
  "unavailable",
  "endpoint_missing",
  "endpoint_mismatch",
  "non_textual",
]);

export interface ReaderProfile {
  schema: "pointbreak.inspect-reader-profile";
  version: 1;
  availability: ReaderProfileAvailability;
  minimumReaderProfile?: string;
  authorityCursor: AuthorityCursorV2;
  commitGraphStamp?: string;
  documents: Record<string, number>;
}

export interface RevisionRef {
  revisionId: string;
  objectArtifactContentHash: string;
}

export type ChangeAttentionReason =
  | { kind: "conflicted" }
  | { kind: "incomplete" }
  | { kind: "no_current_revision" }
  | {
      kind: "unresolved_operative_requests";
      requestIds: string[];
    }
  | {
      kind: "current_revisions_need_assessment";
      revisions: RevisionRef[];
    };

export interface ChangeAttentionReasonPresentation {
  cause: ChangeAttentionReason;
  ask: string;
  reason: string;
  evidence: string;
  nextAction: string;
}

export interface ChangeAttentionPresentation {
  /** Server-ranked cause. It is always byte-for-byte equivalent to reasons[0]. */
  primaryReason: ChangeAttentionReason;
  /** Complete, deterministic, primary-first model causes. */
  reasons: ChangeAttentionReason[];
  /** Server-owned copy, exactly parallel to reasons. */
  reasonPresentations: ChangeAttentionReasonPresentation[];
  diagnostics?: string[];
}

export interface ChangePresentation {
  currentRevisions: Array<{
    revision: RevisionRef;
    revisionProposalSummary?: string;
    summarySource: "revision_proposal_summary" | "absent";
  }>;
  /** Inspector-only and present exclusively on the Attention lens. */
  attention?: ChangeAttentionPresentation;
}

export interface ChangeSummary {
  changeId: string;
  declarationState: "authoritative" | "incomplete" | "conflicted";
  titleAssertions: string[];
  memberCount: number;
  topology: string;
  lifecycle: string;
  attentionSummary: string;
  /**
   * Change-level membership/reference completeness. `available` means every
   * member has one exact Revision reference; its captured body may separately
   * be available, removed, missing, mismatched, or non-textual.
   */
  availabilitySummary: string;
  currentRevisionRefs: RevisionRef[];
  diagnostics?: string[];
  projectionStamp: string;
}

interface ChangePageBase {
  changes: ChangeSummary[];
  diagnostics?: string[];
  presentations?: Record<string, ChangePresentation>;
  projectionStamp: string;
  /** Opaque server capabilities. Optional members are absent on older bare responses. */
  previous?: string | null;
  next: string | null;
  last?: string | null;
}

export interface ChangesPage extends ChangePageBase {
  schema: "pointbreak.inspect-changes-page";
  version: 1;
}

export interface AttentionPage extends ChangePageBase {
  schema: "pointbreak.inspect-attention";
  version: 2;
}

export type ChangePage = ChangesPage | AttentionPage;

export interface ChangeDetail {
  schema: "pointbreak.review-change";
  version: 1;
  summary: ChangeSummary;
  memberRevisions: ChangeMemberRevision[];
  unavailableMemberRevisions: UnavailableChangeMemberRevision[];
  membershipClaims: ChangeMembershipClaim[];
  membershipWithdrawals: ChangeClaimWithdrawal[];
  relationClaims: ChangeRelationClaim[];
  relationWithdrawals: ChangeClaimWithdrawal[];
  links: ChangeLink[];
  effectiveSupersedes: Array<[RevisionRef, RevisionRef]>;
  pendingOrConflictingEdges: ChangeRelationClaim[];
  currentRevisionRefs: RevisionRef[];
  perCurrentRevisionQualification: RevisionQualification[];
  operativeObligations: string[];
  diagnostics: string[];
  projectionStamp: string;
  /**
   * Non-durable Inspector geometry. This private projection is deliberately
   * absent from the shared Change-reader document when no graph can be laid
   * out.
   */
  inspectorPresentation?: {
    revisionGraph: ChangeRevisionGraphPresentation;
  };
}

export type GraphPoint = [number, number];

export interface GraphBounds {
  w: number;
  h: number;
}

/** Whether this relationship node has an exact route in the enclosing Change. */
export type GraphContextAvailability =
  | "available"
  | "relationship_context_only";

/** Inspector-private layout of exact Revision supersession topology. */
export interface ChangeRevisionGraphPresentation {
  nodes: ChangeRevisionGraphNode[];
  effectiveSupersedes: ChangeRevisionGraphEffectiveEdge[];
  pendingOrConflictingClaims: ChangeRevisionGraphClaimEdge[];
  bounds: GraphBounds;
  diagnostics?: string[];
}

export interface ChangeRevisionGraphNode {
  id: string;
  revision: RevisionRef;
  x: number;
  y: number;
  w: number;
  h: number;
  isCurrent: boolean;
  isMember: boolean;
  contextAvailability: GraphContextAvailability;
  activationRevision?: RevisionRef;
}

export interface ChangeRevisionGraphEffectiveEdge {
  from: string;
  to: string;
  successor: RevisionRef;
  predecessor: RevisionRef;
  path: GraphPoint[];
}

export interface ChangeRevisionGraphClaimEdge
  extends ChangeRevisionGraphEffectiveEdge {
  claimId: string;
  diagnostics: string[];
}

export interface ChangeClaimSupport {
  eventId: string;
  actorId: string;
  trackId?: string;
}

export interface ChangeMemberRevision {
  revision: RevisionRef;
  supportingClaimIds: string[];
}

export interface UnavailableChangeMemberRevision {
  revisionId: string;
  reason: "invalid_revision_id" | "invalid_object_artifact_content_hash";
  supportingClaimIds: string[];
}

export interface ChangeMembershipClaim {
  claimId: string;
  changeId: string;
  revisionId: string;
  supports: ChangeClaimSupport[];
  withdrawals: ChangeClaimSupport[];
  active: boolean;
  diagnostics: string[];
}

export interface ChangeRelationClaim {
  claimId: string;
  changeId: string;
  successor: RevisionRef;
  predecessor: RevisionRef;
  supports: ChangeClaimSupport[];
  withdrawals: ChangeClaimSupport[];
  active: boolean;
  diagnostics: string[];
}

export interface ChangeClaimWithdrawal {
  claimId: string;
  supports: ChangeClaimSupport[];
  diagnostics: string[];
}

export interface ChangeLink {
  leftChangeId: string;
  rightChangeId: string;
  relation: string;
}

export interface RevisionQualification {
  revision: RevisionRef;
  qualified: boolean;
}

/** Immutable review target recorded with a fact; never inferred by the reader. */
export interface FactTarget {
  kind:
    | "revision"
    | "file"
    | "range"
    | "observation"
    | "input_request"
    | "assessment"
    | "event";
  revisionId: string;
  filePath?: string;
  side?: "old" | "new";
  startLine?: number;
  endLine?: number;
  observationId?: string;
  inputRequestId?: string;
  assessmentId?: string;
  eventId?: string;
}

export interface ChangeRevisionDetail {
  schema: "pointbreak.review-change-revision";
  version: 1;
  changeId: string;
  revision: RevisionRef;
  membershipSupport: ChangeMembershipClaim[];
  revisionCurrency: string;
  relationClassification: string;
  availability: string;
  exactRevisionDocument: RevisionResource;
  factPresentations: Array<{
    factId: string;
    family: string;
    originRevision: RevisionRef;
    target?: FactTarget;
    contextChangeId?: string;
    presentedInRevision?: RevisionRef;
    portRelation?:
      | "context_only"
      | "reanchored_as"
      | "carried_open_as"
      | "resolved_by";
    actorId: string;
    trackId?: string;
    revisionCurrency: string;
    familyState: string;
    availability: string;
  }>;
  factContentPresentations?: Record<
    string,
    {
      contentType: "text/plain" | "text/markdown";
      bodyContentState: "present" | "suppressed_present" | "physically_removed";
      content: FactContent;
    }
  >;
  factPorts: FactPortPresentation[];
  associations: AssociationComparison[];
  diagnostics: string[];
  projectionStamp: string;
  /**
   * Non-durable Inspector geometry. The exact Revision document remains the
   * source of fact and fact-port meaning; this only routes those identities.
   */
  inspectorPresentation?: {
    factGraph: FactRelationshipGraphPresentation;
  };
}

export interface RevisionResource {
  schema: "pointbreak.review-revision-resource";
  version: 1;
  resource: { revision: RevisionRef; objectId: string };
  projection: { includeBody: boolean; trackId?: string };
  availability: string;
  capturedDocumentHash?: string;
  /**
   * The served, bound view of the stored object artifact.  Decode it before
   * paint: a route-bound resource must not be able to carry another
   * Revision's snapshot merely because both envelopes look plausible.
   */
  capturedDocument?: CapturedReviewSnapshot;
  diagnostics: string[];
  projectionStamp: string;
  cacheKey: string;
}

/** The minimum identity-bearing shape of an authoritative captured snapshot. */
export interface CapturedReviewSnapshot {
  schema: "pointbreak.review-snapshot";
  version: 1;
  contentHash: string;
  snapshot: {
    review_id: string;
    object_id: string;
    files?: unknown[];
  };
}

export interface FactPortPresentation {
  portId: string;
  originRevision: RevisionRef;
  originFact: FactRef;
  targetRevision: RevisionRef;
  relation:
    | "context_only"
    | "reanchored_as"
    | "carried_open_as"
    | "resolved_by";
  targetFact?: FactRef;
  rationaleContentHash?: string;
  contextChangeId?: string;
  actorId: string;
  trackId?: string;
  sourceEventIds: string[];
  applicability: "applicable" | "conflicted" | "unavailable";
  diagnostics: string[];
}

export interface FactRef {
  kind: "observation" | "input_request";
  observationId?: string;
  inputRequestId?: string;
}

/** Inspector-private layout of exact fact relationships and explicit ports. */
export interface FactRelationshipGraphPresentation {
  nodes: FactRelationshipGraphNode[];
  observationSupersedes: FactRelationshipEdge[];
  assessmentReplaces: FactRelationshipEdge[];
  factPorts: FactPortRelationshipEdge[];
  bounds: GraphBounds;
}

export type FactRelationshipGraphNode =
  | {
      id: string;
      kind: "fact";
      revision: RevisionRef;
      factId: string;
      family: string;
      x: number;
      y: number;
      w: number;
      h: number;
      contextAvailability: GraphContextAvailability;
      activationRevision?: RevisionRef;
    }
  | {
      id: string;
      kind: "revision";
      revision: RevisionRef;
      x: number;
      y: number;
      w: number;
      h: number;
      contextAvailability: GraphContextAvailability;
      activationRevision?: RevisionRef;
    };

export interface FactRelationshipEdge {
  from: string;
  to: string;
  originRevision: RevisionRef;
  fromFactId: string;
  toFactId: string;
  path: GraphPoint[];
}

export interface FactPortRelationshipEdge {
  portId: string;
  from: string;
  to: string;
  originRevision: RevisionRef;
  originFact: FactRef;
  targetRevision: RevisionRef;
  targetFact?: FactRef;
  relation: FactPortPresentation["relation"];
  applicability: FactPortPresentation["applicability"];
  path: GraphPoint[];
  diagnostics?: string[];
}

export interface AssociationComparison {
  schema: "pointbreak.review-association-comparison";
  version: 1;
  state: string;
  proofAvailability: string;
  comparison: {
    revision: RevisionRef;
    associationId: string;
    commitOid: string;
    comparisonBase: string;
    viewKind: string;
    proofRef?: string;
  };
  diagnostics: string[];
  cacheKey: string;
}

export interface RevisionInterdiff {
  schema: "pointbreak.review-revision-interdiff";
  version: 1;
  interdiff: {
    from: RevisionRef;
    to: RevisionRef;
    algorithmVersion: string;
    scope: string[];
  };
  availability: string;
  comparison?: unknown;
  diagnostics: string[];
  projectionStamp: string;
  cacheKey: string;
}

export type FactContent =
  | { kind: "observation"; title: string; body?: string }
  | {
      kind: "input_request";
      title: string;
      body?: string;
      status: string;
      responses?: Array<{
        responseId: string;
        outcome: string;
        reason?: string;
        contentType: "text/plain" | "text/markdown";
        bodyContentState:
          | "present"
          | "suppressed_present"
          | "physically_removed";
        availability: string;
      }>;
    }
  | { kind: "assessment"; assessment: string; summary?: string }
  | {
      kind: "validation";
      checkName: string;
      command?: string;
      status: string;
      summary?: string;
    };

export function decodeChangeDetail(value: unknown): ChangeDetail {
  const detail = object(value, "Change detail");
  const summary = detail.summary;
  const stamp = detail.projectionStamp;
  const memberRevisions = detail.memberRevisions;
  const unavailableMemberRevisions = detail.unavailableMemberRevisions;
  const membershipClaims = detail.membershipClaims;
  const membershipWithdrawals = detail.membershipWithdrawals;
  const relationClaims = detail.relationClaims;
  const relationWithdrawals = detail.relationWithdrawals;
  const links = detail.links;
  const effectiveSupersedes = detail.effectiveSupersedes;
  const pendingOrConflictingEdges = detail.pendingOrConflictingEdges;
  const currentRevisionRefs = detail.currentRevisionRefs;
  const perCurrentRevisionQualification =
    detail.perCurrentRevisionQualification;
  const operativeObligations = detail.operativeObligations;
  const diagnostics = detail.diagnostics;
  const inspectorPresentation = detail.inspectorPresentation;
  if (
    detail.schema !== "pointbreak.review-change" ||
    detail.version !== 1 ||
    !nonEmptyString(stamp) ||
    !isChangeSummary(summary, stamp) ||
    !isChangeMemberRevisions(memberRevisions) ||
    !isUnavailableChangeMemberRevisions(unavailableMemberRevisions) ||
    !isMembershipClaims(membershipClaims, summary.changeId) ||
    !isClaimWithdrawals(membershipWithdrawals) ||
    !Array.isArray(relationClaims) ||
    !relationClaims.every((claim) =>
      isRelationClaim(claim, summary.changeId),
    ) ||
    !isClaimWithdrawals(relationWithdrawals) ||
    !isChangeLinks(links) ||
    !isEffectiveSupersedes(effectiveSupersedes) ||
    !Array.isArray(pendingOrConflictingEdges) ||
    !pendingOrConflictingEdges.every((claim) =>
      isRelationClaim(claim, summary.changeId),
    ) ||
    !Array.isArray(currentRevisionRefs) ||
    !currentRevisionRefs.every(isRevisionRef) ||
    !sameRevisionSet(currentRevisionRefs, summary.currentRevisionRefs) ||
    !isRevisionQualifications(
      perCurrentRevisionQualification,
      currentRevisionRefs,
    ) ||
    !isStringArray(operativeObligations) ||
    !isStringArray(diagnostics)
  ) {
    throw new Error("invalid Change detail DTO");
  }
  if (
    !isChangeDetailInspectorPresentation(inspectorPresentation, {
      memberRevisions,
      currentRevisionRefs,
      effectiveSupersedes,
      pendingOrConflictingEdges,
      diagnostics,
    })
  ) {
    throw new Error("invalid Change detail DTO");
  }
  return {
    schema: "pointbreak.review-change",
    version: 1,
    summary,
    memberRevisions,
    unavailableMemberRevisions,
    membershipClaims,
    membershipWithdrawals,
    relationClaims,
    relationWithdrawals,
    links,
    effectiveSupersedes,
    pendingOrConflictingEdges,
    currentRevisionRefs,
    perCurrentRevisionQualification,
    operativeObligations,
    diagnostics,
    projectionStamp: stamp,
    inspectorPresentation,
  };
}

export function decodeChangeRevisionDetail(
  value: unknown,
): ChangeRevisionDetail {
  const detail = object(value, "Change Revision detail");
  const revision = detail.revision;
  const factPresentations = detail.factPresentations;
  const factContentPresentations = detail.factContentPresentations;
  const exactRevisionDocument = detail.exactRevisionDocument;
  const membershipSupport = detail.membershipSupport;
  const factPorts = detail.factPorts;
  const associations = detail.associations;
  const diagnostics = detail.diagnostics;
  const revisionCurrency = detail.revisionCurrency;
  const relationClassification = detail.relationClassification;
  const availability = detail.availability;
  const inspectorPresentation = detail.inspectorPresentation;
  if (
    detail.schema !== "pointbreak.review-change-revision" ||
    detail.version !== 1 ||
    !nonEmptyString(detail.changeId) ||
    !isRevisionRef(revision) ||
    typeof revisionCurrency !== "string" ||
    !REVISION_CURRENCY_VALUES.has(revisionCurrency) ||
    (relationClassification !== "current" &&
      relationClassification !== "superseded") ||
    typeof availability !== "string" ||
    !CONTENT_AVAILABILITY_VALUES.has(availability) ||
    !isRevisionResource(exactRevisionDocument) ||
    !sameRevision(exactRevisionDocument.resource.revision, revision) ||
    availability !== exactRevisionDocument.availability ||
    !isMembershipClaims(membershipSupport, detail.changeId) ||
    !Array.isArray(factPresentations) ||
    !factPresentations.every(isFactPresentation) ||
    !uniqueFactPresentationIds(factPresentations) ||
    (factContentPresentations !== undefined &&
      !isFactContentPresentations(factContentPresentations)) ||
    (factContentPresentations !== undefined &&
      !sameFactIds(factPresentations, factContentPresentations)) ||
    !isFactPortPresentations(
      factPorts,
      detail.changeId,
      factPresentations,
      revision,
    ) ||
    !Array.isArray(associations) ||
    !associations.every(isAssociation) ||
    !isStringArray(diagnostics) ||
    !nonEmptyString(detail.projectionStamp)
  ) {
    throw new Error("invalid Change Revision detail DTO");
  }
  if (
    !isChangeRevisionDetailInspectorPresentation(inspectorPresentation, {
      revision,
      factPresentations,
      factPorts,
    })
  ) {
    throw new Error("invalid Change Revision detail DTO");
  }
  return {
    schema: "pointbreak.review-change-revision",
    version: 1,
    changeId: detail.changeId,
    revision,
    membershipSupport,
    revisionCurrency,
    relationClassification,
    availability,
    exactRevisionDocument,
    factPresentations,
    factContentPresentations,
    factPorts,
    associations,
    diagnostics,
    projectionStamp: detail.projectionStamp,
    inspectorPresentation,
  };
}

export function decodeRevisionResource(value: unknown): RevisionResource {
  const document = object(value, "Revision resource");
  const resource = document.resource;
  const projection = document.projection;
  const diagnostics = document.diagnostics;
  const availability = document.availability;
  const capturedDocumentHash = document.capturedDocumentHash;
  const projectionStamp = document.projectionStamp;
  const cacheKey = document.cacheKey;
  if (
    document.schema !== "pointbreak.review-revision-resource" ||
    document.version !== 1 ||
    !isRecord(resource) ||
    !isRevisionRef(resource.revision) ||
    !nonEmptyString(resource.objectId) ||
    !isResourceProjection(projection) ||
    !isOneOf(availability, CONTENT_AVAILABILITY_VALUES) ||
    (capturedDocumentHash !== undefined &&
      !nonEmptyString(capturedDocumentHash)) ||
    (availability === "available" &&
      (capturedDocumentHash === undefined ||
        !isCapturedReviewSnapshot(
          document.capturedDocument,
          resource.revision.objectArtifactContentHash,
          resource.objectId,
        ))) ||
    (availability !== "available" &&
      (capturedDocumentHash !== undefined ||
        document.capturedDocument !== undefined)) ||
    !nonEmptyString(projectionStamp) ||
    !nonEmptyString(cacheKey) ||
    !isStringArray(diagnostics)
  ) {
    throw new Error("invalid Revision resource DTO");
  }
  return {
    schema: "pointbreak.review-revision-resource",
    version: 1,
    resource: { revision: resource.revision, objectId: resource.objectId },
    projection,
    availability,
    capturedDocumentHash,
    capturedDocument: document.capturedDocument as
      | CapturedReviewSnapshot
      | undefined,
    diagnostics,
    projectionStamp,
    cacheKey,
  };
}

export function decodeRevisionInterdiff(value: unknown): RevisionInterdiff {
  const document = object(value, "Revision interdiff");
  const interdiff = document.interdiff;
  const diagnostics = document.diagnostics;
  const availability = document.availability;
  const projectionStamp = document.projectionStamp;
  const cacheKey = document.cacheKey;
  if (
    document.schema !== "pointbreak.review-revision-interdiff" ||
    document.version !== 1 ||
    !isRecord(interdiff) ||
    !isRevisionRef(interdiff.from) ||
    !isRevisionRef(interdiff.to) ||
    !nonEmptyString(interdiff.algorithmVersion) ||
    !isStringArray(interdiff.scope) ||
    !isOneOf(availability, INTERDIFF_AVAILABILITY_VALUES) ||
    !isStringArray(diagnostics) ||
    !nonEmptyString(projectionStamp) ||
    !nonEmptyString(cacheKey) ||
    (availability === "available") !== (document.comparison !== undefined)
  ) {
    throw new Error("invalid Revision interdiff DTO");
  }
  return {
    schema: "pointbreak.review-revision-interdiff",
    version: 1,
    interdiff: {
      from: interdiff.from,
      to: interdiff.to,
      algorithmVersion: interdiff.algorithmVersion,
      scope: interdiff.scope,
    },
    availability,
    comparison: document.comparison,
    diagnostics,
    projectionStamp,
    cacheKey,
  };
}

export interface ChangePageQuery {
  limit?: number;
  after?: string;
  q?: string;
  topology?: string;
  lifecycle?: string;
  attention?: string;
  availability?: string;
  order?: "change_id_asc";
}

const MAX_INSPECTOR_QUERY_BYTES = 256;

function normalizeBoundedQueryText(value: string, label: string): string {
  const normalized = trimUnicodeWhitespace(value).toLowerCase();
  if (
    !normalized ||
    new TextEncoder().encode(normalized).length > MAX_INSPECTOR_QUERY_BYTES
  ) {
    throw new Error(
      `${label} query must be non-empty and at most ${MAX_INSPECTOR_QUERY_BYTES} bytes`,
    );
  }
  return normalized;
}

/** Normalize and bound the plain Change-page search before route transport. */
export function normalizeChangePageQueryText(value: string): string {
  return normalizeBoundedQueryText(value, "Change page");
}

/** Normalize and bound the structured Timeline search before route transport. */
export function normalizeEventHistoryQueryText(value: string): string {
  return normalizeBoundedQueryText(value, "Timeline");
}

export function buildChangePageUrl(
  lens: ChangeLens,
  query: ChangePageQuery = {},
): string {
  const limit = query.limit ?? CHANGE_PAGE_LIMIT;
  if (!Number.isInteger(limit) || limit < 1 || limit > 100) {
    throw new Error("Change page limit must be an integer from 1 through 100");
  }
  const params = new URLSearchParams({ limit: String(limit) });
  if (query.after !== undefined) {
    if (!query.after || new TextEncoder().encode(query.after).length > 4096) {
      throw new Error(
        "Change page continuation must be a non-empty opaque token",
      );
    }
    params.set("after", query.after);
  }
  if (query.q !== undefined) {
    params.set("q", normalizeChangePageQueryText(query.q));
  }
  appendEnum(params, "topology", query.topology, TOPOLOGY_VALUES);
  appendEnum(params, "lifecycle", query.lifecycle, LIFECYCLE_VALUES);
  appendEnum(params, "attention", query.attention, ATTENTION_VALUES);
  appendEnum(params, "availability", query.availability, AVAILABILITY_VALUES);
  if (query.order !== undefined && query.order !== "change_id_asc") {
    throw new Error("Change page order must be change_id_asc");
  }
  params.set("order", "change_id_asc");
  return `/api/v2/${lens}?${params}`;
}

/** Construct the strict bounded Timeline request without borrowing legacy URLs. */
export function buildEventHistoryUrl(query: EventHistoryQuery = {}): string {
  const limit = query.limit ?? 100;
  if (!Number.isInteger(limit) || limit < 1 || limit > 100) {
    throw new Error("Timeline limit must be an integer from 1 through 100");
  }
  if (query.after !== undefined && query.at !== undefined) {
    throw new Error("Timeline at and after are mutually exclusive");
  }
  if (
    (query.revision === undefined) !== (query.artifactHash === undefined) ||
    (query.revision !== undefined && (!query.revision || !query.artifactHash))
  ) {
    throw new Error("Timeline revision requires an exact artifact hash");
  }
  const eventTypes = query.type?.split(",");
  if (eventTypes?.some((eventType) => !isEventHistoryEventType(eventType))) {
    throw new Error("Timeline type contains an unknown event type");
  }
  if (eventTypes && new Set(eventTypes).size !== eventTypes.length) {
    throw new Error("Timeline type contains a duplicate event type");
  }
  const canonicalTypes = eventTypes?.sort().join(",");
  const params = new URLSearchParams({ limit: String(limit) });
  if (query.q !== undefined) {
    params.set("q", normalizeEventHistoryQueryText(query.q));
  }
  const textFields = [
    "after",
    "at",
    "track",
    "change",
    "revision",
    "artifactHash",
  ] as const;
  for (const field of textFields) {
    const value = query[field];
    if (value === undefined) continue;
    if (!value) throw new Error(`Timeline ${field} must be non-empty`);
    params.set(field, value);
  }
  if (canonicalTypes) params.set("type", canonicalTypes);
  if (
    query.order !== undefined &&
    query.order !== "asc" &&
    query.order !== "desc"
  ) {
    throw new Error("Timeline order must be asc or desc");
  }
  params.set("order", query.order ?? "desc");
  return `/api/v2/history?${params}`;
}

function isEventHistoryRevisionRef(
  value: unknown,
): value is EventHistoryRevisionRef {
  return (
    isRecord(value) &&
    nonEmptyString(value.revisionId) &&
    nonEmptyString(value.objectArtifactContentHash)
  );
}

const EVENT_HISTORY_EVENT_TYPE_VALUES = new Set<string>(
  EVENT_HISTORY_EVENT_TYPES,
);

function isEventHistoryEventType(
  value: unknown,
): value is EventHistoryEventType {
  return (
    typeof value === "string" && EVENT_HISTORY_EVENT_TYPE_VALUES.has(value)
  );
}

function isEventHistoryWriter(value: unknown): value is EventHistoryWriter {
  return (
    isRecord(value) &&
    nonEmptyString(value.actorId) &&
    isRecord(value.producer) &&
    nonEmptyString(value.producer.name) &&
    nonEmptyString(value.producer.version)
  );
}

function isReviewEndpoint(
  value: unknown,
): value is RevisionCommitAssociatedSummary["commit"] {
  if (!isRecord(value)) return false;
  switch (value.kind) {
    case "git_commit":
      return nonEmptyString(value.commitOid) && nonEmptyString(value.treeOid);
    case "git_tree":
    case "git_index":
      return nonEmptyString(value.treeOid);
    case "git_working_tree":
      return nonEmptyString(value.worktreeRoot);
    default:
      return false;
  }
}

function isEventHistorySubject(value: unknown): value is EventHistorySubject {
  if (!isRecord(value)) return false;
  switch (value.kind) {
    case "journal":
      return nonEmptyString(value.journalId);
    case "review":
      return isFactTarget(value.target);
    case "change":
      return nonEmptyString(value.changeId);
    case "change_membership_claim":
      return nonEmptyString(value.membershipClaimId);
    case "change_link_claim":
      return nonEmptyString(value.linkClaimId);
    case "change_revision_relation_claim":
      return nonEmptyString(value.relationClaimId);
    case "revision_relation_attestation":
      return (
        nonEmptyString(value.relationAttestationId) &&
        isEventHistoryRevisionRef(value.revision)
      );
    case "review_fact_port":
      return (
        nonEmptyString(value.portId) &&
        isEventHistoryRevisionRef(value.originRevision) &&
        isFactRef(value.originFact)
      );
    default:
      return false;
  }
}

function isOptionalStringArray(value: unknown): boolean {
  return value === undefined || isStringArray(value);
}

function isNullableString(value: unknown): boolean {
  return value === null || typeof value === "string";
}

function isReviewTargetSummary(value: unknown): value is FactTarget {
  return isFactTarget(value);
}

function isEventHistorySummary(
  value: unknown,
  eventType: EventHistoryEventType,
): value is EventHistorySummary {
  if (!isRecord(value) || value.kind !== eventType) return false;
  if (
    eventType === "review_initialized" ||
    eventType === "review_note_imported"
  ) {
    return value.details === undefined;
  }
  const details = value.details;
  if (!isRecord(details)) return false;
  switch (eventType) {
    case "work_object_proposed":
      return (
        nonEmptyString(details.engagementId) &&
        isRecord(details.revision) &&
        nonEmptyString(details.revision.id) &&
        nonEmptyString(details.revision.objectId) &&
        isNullableString(details.summary) &&
        nonEmptyString(details.objectArtifactContentHash) &&
        isStringArray(details.supersedes)
      );
    case "review_observation_recorded":
      return (
        nonEmptyString(details.observationId) &&
        isReviewTargetSummary(details.target) &&
        nonEmptyString(details.title) &&
        optionalString(details.body) &&
        isOptionalStringArray(details.tags) &&
        optionalString(details.confidence) &&
        isOptionalStringArray(details.supersedesObservationIds) &&
        isOptionalStringArray(details.respondsToObservationIds)
      );
    case "review_assessment_recorded":
      return (
        nonEmptyString(details.assessmentId) &&
        isReviewTargetSummary(details.target) &&
        (details.assessment === "accepted" ||
          details.assessment === "accepted_with_follow_up" ||
          details.assessment === "needs_changes" ||
          details.assessment === "needs_clarification") &&
        optionalString(details.summary) &&
        isOptionalStringArray(details.replacesAssessmentIds) &&
        isOptionalStringArray(details.relatedObservationIds) &&
        isOptionalStringArray(details.relatedInputRequestIds)
      );
    case "input_request_opened":
      return (
        nonEmptyString(details.inputRequestId) &&
        isReviewTargetSummary(details.target) &&
        (details.reasonCode === "ambiguous_state" ||
          details.reasonCode === "unsafe_action" ||
          details.reasonCode === "stale_revision" ||
          details.reasonCode === "failed_gate" ||
          details.reasonCode === "external_side_effect" ||
          details.reasonCode === "conflicting_event" ||
          details.reasonCode === "missing_permission" ||
          details.reasonCode === "manual_decision_required" ||
          details.reasonCode === "insufficient_evidence") &&
        nonEmptyString(details.title) &&
        optionalString(details.body)
      );
    case "input_request_responded":
      return (
        nonEmptyString(details.inputRequestResponseId) &&
        nonEmptyString(details.inputRequestId) &&
        nonEmptyString(details.revisionId) &&
        (details.outcome === "approved" ||
          details.outcome === "rejected" ||
          details.outcome === "dismissed" ||
          details.outcome === "superseded" ||
          details.outcome === "abandoned") &&
        optionalString(details.reason)
      );
    case "revision_ref_associated":
      return (
        nonEmptyString(details.refAssociationId) &&
        isReviewTargetSummary(details.target) &&
        nonEmptyString(details.refName) &&
        nonEmptyString(details.headOid)
      );
    case "revision_ref_withdrawn":
      return (
        nonEmptyString(details.refWithdrawalId) &&
        isReviewTargetSummary(details.target) &&
        nonEmptyString(details.refAssociationId)
      );
    case "revision_commit_associated":
      return (
        nonEmptyString(details.commitAssociationId) &&
        isReviewTargetSummary(details.target) &&
        isReviewEndpoint(details.commit)
      );
    case "revision_commit_withdrawn":
      return (
        nonEmptyString(details.commitWithdrawalId) &&
        isReviewTargetSummary(details.target) &&
        nonEmptyString(details.commitAssociationId)
      );
    case "validation_check_recorded":
      return (
        nonEmptyString(details.validationCheckId) &&
        isRecord(details.target) &&
        details.target.kind === "revision" &&
        nonEmptyString(details.target.revisionId) &&
        nonEmptyString(details.checkName) &&
        optionalString(details.command) &&
        (details.status === "passed" ||
          details.status === "failed" ||
          details.status === "errored" ||
          details.status === "skipped") &&
        (details.exitCode === undefined ||
          (typeof details.exitCode === "number" &&
            Number.isSafeInteger(details.exitCode))) &&
        (details.trigger === "manual" ||
          details.trigger === "push" ||
          details.trigger === "pull_request") &&
        optionalString(details.summary)
      );
    case "change_declared":
      return (
        details.schema === "pointbreak.change-declared" &&
        details.version === 1 &&
        nonEmptyString(details.declarationClaimId) &&
        nonEmptyString(details.changeId) &&
        isRecord(details.identityDescriptor) &&
        details.identityDescriptor.schema === "pointbreak.change-identity.v1" &&
        ((details.identityDescriptor.kind === "opaque_nonce" &&
          nonEmptyString(details.identityDescriptor.nonce)) ||
          (details.identityDescriptor.kind === "root_revision" &&
            nonEmptyString(details.identityDescriptor.revision_id))) &&
        nonEmptyString(details.claimNonce)
      );
    case "change_membership_asserted":
      return (
        details.schema === "pointbreak.change-membership-asserted" &&
        details.version === 1 &&
        nonEmptyString(details.membershipClaimId) &&
        nonEmptyString(details.changeId) &&
        nonEmptyString(details.revisionId) &&
        nonEmptyString(details.claimNonce)
      );
    case "change_membership_withdrawn":
      return (
        details.schema === "pointbreak.change-membership-withdrawn" &&
        details.version === 1 &&
        nonEmptyString(details.membershipWithdrawalId) &&
        nonEmptyString(details.membershipClaimId) &&
        nonEmptyString(details.claimNonce)
      );
    case "change_link_asserted":
      return (
        details.schema === "pointbreak.change-link-asserted" &&
        details.version === 1 &&
        nonEmptyString(details.linkClaimId) &&
        nonEmptyString(details.leftChangeId) &&
        nonEmptyString(details.rightChangeId) &&
        (details.relation === "same_work" ||
          details.relation === "related_work") &&
        nonEmptyString(details.claimNonce)
      );
    case "change_revision_relation_asserted":
      return (
        details.schema === "pointbreak.change-revision-relation-asserted" &&
        details.version === 1 &&
        nonEmptyString(details.relationClaimId) &&
        nonEmptyString(details.changeId) &&
        isEventHistoryRevisionRef(details.successor) &&
        isEventHistoryRevisionRef(details.predecessor) &&
        details.relation === "supersedes" &&
        nonEmptyString(details.claimNonce)
      );
    case "change_revision_relation_withdrawn":
      return (
        details.schema === "pointbreak.change-revision-relation-withdrawn" &&
        details.version === 1 &&
        nonEmptyString(details.relationWithdrawalId) &&
        nonEmptyString(details.relationClaimId) &&
        nonEmptyString(details.claimNonce)
      );
    case "revision_relation_attested":
      return (
        details.schema === "pointbreak.revision-relation-attested" &&
        details.version === 1 &&
        nonEmptyString(details.relationAttestationId) &&
        isEventHistoryRevisionRef(details.revision) &&
        nonEmptyString(details.commitAssociationId) &&
        (details.semanticRelation === "exact_materialization" ||
          details.semanticRelation === "equivalent_rewrite" ||
          details.semanticRelation === "content_preserving_extension" ||
          details.semanticRelation === "landing_provenance" ||
          details.semanticRelation === "related_provenance" ||
          details.semanticRelation === "unknown") &&
        (details.proofStatus === "verified" ||
          details.proofStatus === "asserted" ||
          details.proofStatus === "unverified" ||
          details.proofStatus === "indeterminate" ||
          details.proofStatus === "refuted") &&
        nonEmptyString(details.proofMethod) &&
        nonEmptyString(details.proofAlgorithmVersion) &&
        isStringArray(details.captureScope) &&
        isNullableString(details.comparisonBaseOrParent) &&
        isStringArray(details.endpointOids) &&
        isNullableString(details.evidenceContentHash) &&
        nonEmptyString(details.resultDigest)
      );
    case "review_fact_ported":
      return (
        details.schema === "pointbreak.review-fact-ported" &&
        details.version === 1 &&
        nonEmptyString(details.portId) &&
        isEventHistoryRevisionRef(details.originRevision) &&
        isFactRef(details.originFact) &&
        isEventHistoryRevisionRef(details.targetRevision) &&
        (details.relation === "context_only" ||
          details.relation === "reanchored_as" ||
          details.relation === "carried_open_as" ||
          details.relation === "resolved_by") &&
        (details.targetFact === null || isFactRef(details.targetFact)) &&
        isNullableString(details.rationaleContentHash) &&
        isNullableString(details.contextChangeId)
      );
  }
}

function isEventHistoryEntry(value: unknown): value is EventHistoryEntry {
  if (!isRecord(value) || !isEventHistoryEventType(value.eventType)) {
    return false;
  }
  return (
    nonEmptyString(value.eventId) &&
    nonEmptyString(value.occurredAt) &&
    nonEmptyString(value.payloadHash) &&
    nonEmptyString(value.journalId) &&
    optionalString(value.trackId) &&
    isEventHistoryWriter(value.writer) &&
    (value.verificationStatus === "valid" ||
      value.verificationStatus === "invalid" ||
      value.verificationStatus === "untrusted_key" ||
      value.verificationStatus === "unsigned") &&
    (value.assertionMode === "advisory" ||
      value.assertionMode === "operative") &&
    optionalString(value.signer) &&
    (value.sourceRef === undefined ||
      (isRecord(value.sourceRef) &&
        nonEmptyString(value.sourceRef.sourceSystem) &&
        nonEmptyString(value.sourceRef.sourceId))) &&
    (value.ingest === undefined ||
      (isRecord(value.ingest) &&
        (value.ingest.via === "ingest-events" ||
          value.ingest.via === "bundle-apply") &&
        nonEmptyString(value.ingest.receivedAt))) &&
    isEventHistorySubject(value.subject) &&
    isStringArray(value.changeIds) &&
    Array.isArray(value.revisionRefs) &&
    value.revisionRefs.every(isEventHistoryRevisionRef) &&
    isStringArray(value.unresolvedRevisionIds) &&
    isEventHistorySummary(value.summary, value.eventType)
  );
}

/** Validate the fully server-owned, paged Change-aware Timeline projection. */
export function decodeEventHistory(value: unknown): EventHistoryDocument {
  const document = object(value, "event history");
  const completion = document.completion;
  const authorityCursor = decodeAuthorityCursorV2(document.authorityCursor);
  if (
    document.schema !== "pointbreak.inspect-event-history" ||
    document.version !== 1 ||
    !nonEmptyString(document.sourceChangeProjectionStamp) ||
    !nonEmptyString(document.timelineProjectionStamp) ||
    (document.order !== "asc" && document.order !== "desc") ||
    !Number.isSafeInteger(document.eventCount) ||
    (document.eventCount as number) < 0 ||
    (document.eventCount as number) !== authorityCursor.eventCount ||
    !Number.isSafeInteger(document.matchCount) ||
    (document.matchCount as number) < 0 ||
    !Number.isSafeInteger(document.offset) ||
    (document.offset as number) < 0 ||
    (document.matchIndex !== undefined &&
      (!Number.isSafeInteger(document.matchIndex) ||
        (document.matchIndex as number) < 0)) ||
    !isRecord(document.facets) ||
    !Object.entries(document.facets).every(
      ([eventType, count]) =>
        isEventHistoryEventType(eventType) &&
        typeof count === "number" &&
        Number.isSafeInteger(count) &&
        count >= 0,
    ) ||
    !isRecord(completion) ||
    !isStringArray(completion.eventTypes) ||
    !completion.eventTypes.every(isEventHistoryEventType) ||
    new Set(completion.eventTypes).size !== completion.eventTypes.length ||
    !isStringArray(completion.trackIds) ||
    !isStringArray(completion.changeIds) ||
    !Array.isArray(completion.revisionRefs) ||
    !completion.revisionRefs.every(isEventHistoryRevisionRef) ||
    !isStringArray(completion.unresolvedRevisionIds) ||
    !isStringArray(document.diagnostics) ||
    !isStringArray(document.queryNotices) ||
    !Array.isArray(document.entries) ||
    document.entries.length > 100 ||
    !document.entries.every(isEventHistoryEntry) ||
    (document.matchCount as number) > (document.eventCount as number) ||
    (document.offset as number) > (document.matchCount as number) ||
    (document.offset as number) + document.entries.length >
      (document.matchCount as number) ||
    (document.previous !== undefined && !nonEmptyString(document.previous)) ||
    (document.next !== undefined && !nonEmptyString(document.next))
  ) {
    throw new Error("invalid event history DTO");
  }
  if (
    (document.offset as number) + document.entries.length >
    (document.matchCount as number)
  ) {
    throw new Error("event history page exceeds its match count");
  }
  return {
    ...(document as unknown as EventHistoryDocument),
    authorityCursor,
  };
}

const AUTHORITY_CURSOR_V2_KEYS = new Set([
  "schema",
  "journalRecordCount",
  "eventCount",
  "journalRecordSetHash",
  "eventSetHash",
  "capabilitySetHash",
]);
const PREFIXED_SHA256 = /^sha256:[0-9a-f]{64}$/;

/** Decode the closed authority-generation identity shared by all v2 readers. */
export function decodeAuthorityCursorV2(value: unknown): AuthorityCursorV2 {
  const cursor = object(value, "authority cursor");
  if (
    !hasExactKeys(cursor, AUTHORITY_CURSOR_V2_KEYS) ||
    cursor.schema !== "pointbreak.authority-cursor.v2" ||
    !isNonnegativeSafeInteger(cursor.journalRecordCount) ||
    !isNonnegativeSafeInteger(cursor.eventCount) ||
    cursor.eventCount > cursor.journalRecordCount ||
    typeof cursor.journalRecordSetHash !== "string" ||
    !PREFIXED_SHA256.test(cursor.journalRecordSetHash) ||
    typeof cursor.eventSetHash !== "string" ||
    !PREFIXED_SHA256.test(cursor.eventSetHash) ||
    typeof cursor.capabilitySetHash !== "string" ||
    !PREFIXED_SHA256.test(cursor.capabilitySetHash)
  ) {
    throw new Error("invalid authority cursor DTO");
  }
  return cursor as unknown as AuthorityCursorV2;
}

export function decodeReaderProfile(value: unknown): ReaderProfile {
  const profile = object(value, "Inspector reader profile");
  const availability = profile.availability;
  const authorityCursor = decodeAuthorityCursorV2(profile.authorityCursor);
  const documents = profile.documents;
  const minimumReaderProfile = profile.minimumReaderProfile;
  const commitGraphStamp = profile.commitGraphStamp;
  if (
    profile.schema !== "pointbreak.inspect-reader-profile" ||
    profile.version !== 1 ||
    !isReaderProfileAvailability(availability) ||
    !isDocumentMap(documents) ||
    !sameDocumentMap(documents, CHANGE_READER_DOCUMENTS)
  ) {
    throw new Error("incompatible Inspector reader profile");
  }
  if (
    availability === "ready" &&
    (minimumReaderProfile !== CHANGE_READER_PROFILE ||
      typeof commitGraphStamp !== "string" ||
      commitGraphStamp.length === 0)
  ) {
    throw new Error(
      "ready Inspector reader profile is missing capability or commit graph stamp",
    );
  }
  return {
    schema: "pointbreak.inspect-reader-profile",
    version: 1,
    availability,
    minimumReaderProfile:
      typeof minimumReaderProfile === "string"
        ? minimumReaderProfile
        : undefined,
    authorityCursor,
    commitGraphStamp:
      typeof commitGraphStamp === "string" ? commitGraphStamp : undefined,
    documents,
  };
}

export function decodeChangePage(
  value: unknown,
  expected: { lens: "changes"; bounded: boolean },
): ChangesPage;
export function decodeChangePage(
  value: unknown,
  expected: { lens: "attention"; bounded: boolean },
): AttentionPage;
export function decodeChangePage(
  value: unknown,
  expected: { lens: ChangeLens; bounded: boolean },
): ChangePage {
  const page = object(value, `${expected.lens} Change page`);
  const expectedSchema =
    expected.lens === "changes"
      ? "pointbreak.inspect-changes-page"
      : "pointbreak.inspect-attention";
  const expectedVersion = expected.lens === "changes" ? 1 : 2;
  const stamp = page.projectionStamp;
  const changes = page.changes;
  const diagnostics = page.diagnostics;
  const presentations = page.presentations;
  if (
    page.schema !== expectedSchema ||
    page.version !== expectedVersion ||
    !nonEmptyString(stamp) ||
    !Array.isArray(changes) ||
    (expected.bounded && changes.length > 100) ||
    !changes.every((change) => isChangeSummary(change, stamp)) ||
    !isStrictlyAscending(changes.map((change) => change.changeId)) ||
    new Set(changes.map((change) => change.changeId)).size !== changes.length ||
    (diagnostics !== undefined && !isStringArray(diagnostics)) ||
    (presentations !== undefined &&
      !isPresentations(presentations, changes, expected.lens))
  ) {
    throw new Error(`invalid ${expected.lens} Change page DTO`);
  }
  const capability = (name: "previous" | "next" | "last") => {
    const candidate = page[name];
    if (
      candidate !== undefined &&
      candidate !== null &&
      (!nonEmptyString(candidate) ||
        new TextEncoder().encode(candidate).length > 4096)
    ) {
      throw new Error(`invalid Change page ${name} continuation`);
    }
    return candidate as string | null | undefined;
  };
  const previous = capability("previous");
  const next = capability("next");
  const last = capability("last");
  if (expected.bounded && next === undefined)
    throw new Error("bounded Change page is missing next continuation");
  const common = {
    changes,
    diagnostics,
    presentations,
    projectionStamp: stamp,
    ...(previous === undefined ? {} : { previous }),
    next: next ?? null,
    ...(last === undefined ? {} : { last }),
  };
  return expected.lens === "changes"
    ? {
        schema: "pointbreak.inspect-changes-page",
        version: 1,
        ...common,
      }
    : {
        schema: "pointbreak.inspect-attention",
        version: 2,
        ...common,
      };
}

export function requireCoherentGeneration(
  changes: ChangesPage,
  attention: AttentionPage,
): void {
  if (changes.projectionStamp !== attention.projectionStamp) {
    throw new Error("Change documents do not form one coherent generation");
  }
}

/** Compare the full capability/freshness state after staging, independent of key order. */
export function sameProfileGeneration(
  initial: ReaderProfile,
  postflight: ReaderProfile,
): boolean {
  return (
    initial.availability === postflight.availability &&
    initial.minimumReaderProfile === postflight.minimumReaderProfile &&
    initial.commitGraphStamp === postflight.commitGraphStamp &&
    sameDocumentMap(initial.documents, postflight.documents) &&
    sameAuthorityCursor(initial.authorityCursor, postflight.authorityCursor)
  );
}

/** Compare the canonical closed cursor value, independent of object key order. */
export function sameAuthorityCursor(
  left: AuthorityCursorV2,
  right: AuthorityCursorV2,
): boolean {
  return canonicalJson(left) === canonicalJson(right);
}

/** Match Rust's Unicode-whitespace edge trim before byte counting or folding. */
export function trimUnicodeWhitespace(value: string): string {
  return value.replace(/^\p{White_Space}+|\p{White_Space}+$/gu, "");
}

function appendEnum(
  params: URLSearchParams,
  name: string,
  value: string | undefined,
  values: ReadonlySet<string>,
): void {
  if (value === undefined) return;
  if (!values.has(value)) throw new Error(`invalid Change page ${name}`);
  params.set(name, value);
}

function isReaderProfileAvailability(
  value: unknown,
): value is ReaderProfileAvailability {
  return (
    value === "migration_required" ||
    value === "migration_in_progress" ||
    value === "ready"
  );
}

function isChangeSummary(
  value: unknown,
  stamp: string,
): value is ChangeSummary {
  if (!isRecord(value)) return false;
  return (
    nonEmptyString(value.changeId) &&
    (value.declarationState === "authoritative" ||
      value.declarationState === "incomplete" ||
      value.declarationState === "conflicted") &&
    isStringArray(value.titleAssertions) &&
    typeof value.memberCount === "number" &&
    Number.isSafeInteger(value.memberCount) &&
    value.memberCount >= 0 &&
    isOneOf(value.topology, TOPOLOGY_VALUES) &&
    isOneOf(value.lifecycle, LIFECYCLE_VALUES) &&
    isOneOf(value.attentionSummary, ATTENTION_VALUES) &&
    isOneOf(value.availabilitySummary, AVAILABILITY_VALUES) &&
    value.projectionStamp === stamp &&
    Array.isArray(value.currentRevisionRefs) &&
    value.currentRevisionRefs.every(isRevisionRef) &&
    uniqueRevisionKeys(value.currentRevisionRefs).size ===
      value.currentRevisionRefs.length &&
    (value.diagnostics === undefined || isStringArray(value.diagnostics))
  );
}

function isClaimSupport(value: unknown): value is ChangeClaimSupport {
  return (
    isRecord(value) &&
    nonEmptyString(value.eventId) &&
    nonEmptyString(value.actorId) &&
    optionalString(value.trackId)
  );
}

function isChangeMemberRevisions(
  value: unknown,
): value is ChangeMemberRevision[] {
  return (
    Array.isArray(value) &&
    value.every(
      (member) =>
        isRecord(member) &&
        isRevisionRef(member.revision) &&
        isStringArray(member.supportingClaimIds),
    )
  );
}

function isUnavailableChangeMemberRevisions(
  value: unknown,
): value is UnavailableChangeMemberRevision[] {
  return (
    Array.isArray(value) &&
    value.every(
      (member) =>
        isRecord(member) &&
        nonEmptyString(member.revisionId) &&
        (member.reason === "invalid_revision_id" ||
          member.reason === "invalid_object_artifact_content_hash") &&
        isStringArray(member.supportingClaimIds),
    )
  );
}

function isMembershipClaims(
  value: unknown,
  changeId: string,
): value is ChangeMembershipClaim[] {
  return (
    Array.isArray(value) &&
    value.every(
      (claim) =>
        isRecord(claim) &&
        nonEmptyString(claim.claimId) &&
        claim.changeId === changeId &&
        nonEmptyString(claim.revisionId) &&
        Array.isArray(claim.supports) &&
        claim.supports.every(isClaimSupport) &&
        Array.isArray(claim.withdrawals) &&
        claim.withdrawals.every(isClaimSupport) &&
        typeof claim.active === "boolean" &&
        isStringArray(claim.diagnostics),
    )
  );
}

function isClaimWithdrawals(value: unknown): value is ChangeClaimWithdrawal[] {
  return (
    Array.isArray(value) &&
    value.every(
      (withdrawal) =>
        isRecord(withdrawal) &&
        nonEmptyString(withdrawal.claimId) &&
        Array.isArray(withdrawal.supports) &&
        withdrawal.supports.every(isClaimSupport) &&
        isStringArray(withdrawal.diagnostics),
    )
  );
}

function isChangeLinks(value: unknown): value is ChangeLink[] {
  return (
    Array.isArray(value) &&
    value.every(
      (link) =>
        isRecord(link) &&
        nonEmptyString(link.leftChangeId) &&
        nonEmptyString(link.rightChangeId) &&
        nonEmptyString(link.relation),
    )
  );
}

function isEffectiveSupersedes(
  value: unknown,
): value is Array<[RevisionRef, RevisionRef]> {
  return (
    Array.isArray(value) &&
    value.every(
      (edge) =>
        Array.isArray(edge) &&
        edge.length === 2 &&
        isRevisionRef(edge[0]) &&
        isRevisionRef(edge[1]),
    )
  );
}

function isRevisionQualifications(
  value: unknown,
  currentRevisionRefs: RevisionRef[],
): value is RevisionQualification[] {
  if (!Array.isArray(value)) return false;
  const qualifications: RevisionQualification[] = [];
  for (const candidate of value) {
    if (!isRecord(candidate) || !isRevisionRef(candidate.revision)) {
      return false;
    }
    const revision = candidate.revision;
    if (
      typeof candidate.qualified !== "boolean" ||
      !currentRevisionRefs.some((current) => sameRevision(current, revision))
    )
      return false;
    qualifications.push({
      revision,
      qualified: candidate.qualified,
    });
  }
  return sameRevisionSet(
    qualifications.map((qualification) => qualification.revision),
    currentRevisionRefs,
  );
}

function sameRevisionSet(left: RevisionRef[], right: RevisionRef[]): boolean {
  const leftKeys = uniqueRevisionKeys(left);
  const rightKeys = uniqueRevisionKeys(right);
  return (
    leftKeys.size === left.length &&
    rightKeys.size === right.length &&
    leftKeys.size === rightKeys.size &&
    [...leftKeys].every((key) => rightKeys.has(key))
  );
}

function isPresentations(
  value: unknown,
  changes: ChangeSummary[],
  lens: ChangeLens,
): value is Record<string, ChangePresentation> {
  if (!isRecord(value)) return false;
  const summaries = new Map(
    changes.map((change) => [change.changeId, change] as const),
  );
  if (Object.keys(value).length !== summaries.size) return false;
  return Object.entries(value).every(([changeId, presentation]) => {
    const change = summaries.get(changeId);
    if (
      change === undefined ||
      !isRecord(presentation) ||
      !Array.isArray(presentation.currentRevisions) ||
      !presentation.currentRevisions.every(isPresentationRevision) ||
      (lens === "attention"
        ? !isAttentionPresentation(presentation.attention)
        : presentation.attention !== undefined)
    ) {
      return false;
    }
    const expected = uniqueRevisionKeys(change.currentRevisionRefs);
    const actual = uniqueRevisionKeys(
      presentation.currentRevisions.map((candidate) => candidate.revision),
    );
    return (
      expected.size === change.currentRevisionRefs.length &&
      actual.size === presentation.currentRevisions.length &&
      expected.size === actual.size &&
      [...expected].every((key) => actual.has(key))
    );
  });
}

function isAttentionPresentation(
  value: unknown,
): value is ChangeAttentionPresentation {
  if (!isRecord(value)) return false;
  const reasons = value.reasons;
  const reasonPresentations = value.reasonPresentations;
  if (
    !isAttentionReason(value.primaryReason) ||
    !Array.isArray(reasons) ||
    reasons.length === 0 ||
    !reasons.every(isAttentionReason) ||
    !sameAttentionReason(value.primaryReason, reasons[0]) ||
    !Array.isArray(reasonPresentations) ||
    reasonPresentations.length !== reasons.length ||
    !reasonPresentations.every(
      (presentation, index) =>
        isAttentionReasonPresentation(presentation) &&
        sameAttentionReason(presentation.cause, reasons[index]),
    ) ||
    (value.diagnostics !== undefined && !isStringArray(value.diagnostics))
  ) {
    return false;
  }
  return true;
}

const ATTENTION_REASON_PRESENTATION_KEYS = new Set([
  "cause",
  "ask",
  "reason",
  "evidence",
  "nextAction",
]);

function nonBlankAttentionCopy(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

function isAttentionReasonPresentation(
  value: unknown,
): value is ChangeAttentionReasonPresentation {
  return (
    isRecord(value) &&
    hasExactKeys(value, ATTENTION_REASON_PRESENTATION_KEYS) &&
    isAttentionReason(value.cause) &&
    nonBlankAttentionCopy(value.ask) &&
    nonBlankAttentionCopy(value.reason) &&
    nonBlankAttentionCopy(value.evidence) &&
    nonBlankAttentionCopy(value.nextAction)
  );
}

function isAttentionReason(value: unknown): value is ChangeAttentionReason {
  if (!isRecord(value)) return false;
  switch (value.kind) {
    case "conflicted":
    case "incomplete":
    case "no_current_revision":
      return Object.keys(value).length === 1;
    case "unresolved_operative_requests":
      return (
        Object.keys(value).length === 2 &&
        Array.isArray(value.requestIds) &&
        value.requestIds.length > 0 &&
        value.requestIds.every(nonEmptyString) &&
        new Set(value.requestIds).size === value.requestIds.length
      );
    case "current_revisions_need_assessment":
      return (
        Object.keys(value).length === 2 &&
        Array.isArray(value.revisions) &&
        value.revisions.length > 0 &&
        value.revisions.every(isRevisionRef) &&
        uniqueRevisionKeys(value.revisions).size === value.revisions.length
      );
    default:
      return false;
  }
}

function sameAttentionReason(
  left: ChangeAttentionReason,
  right: ChangeAttentionReason,
): boolean {
  if (left.kind !== right.kind) return false;
  if (
    left.kind === "unresolved_operative_requests" &&
    right.kind === "unresolved_operative_requests"
  ) {
    return (
      left.requestIds.length === right.requestIds.length &&
      left.requestIds.every(
        (requestId, index) => requestId === right.requestIds[index],
      )
    );
  }
  if (
    left.kind === "current_revisions_need_assessment" &&
    right.kind === "current_revisions_need_assessment"
  ) {
    return (
      left.revisions.length === right.revisions.length &&
      left.revisions.every((revision, index) =>
        sameRevision(revision, right.revisions[index]),
      )
    );
  }
  return true;
}

function isPresentationRevision(value: unknown): boolean {
  return (
    isRecord(value) &&
    isRevisionRef(value.revision) &&
    ((value.summarySource === "revision_proposal_summary" &&
      nonEmptyString(value.revisionProposalSummary)) ||
      (value.summarySource === "absent" &&
        value.revisionProposalSummary === undefined))
  );
}

function isRevisionRef(value: unknown): value is RevisionRef {
  return (
    isRecord(value) &&
    nonEmptyString(value.revisionId) &&
    nonEmptyString(value.objectArtifactContentHash)
  );
}

function isChangeDetailInspectorPresentation(
  value: unknown,
  detail: Pick<
    ChangeDetail,
    | "memberRevisions"
    | "currentRevisionRefs"
    | "effectiveSupersedes"
    | "pendingOrConflictingEdges"
    | "diagnostics"
  >,
): value is ChangeDetail["inspectorPresentation"] {
  if (value === undefined) return true;
  if (
    !isRecord(value) ||
    !isChangeRevisionGraphPresentation(value.revisionGraph)
  )
    return false;
  const graph = value.revisionGraph;
  const expectedMembers = new Set(
    detail.memberRevisions.map((member) =>
      revisionGraphNodeId(member.revision),
    ),
  );
  const expectedCurrent = new Set(
    detail.currentRevisionRefs.map(revisionGraphNodeId),
  );
  const expectedNodes = new Set(expectedMembers);
  for (const claim of detail.pendingOrConflictingEdges) {
    expectedNodes.add(revisionGraphNodeId(claim.successor));
    expectedNodes.add(revisionGraphNodeId(claim.predecessor));
  }
  const actualNodes = new Map(graph.nodes.map((node) => [node.id, node]));
  if (
    actualNodes.size !== expectedNodes.size ||
    ![...expectedNodes].every((id) => actualNodes.has(id)) ||
    !graph.nodes.every(
      (node) =>
        node.isMember === expectedMembers.has(node.id) &&
        node.isCurrent === expectedCurrent.has(node.id),
    ) ||
    !sameStringArray(graph.diagnostics ?? [], detail.diagnostics)
  ) {
    return false;
  }
  const effective = new Set(
    detail.effectiveSupersedes.map(([successor, predecessor]) =>
      graphEdgeKey(
        revisionGraphNodeId(successor),
        revisionGraphNodeId(predecessor),
      ),
    ),
  );
  const graphEffective = new Set(
    graph.effectiveSupersedes.map((edge) => graphEdgeKey(edge.from, edge.to)),
  );
  if (!sameStringSet(effective, graphEffective)) return false;
  const pending = new Map(
    detail.pendingOrConflictingEdges.map((claim) => [claim.claimId, claim]),
  );
  return (
    pending.size === graph.pendingOrConflictingClaims.length &&
    graph.pendingOrConflictingClaims.every((edge) => {
      const claim = pending.get(edge.claimId);
      return (
        claim !== undefined &&
        sameRevision(edge.successor, claim.successor) &&
        sameRevision(edge.predecessor, claim.predecessor) &&
        sameStringArray(edge.diagnostics, claim.diagnostics)
      );
    })
  );
}

function isChangeRevisionDetailInspectorPresentation(
  value: unknown,
  detail: Pick<
    ChangeRevisionDetail,
    "revision" | "factPresentations" | "factPorts"
  >,
): value is ChangeRevisionDetail["inspectorPresentation"] {
  if (value === undefined) return true;
  if (!isRecord(value) || !isFactRelationshipGraphPresentation(value.factGraph))
    return false;
  const graph = value.factGraph;
  const expectedActivation = new Map<string, RevisionRef>();
  const expectedNodes = new Set<string>();
  for (const fact of detail.factPresentations) {
    const id = factGraphNodeId(fact.originRevision, fact.family, fact.factId);
    expectedNodes.add(id);
    if (
      sameRevision(fact.originRevision, detail.revision) ||
      (fact.presentedInRevision !== undefined &&
        sameRevision(fact.presentedInRevision, detail.revision))
    ) {
      expectedActivation.set(id, detail.revision);
    }
  }
  for (const port of detail.factPorts) {
    expectedNodes.add(
      factGraphNodeId(
        port.originRevision,
        port.originFact.kind,
        factRefId(port.originFact),
      ),
    );
    const targetId =
      port.targetFact === undefined
        ? revisionGraphNodeId(port.targetRevision)
        : factGraphNodeId(
            port.targetRevision,
            port.targetFact.kind,
            factRefId(port.targetFact),
          );
    expectedNodes.add(targetId);
    if (
      port.targetFact === undefined &&
      sameRevision(port.targetRevision, detail.revision)
    ) {
      expectedActivation.set(targetId, detail.revision);
    }
  }
  const actualNodes = new Map(graph.nodes.map((node) => [node.id, node]));
  if (
    actualNodes.size !== expectedNodes.size ||
    ![...expectedNodes].every((id) => actualNodes.has(id)) ||
    !graph.nodes.every((node) => {
      const activation = expectedActivation.get(node.id);
      return activation === undefined
        ? node.contextAvailability === "relationship_context_only" &&
            node.activationRevision === undefined
        : node.contextAvailability === "available" &&
            node.activationRevision !== undefined &&
            sameRevision(node.activationRevision, activation);
    })
  ) {
    return false;
  }
  const ports = new Map(detail.factPorts.map((port) => [port.portId, port]));
  return (
    ports.size === graph.factPorts.length &&
    graph.factPorts.every((edge) => {
      const port = ports.get(edge.portId);
      return (
        port !== undefined &&
        sameRevision(edge.originRevision, port.originRevision) &&
        sameFactRef(edge.originFact, port.originFact) &&
        sameRevision(edge.targetRevision, port.targetRevision) &&
        sameOptionalFactRef(edge.targetFact, port.targetFact) &&
        edge.relation === port.relation &&
        edge.applicability === port.applicability &&
        sameStringArray(edge.diagnostics ?? [], port.diagnostics)
      );
    })
  );
}

function isChangeRevisionGraphPresentation(
  value: unknown,
): value is ChangeRevisionGraphPresentation {
  if (
    !isRecord(value) ||
    !Array.isArray(value.nodes) ||
    value.nodes.length === 0 ||
    !value.nodes.every(isChangeRevisionGraphNode) ||
    !Array.isArray(value.effectiveSupersedes) ||
    !Array.isArray(value.pendingOrConflictingClaims) ||
    !isGraphBounds(value.bounds) ||
    (value.diagnostics !== undefined && !isStringArray(value.diagnostics))
  ) {
    return false;
  }
  const nodes = new Map(value.nodes.map((node) => [node.id, node]));
  return (
    nodes.size === value.nodes.length &&
    value.effectiveSupersedes.every((edge) =>
      isChangeRevisionGraphEffectiveEdge(edge, nodes),
    ) &&
    uniqueGraphEdgeEndpoints(value.effectiveSupersedes) &&
    value.pendingOrConflictingClaims.every((edge) =>
      isChangeRevisionGraphClaimEdge(edge, nodes),
    ) &&
    new Set(value.pendingOrConflictingClaims.map((edge) => edge.claimId))
      .size === value.pendingOrConflictingClaims.length
  );
}

function isChangeRevisionGraphNode(
  value: unknown,
): value is ChangeRevisionGraphNode {
  return (
    isRecord(value) &&
    nonEmptyString(value.id) &&
    isRevisionRef(value.revision) &&
    value.id === revisionGraphNodeId(value.revision) &&
    isFiniteGeometry(value) &&
    typeof value.isCurrent === "boolean" &&
    typeof value.isMember === "boolean" &&
    isGraphContext(value) &&
    (value.isMember
      ? value.contextAvailability === "available" &&
        isRevisionRef(value.activationRevision) &&
        sameRevision(value.activationRevision, value.revision)
      : value.contextAvailability === "relationship_context_only" &&
        value.activationRevision === undefined)
  );
}

function isChangeRevisionGraphEffectiveEdge(
  value: unknown,
  nodes: ReadonlyMap<string, ChangeRevisionGraphNode>,
): value is ChangeRevisionGraphEffectiveEdge {
  return (
    isRecord(value) &&
    nonEmptyString(value.from) &&
    nonEmptyString(value.to) &&
    isRevisionRef(value.successor) &&
    isRevisionRef(value.predecessor) &&
    value.from === revisionGraphNodeId(value.successor) &&
    value.to === revisionGraphNodeId(value.predecessor) &&
    nodes.has(value.from) &&
    nodes.has(value.to) &&
    isGraphPath(value.path)
  );
}

function isChangeRevisionGraphClaimEdge(
  value: unknown,
  nodes: ReadonlyMap<string, ChangeRevisionGraphNode>,
): value is ChangeRevisionGraphClaimEdge {
  if (!isRecord(value) || !isChangeRevisionGraphEffectiveEdge(value, nodes))
    return false;
  return nonEmptyString(value.claimId) && isStringArray(value.diagnostics);
}

function isFactRelationshipGraphPresentation(
  value: unknown,
): value is FactRelationshipGraphPresentation {
  if (
    !isRecord(value) ||
    !Array.isArray(value.nodes) ||
    value.nodes.length === 0 ||
    !value.nodes.every(isFactRelationshipGraphNode) ||
    !Array.isArray(value.observationSupersedes) ||
    !Array.isArray(value.assessmentReplaces) ||
    !Array.isArray(value.factPorts) ||
    !isGraphBounds(value.bounds)
  ) {
    return false;
  }
  const nodes = new Map(value.nodes.map((node) => [node.id, node]));
  return (
    nodes.size === value.nodes.length &&
    value.observationSupersedes.every((edge) =>
      isFactRelationshipEdge(edge, "observation", nodes),
    ) &&
    uniqueGraphEdgeEndpoints(value.observationSupersedes) &&
    value.assessmentReplaces.every((edge) =>
      isFactRelationshipEdge(edge, "assessment", nodes),
    ) &&
    uniqueGraphEdgeEndpoints(value.assessmentReplaces) &&
    value.factPorts.every((edge) => isFactPortRelationshipEdge(edge, nodes)) &&
    new Set(value.factPorts.map((edge) => edge.portId)).size ===
      value.factPorts.length
  );
}

function isFactRelationshipGraphNode(
  value: unknown,
): value is FactRelationshipGraphNode {
  if (
    !isRecord(value) ||
    !nonEmptyString(value.id) ||
    !isRevisionRef(value.revision) ||
    !isFiniteGeometry(value) ||
    !isGraphContext(value)
  ) {
    return false;
  }
  if (value.kind === "fact") {
    return (
      nonEmptyString(value.factId) &&
      nonEmptyString(value.family) &&
      value.id === factGraphNodeId(value.revision, value.family, value.factId)
    );
  }
  return (
    value.kind === "revision" &&
    value.factId === undefined &&
    value.family === undefined &&
    value.id === revisionGraphNodeId(value.revision)
  );
}

function isGraphContext(value: Record<string, unknown>): boolean {
  if (value.contextAvailability === "available") {
    return isRevisionRef(value.activationRevision);
  }
  return (
    value.contextAvailability === "relationship_context_only" &&
    value.activationRevision === undefined
  );
}

function isFactRelationshipEdge(
  value: unknown,
  family: string,
  nodes: ReadonlyMap<string, FactRelationshipGraphNode>,
): value is FactRelationshipEdge {
  return (
    isRecord(value) &&
    nonEmptyString(value.from) &&
    nonEmptyString(value.to) &&
    isRevisionRef(value.originRevision) &&
    nonEmptyString(value.fromFactId) &&
    nonEmptyString(value.toFactId) &&
    value.from ===
      factGraphNodeId(value.originRevision, family, value.fromFactId) &&
    value.to ===
      factGraphNodeId(value.originRevision, family, value.toFactId) &&
    nodes.has(value.from) &&
    nodes.has(value.to) &&
    isGraphPath(value.path)
  );
}

function isFactPortRelationshipEdge(
  value: unknown,
  nodes: ReadonlyMap<string, FactRelationshipGraphNode>,
): value is FactPortRelationshipEdge {
  if (
    !isRecord(value) ||
    !nonEmptyString(value.portId) ||
    !nonEmptyString(value.from) ||
    !nonEmptyString(value.to) ||
    !isRevisionRef(value.originRevision) ||
    !isFactRef(value.originFact) ||
    !isRevisionRef(value.targetRevision) ||
    (value.targetFact !== undefined && !isFactRef(value.targetFact)) ||
    (value.relation !== "context_only" &&
      value.relation !== "reanchored_as" &&
      value.relation !== "carried_open_as" &&
      value.relation !== "resolved_by") ||
    (value.applicability !== "applicable" &&
      value.applicability !== "conflicted" &&
      value.applicability !== "unavailable") ||
    !isGraphPath(value.path) ||
    (value.diagnostics !== undefined && !isStringArray(value.diagnostics))
  ) {
    return false;
  }
  const from = factGraphNodeId(
    value.originRevision,
    value.originFact.kind,
    factRefId(value.originFact),
  );
  const to =
    value.targetFact === undefined
      ? revisionGraphNodeId(value.targetRevision)
      : factGraphNodeId(
          value.targetRevision,
          value.targetFact.kind,
          factRefId(value.targetFact),
        );
  return (
    value.from === from && value.to === to && nodes.has(from) && nodes.has(to)
  );
}

function isGraphBounds(value: unknown): value is GraphBounds {
  return isRecord(value) && isFiniteNumber(value.w) && isFiniteNumber(value.h);
}

function isFiniteGeometry(value: Record<string, unknown>): boolean {
  return (
    isFiniteNumber(value.x) &&
    isFiniteNumber(value.y) &&
    isFiniteNumber(value.w) &&
    isFiniteNumber(value.h)
  );
}

function isGraphPath(value: unknown): value is GraphPoint[] {
  return (
    Array.isArray(value) &&
    value.length > 0 &&
    value.every(
      (point) =>
        Array.isArray(point) &&
        point.length === 2 &&
        isFiniteNumber(point[0]) &&
        isFiniteNumber(point[1]),
    )
  );
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function uniqueGraphEdgeEndpoints(
  edges: Array<{ from: string; to: string }>,
): boolean {
  return (
    new Set(edges.map((edge) => `${edge.from}\u0000${edge.to}`)).size ===
    edges.length
  );
}

function revisionGraphNodeId(revision: RevisionRef): string {
  return `revision:${revision.revisionId}@${revision.objectArtifactContentHash}`;
}

function factGraphNodeId(
  revision: RevisionRef,
  family: string,
  factId: string,
): string {
  return `${revisionGraphNodeId(revision).replace("revision:", "fact:")}:${family}:${factId}`;
}

function graphEdgeKey(from: string, to: string): string {
  return `${from}\u0000${to}`;
}

function sameStringArray(left: string[], right: string[]): boolean {
  return (
    left.length === right.length &&
    left.every((value, index) => value === right[index])
  );
}

function sameStringSet(left: Set<string>, right: Set<string>): boolean {
  return (
    left.size === right.size && [...left].every((value) => right.has(value))
  );
}

function sameFactRef(left: FactRef, right: FactRef): boolean {
  return left.kind === right.kind && factRefId(left) === factRefId(right);
}

function sameOptionalFactRef(
  left: FactRef | undefined,
  right: FactRef | undefined,
): boolean {
  return (
    (left === undefined && right === undefined) ||
    (left !== undefined && right !== undefined && sameFactRef(left, right))
  );
}

function uniqueRevisionKeys(revisions: RevisionRef[]): Set<string> {
  return new Set(
    revisions.map(
      (revision) =>
        `${revision.revisionId}\u0000${revision.objectArtifactContentHash}`,
    ),
  );
}

function isRelationClaim(
  value: unknown,
  changeId: string,
): value is ChangeRelationClaim {
  return (
    isRecord(value) &&
    nonEmptyString(value.claimId) &&
    value.changeId === changeId &&
    typeof value.active === "boolean" &&
    isRevisionRef(value.successor) &&
    isRevisionRef(value.predecessor) &&
    Array.isArray(value.supports) &&
    value.supports.every(isClaimSupport) &&
    Array.isArray(value.withdrawals) &&
    value.withdrawals.every(isClaimSupport) &&
    isStringArray(value.diagnostics)
  );
}

function isFactPresentation(
  value: unknown,
): value is ChangeRevisionDetail["factPresentations"][number] {
  return (
    isRecord(value) &&
    nonEmptyString(value.factId) &&
    nonEmptyString(value.family) &&
    isRevisionRef(value.originRevision) &&
    (value.target === undefined || isFactTarget(value.target)) &&
    (value.contextChangeId === undefined ||
      nonEmptyString(value.contextChangeId)) &&
    (value.presentedInRevision === undefined ||
      isRevisionRef(value.presentedInRevision)) &&
    (value.portRelation === undefined ||
      value.portRelation === "context_only" ||
      value.portRelation === "reanchored_as" ||
      value.portRelation === "carried_open_as" ||
      value.portRelation === "resolved_by") &&
    nonEmptyString(value.actorId) &&
    (value.trackId === undefined || nonEmptyString(value.trackId)) &&
    isOneOf(value.revisionCurrency, REVISION_CURRENCY_VALUES) &&
    isOneOf(value.familyState, FACT_FAMILY_STATE_VALUES) &&
    isOneOf(value.availability, CONTENT_AVAILABILITY_VALUES)
  );
}

function isFactTarget(value: unknown): value is FactTarget {
  if (!isRecord(value) || !nonEmptyString(value.revisionId)) return false;
  if (value.kind === "revision") return true;
  if (value.kind === "file") return nonEmptyString(value.filePath);
  if (value.kind === "range") {
    return (
      nonEmptyString(value.filePath) &&
      (value.side === "old" || value.side === "new") &&
      Number.isSafeInteger(value.startLine) &&
      (value.startLine as number) > 0 &&
      Number.isSafeInteger(value.endLine) &&
      (value.endLine as number) >= (value.startLine as number)
    );
  }
  if (value.kind === "observation") return nonEmptyString(value.observationId);
  if (value.kind === "input_request")
    return nonEmptyString(value.inputRequestId);
  if (value.kind === "assessment") return nonEmptyString(value.assessmentId);
  return value.kind === "event" && nonEmptyString(value.eventId);
}

function uniqueFactPresentationIds(
  facts: ChangeRevisionDetail["factPresentations"],
): boolean {
  return new Set(facts.map((fact) => fact.factId)).size === facts.length;
}

function isResourceProjection(
  value: unknown,
): value is RevisionResource["projection"] {
  return (
    isRecord(value) &&
    typeof value.includeBody === "boolean" &&
    (value.trackId === undefined || nonEmptyString(value.trackId))
  );
}

/**
 * Check the immutable bindings available in the served envelope.  Diff-row
 * shape remains renderer-owned, but schema, artifact hash, and object id are
 * protocol identity and must be established before the renderer sees bytes.
 */
function isCapturedReviewSnapshot(
  value: unknown,
  expectedContentHash: string,
  expectedObjectId: string,
): value is CapturedReviewSnapshot {
  if (
    !isRecord(value) ||
    value.schema !== "pointbreak.review-snapshot" ||
    value.version !== 1 ||
    value.contentHash !== expectedContentHash ||
    !isRecord(value.snapshot)
  ) {
    return false;
  }
  return (
    nonEmptyString(value.snapshot.review_id) &&
    value.snapshot.object_id === expectedObjectId &&
    Array.isArray(value.snapshot.files)
  );
}

function isFactContentPresentations(
  value: unknown,
): value is NonNullable<ChangeRevisionDetail["factContentPresentations"]> {
  return (
    isRecord(value) &&
    Object.values(value).every(
      (presentation) =>
        isRecord(presentation) &&
        (presentation.contentType === "text/plain" ||
          presentation.contentType === "text/markdown") &&
        (presentation.bodyContentState === "present" ||
          presentation.bodyContentState === "suppressed_present" ||
          presentation.bodyContentState === "physically_removed") &&
        isFactContent(presentation.content),
    )
  );
}

function sameFactIds(
  facts: ChangeRevisionDetail["factPresentations"],
  content: NonNullable<ChangeRevisionDetail["factContentPresentations"]>,
): boolean {
  const expected = new Set(facts.map((fact) => fact.factId));
  const actual = Object.keys(content);
  return (
    expected.size === facts.length &&
    expected.size === actual.length &&
    actual.every((factId) => expected.has(factId))
  );
}

function isRevisionResource(value: unknown): value is RevisionResource {
  try {
    decodeRevisionResource(value);
    return true;
  } catch {
    return false;
  }
}

function isFactPortPresentations(
  value: unknown,
  changeId: string,
  facts: ChangeRevisionDetail["factPresentations"],
  selectedRevision: RevisionRef,
): value is FactPortPresentation[] {
  if (!Array.isArray(value) || !value.every(isFactPortPresentation))
    return false;
  if (new Set(value.map((port) => port.portId)).size !== value.length)
    return false;
  return value.every(
    (port) =>
      (port.contextChangeId === undefined ||
        port.contextChangeId === changeId) &&
      port.sourceEventIds.length > 0 &&
      new Set(port.sourceEventIds).size === port.sourceEventIds.length &&
      port.trackId !== undefined &&
      (port.applicability !== "applicable" ||
        applicableFactPortHasExactEndpoints(port, facts, selectedRevision)),
  );
}

function factRefId(fact: FactRef): string {
  return fact.kind === "observation"
    ? (fact.observationId ?? "")
    : (fact.inputRequestId ?? "");
}

function applicableFactPortHasExactEndpoints(
  port: FactPortPresentation,
  facts: ChangeRevisionDetail["factPresentations"],
  selectedRevision: RevisionRef,
): boolean {
  if (!sameRevision(port.targetRevision, selectedRevision)) return false;
  const matchingOrigin = facts.filter(
    (fact) =>
      fact.factId === factRefId(port.originFact) &&
      fact.family === port.originFact.kind &&
      sameRevision(fact.originRevision, port.originRevision) &&
      fact.presentedInRevision !== undefined &&
      sameRevision(fact.presentedInRevision, selectedRevision),
  );
  if (matchingOrigin.length !== 1) return false;
  const targetFact = port.targetFact;
  if (targetFact === undefined) return true;
  return (
    facts.filter(
      (fact) =>
        fact.factId === factRefId(targetFact) &&
        fact.family === targetFact.kind &&
        sameRevision(fact.originRevision, selectedRevision),
    ).length === 1
  );
}

function isFactPortPresentation(value: unknown): value is FactPortPresentation {
  return (
    isRecord(value) &&
    nonEmptyString(value.portId) &&
    isRevisionRef(value.originRevision) &&
    isFactRef(value.originFact) &&
    isRevisionRef(value.targetRevision) &&
    (value.relation === "context_only" ||
      value.relation === "reanchored_as" ||
      value.relation === "carried_open_as" ||
      value.relation === "resolved_by") &&
    (value.targetFact === undefined || isFactRef(value.targetFact)) &&
    optionalString(value.rationaleContentHash) &&
    optionalString(value.contextChangeId) &&
    nonEmptyString(value.actorId) &&
    nonEmptyString(value.trackId) &&
    isStringArray(value.sourceEventIds) &&
    (value.applicability === "applicable" ||
      value.applicability === "conflicted" ||
      value.applicability === "unavailable") &&
    isStringArray(value.diagnostics)
  );
}

function isFactRef(value: unknown): value is FactRef {
  if (!isRecord(value)) return false;
  if (value.kind === "observation") {
    return (
      nonEmptyString(value.observationId) && value.inputRequestId === undefined
    );
  }
  if (value.kind === "input_request") {
    return (
      nonEmptyString(value.inputRequestId) && value.observationId === undefined
    );
  }
  return false;
}

function isFactContent(value: unknown): value is FactContent {
  if (!isRecord(value)) return false;
  switch (value.kind) {
    case "observation":
      return nonEmptyString(value.title) && optionalString(value.body);
    case "input_request":
      return (
        nonEmptyString(value.title) &&
        optionalString(value.body) &&
        nonEmptyString(value.status) &&
        (value.responses === undefined ||
          (Array.isArray(value.responses) &&
            value.responses.every(isFactResponse)))
      );
    case "assessment":
      return nonEmptyString(value.assessment) && optionalString(value.summary);
    case "validation":
      return (
        nonEmptyString(value.checkName) &&
        optionalString(value.command) &&
        nonEmptyString(value.status) &&
        optionalString(value.summary)
      );
    default:
      return false;
  }
}

function isFactResponse(value: unknown): boolean {
  return (
    isRecord(value) &&
    nonEmptyString(value.responseId) &&
    nonEmptyString(value.outcome) &&
    optionalString(value.reason) &&
    (value.contentType === "text/plain" ||
      value.contentType === "text/markdown") &&
    (value.bodyContentState === "present" ||
      value.bodyContentState === "suppressed_present" ||
      value.bodyContentState === "physically_removed") &&
    isOneOf(value.availability, CONTENT_AVAILABILITY_VALUES)
  );
}

function isAssociation(value: unknown): value is AssociationComparison {
  return (
    isRecord(value) &&
    value.schema === "pointbreak.review-association-comparison" &&
    value.version === 1 &&
    isOneOf(value.state, ASSOCIATION_STATE_VALUES) &&
    isOneOf(value.proofAvailability, ASSOCIATION_PROOF_VALUES) &&
    isRecord(value.comparison) &&
    isRevisionRef(value.comparison.revision) &&
    nonEmptyString(value.comparison.associationId) &&
    nonEmptyString(value.comparison.commitOid) &&
    nonEmptyString(value.comparison.comparisonBase) &&
    nonEmptyString(value.comparison.viewKind) &&
    optionalString(value.comparison.proofRef) &&
    isStringArray(value.diagnostics) &&
    nonEmptyString(value.cacheKey)
  );
}

function sameRevision(left: RevisionRef, right: RevisionRef): boolean {
  return (
    left.revisionId === right.revisionId &&
    left.objectArtifactContentHash === right.objectArtifactContentHash
  );
}

function isStrictlyAscending(values: string[]): boolean {
  return values.every((value, index) => {
    const previous = values[index - 1];
    return index === 0 || (previous !== undefined && previous < value);
  });
}

function isOneOf(value: unknown, values: ReadonlySet<string>): value is string {
  return typeof value === "string" && values.has(value);
}

function isDocumentMap(value: unknown): value is Record<string, number> {
  return (
    isRecord(value) &&
    Object.values(value).every((version) => Number.isInteger(version))
  );
}

function sameDocumentMap(
  left: Record<string, number>,
  right: Readonly<Record<string, number>>,
): boolean {
  const leftEntries = Object.entries(left).sort(([a], [b]) =>
    a.localeCompare(b),
  );
  const rightEntries = Object.entries(right).sort(([a], [b]) =>
    a.localeCompare(b),
  );
  return (
    leftEntries.length === rightEntries.length &&
    leftEntries.every(
      ([schema, version], index) =>
        schema === rightEntries[index]?.[0] &&
        version === rightEntries[index]?.[1],
    )
  );
}

function hasExactKeys(
  value: Record<string, unknown>,
  expected: ReadonlySet<string>,
): boolean {
  const actual = Object.keys(value);
  return (
    actual.length === expected.size && actual.every((key) => expected.has(key))
  );
}

function isNonnegativeSafeInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}

function canonicalJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (isRecord(value)) {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function object(value: unknown, name: string): Record<string, unknown> {
  if (!isRecord(value)) throw new Error(`invalid ${name} DTO`);
  return value;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function nonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

function optionalString(value: unknown): boolean {
  return value === undefined || typeof value === "string";
}

function isStringArray(value: unknown): value is string[] {
  return (
    Array.isArray(value) && value.every((item) => typeof item === "string")
  );
}
