/**
 * The Change-reader wire contract. The browser only constructs bounded requests
 * and validates server-owned projections; it never derives Change semantics.
 */

import changeReaderProfile from "../../../../documents/change_reader_profile_v1.json";

export type Availability =
  | "migration_required"
  | "migration_in_progress"
  | "ready";

export type ChangeLens = "changes" | "attention";

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
  availability: Availability;
  minimumReaderProfile?: string;
  authorityCursor: Record<string, unknown>;
  commitGraphStamp?: string;
  documents: Record<string, number>;
}

export interface RevisionRef {
  revisionId: string;
  objectArtifactContentHash: string;
}

export interface ChangePresentation {
  currentRevisions: Array<{
    revision: RevisionRef;
    revisionProposalSummary?: string;
    summarySource: "revision_proposal_summary" | "absent";
  }>;
}

export interface ChangeSummary {
  changeId: string;
  declarationState: "authoritative" | "incomplete" | "conflicted";
  titleAssertions: string[];
  memberCount: number;
  topology: string;
  lifecycle: string;
  attentionSummary: string;
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
  /** Opaque server continuation. It is absent on cohort-compatible bare responses. */
  next: string | null;
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
    const normalized = trimUnicodeWhitespace(query.q).toLowerCase();
    if (!normalized || new TextEncoder().encode(normalized).length > 256) {
      throw new Error(
        "Change page query must be non-empty and at most 256 bytes",
      );
    }
    params.set("q", normalized);
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

export function decodeReaderProfile(value: unknown): ReaderProfile {
  const profile = object(value, "Inspector reader profile");
  const availability = profile.availability;
  const authorityCursor = profile.authorityCursor;
  const documents = profile.documents;
  const minimumReaderProfile = profile.minimumReaderProfile;
  const commitGraphStamp = profile.commitGraphStamp;
  if (
    profile.schema !== "pointbreak.inspect-reader-profile" ||
    profile.version !== 1 ||
    !isAvailability(availability) ||
    !isRecord(authorityCursor) ||
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
    (presentations !== undefined && !isPresentations(presentations, changes))
  ) {
    throw new Error(`invalid ${expected.lens} Change page DTO`);
  }
  const next = page.next;
  if (next !== undefined && next !== null && !nonEmptyString(next)) {
    throw new Error("invalid Change page next continuation");
  }
  if (expected.bounded && next === undefined)
    throw new Error("bounded Change page is missing next continuation");
  const common = {
    changes,
    diagnostics,
    presentations,
    projectionStamp: stamp,
    next: next ?? null,
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
    canonicalJson(initial.authorityCursor) ===
      canonicalJson(postflight.authorityCursor)
  );
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

function isAvailability(value: unknown): value is Availability {
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
      !presentation.currentRevisions.every(isPresentationRevision)
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
