import changeReaderProfile from "../../../src/documents/change_reader_profile_v1.json";

export const CHANGE_READER_PROFILE = changeReaderProfile.minimumReaderProfile;

/**
 * One strict document map shared by the extension's cold CLI and warm
 * Inspector adapters. The product version is informational; this versioned
 * profile is the semantic compatibility authority.
 */
export const CHANGE_READER_DOCUMENTS = changeReaderProfile.documents;

export type ChangeReaderAvailability =
  | "migration_required"
  | "migration_in_progress"
  | "ready";

export interface AuthorityCursorV2 {
  readonly eventCount?: number;
  readonly [field: string]: unknown;
}

export interface ReaderProfileDoc {
  readonly schema: "pointbreak.inspect-reader-profile";
  readonly version: 1;
  readonly availability: ChangeReaderAvailability;
  readonly minimumReaderProfile?: string;
  readonly commitGraphStamp?: string;
  readonly authorityCursor: AuthorityCursorV2;
  readonly documents: Record<string, number>;
  readonly diagnostics?: unknown[];
}

export interface RevisionRefV1 {
  readonly revisionId: string;
  readonly objectArtifactContentHash: string;
}

export interface CurrentRevisionPresentationV1 {
  readonly revision: RevisionRefV1;
  readonly revisionProposalSummary?: string;
  readonly summarySource: "revision_proposal_summary" | "absent";
}

export interface ChangePresentationV1 {
  readonly currentRevisions: CurrentRevisionPresentationV1[];
}

export interface ChangeSummaryV1 {
  readonly changeId: string;
  readonly declarationState: string;
  readonly memberCount: number;
  readonly currentRevisionRefs: RevisionRefV1[];
  readonly topology: string;
  readonly lifecycle: string;
  readonly attentionSummary: string;
  readonly availabilitySummary: string;
  readonly diagnostics: string[];
  readonly projectionStamp: string;
}

export interface ChangeListDoc {
  readonly schema:
    | "pointbreak.review-change-list"
    | "pointbreak.inspect-changes-page";
  readonly version: 1;
  readonly changes: ChangeSummaryV1[];
  readonly diagnostics: string[];
  readonly presentations?: Readonly<Record<string, ChangePresentationV1>>;
  readonly projectionStamp: string;
}

export interface ChangeAttentionDoc {
  readonly schema: "pointbreak.attention-list" | "pointbreak.inspect-attention";
  readonly version: 2;
  readonly changes: ChangeSummaryV1[];
  readonly presentations?: Readonly<Record<string, ChangePresentationV1>>;
  readonly projectionStamp: string;
}

export interface RevisionResourceDoc {
  readonly schema: "pointbreak.review-revision-resource";
  readonly version: 1;
  readonly resource: {
    readonly revision: RevisionRefV1;
    readonly objectId: string;
  };
  readonly projection: {
    readonly trackId?: string;
    readonly includeBody: boolean;
  };
  readonly availability:
    | "available"
    | "removed"
    | "missing"
    | "mismatch"
    | "non_textual";
  readonly capturedDocumentHash?: string;
  readonly capturedDocument?: unknown;
  readonly diagnostics: string[];
  readonly cacheKey: string;
}

export interface ChangeRevisionDoc {
  readonly schema: "pointbreak.review-change-revision";
  readonly version: 1;
  readonly changeId: string;
  readonly revision: RevisionRefV1;
  readonly membershipSupport: unknown[];
  readonly revisionCurrency: string;
  readonly relationClassification: string;
  readonly exactRevisionDocument: RevisionResourceDoc;
  readonly factPresentations: Array<{
    readonly factId: string;
    readonly family: string;
    readonly originRevision: RevisionRefV1;
    readonly contextChangeId?: string;
    readonly presentedInRevision?: RevisionRefV1;
    readonly revisionCurrency: string;
    readonly familyState: string;
    readonly availability: string;
  }>;
  readonly factContentPresentations?: Readonly<
    Record<
      string,
      {
        readonly contentType: "text/plain" | "text/markdown";
        readonly bodyContentState:
          | "present"
          | "suppressed_present"
          | "physically_removed";
        readonly content: Readonly<Record<string, unknown>>;
      }
    >
  >;
  readonly associations: Array<{
    readonly comparison: { readonly revision: RevisionRefV1 };
    readonly [field: string]: unknown;
  }>;
  readonly availability: RevisionResourceDoc["availability"];
  readonly diagnostics: string[];
  readonly projectionStamp: string;
}

export interface ChangeDetailDoc {
  readonly schema: "pointbreak.review-change";
  readonly version: 1;
  readonly summary: ChangeSummaryV1;
  readonly relationClaims: unknown[];
  readonly currentRevisionRefs: RevisionRefV1[];
  readonly diagnostics: string[];
  readonly projectionStamp: string;
  readonly [field: string]: unknown;
}

export interface RevisionInterdiffDoc {
  readonly schema: "pointbreak.review-revision-interdiff";
  readonly version: 1;
  readonly interdiff: {
    readonly from: RevisionRefV1;
    readonly to: RevisionRefV1;
  };
  readonly availability: string;
  readonly comparison?: unknown;
  readonly diagnostics: string[];
  readonly cacheKey: string;
}

export class ChangeProtocolError extends Error {
  constructor(
    message = "Pointbreak returned an incompatible Change reader document.",
  ) {
    super(message);
    this.name = "ChangeProtocolError";
  }
}

export class ChangeReaderUnavailableError extends Error {
  constructor(
    readonly availability: Exclude<ChangeReaderAvailability, "ready">,
  ) {
    super(
      availability === "migration_required"
        ? "Pointbreak store migration is required before Change state can be read."
        : "Pointbreak store migration is in progress; partial Change state is unavailable.",
    );
    this.name = "ChangeReaderUnavailableError";
  }
}

export function validateReaderProfile(value: unknown): ReaderProfileDoc {
  if (
    !isObject(value) ||
    value.schema !== "pointbreak.inspect-reader-profile" ||
    value.version !== 1 ||
    !isAvailability(value.availability) ||
    !isObject(value.authorityCursor) ||
    !isVersionMap(value.documents)
  ) {
    throw new ChangeProtocolError();
  }
  for (const [schema, version] of Object.entries(CHANGE_READER_DOCUMENTS)) {
    if (value.documents[schema] !== version) {
      throw new ChangeProtocolError(
        `Change reader profile is missing ${schema} v${version}.`,
      );
    }
  }
  if (
    Object.keys(value.documents).length !==
    Object.keys(CHANGE_READER_DOCUMENTS).length
  ) {
    throw new ChangeProtocolError(
      "Change reader profile contains an unrecognized document contract.",
    );
  }
  if (
    value.availability === "ready" &&
    value.minimumReaderProfile !== CHANGE_READER_PROFILE
  ) {
    throw new ChangeProtocolError(
      "Pointbreak minimum reader profile is incompatible.",
    );
  }
  if (
    value.minimumReaderProfile !== undefined &&
    typeof value.minimumReaderProfile !== "string"
  ) {
    throw new ChangeProtocolError();
  }
  if (
    value.commitGraphStamp !== undefined &&
    typeof value.commitGraphStamp !== "string"
  ) {
    throw new ChangeProtocolError();
  }
  return value as unknown as ReaderProfileDoc;
}

export function requireReadyReaderProfile(
  profile: ReaderProfileDoc,
): asserts profile is ReaderProfileDoc & { availability: "ready" } {
  if (profile.availability !== "ready") {
    throw new ChangeReaderUnavailableError(profile.availability);
  }
}

export function validateChangeList(
  value: unknown,
  schema: ChangeListDoc["schema"],
): ChangeListDoc {
  if (
    !isObject(value) ||
    value.schema !== schema ||
    value.version !== 1 ||
    !isNonEmptyString(value.projectionStamp) ||
    !isStringArray(value.diagnostics) ||
    !Array.isArray(value.changes) ||
    !value.changes.every((change) =>
      isChangeSummary(change, value.projectionStamp as string),
    )
  ) {
    throw new ChangeProtocolError();
  }
  return value as unknown as ChangeListDoc;
}

export function validateChangeAttention(
  value: unknown,
  schema: ChangeAttentionDoc["schema"],
  projectionStamp?: string,
): ChangeAttentionDoc {
  if (
    !isObject(value) ||
    value.schema !== schema ||
    value.version !== 2 ||
    !isNonEmptyString(value.projectionStamp) ||
    (projectionStamp !== undefined &&
      value.projectionStamp !== projectionStamp) ||
    !Array.isArray(value.changes) ||
    !value.changes.every((change) =>
      isChangeSummary(change, value.projectionStamp as string),
    )
  ) {
    throw new ChangeProtocolError();
  }
  return value as unknown as ChangeAttentionDoc;
}

export function validateChangeDetail(
  value: unknown,
  changeId: string,
  projectionStamp?: string,
): ChangeDetailDoc {
  if (
    !isObject(value) ||
    value.schema !== "pointbreak.review-change" ||
    value.version !== 1 ||
    !isObject(value.summary) ||
    !isChangeSummary(value.summary, value.projectionStamp as string) ||
    value.summary.changeId !== changeId ||
    !Array.isArray(value.relationClaims) ||
    !Array.isArray(value.currentRevisionRefs) ||
    !value.currentRevisionRefs.every(isRevisionRef) ||
    !isStringArray(value.diagnostics) ||
    !isNonEmptyString(value.projectionStamp) ||
    (projectionStamp !== undefined && value.projectionStamp !== projectionStamp)
  ) {
    throw new ChangeProtocolError();
  }
  return value as unknown as ChangeDetailDoc;
}

export function validateRevisionResource(
  value: unknown,
  revision: RevisionRefV1,
): RevisionResourceDoc {
  if (
    !isObject(value) ||
    value.schema !== "pointbreak.review-revision-resource" ||
    value.version !== 1 ||
    !isObject(value.resource) ||
    !isRevisionRef(value.resource.revision) ||
    !sameRevisionRef(
      value.resource.revision as unknown as RevisionRefV1,
      revision,
    ) ||
    !isNonEmptyString(value.resource.objectId) ||
    !isObject(value.projection) ||
    typeof value.projection.includeBody !== "boolean" ||
    !isResourceAvailability(value.availability) ||
    !isStringArray(value.diagnostics) ||
    !isNonEmptyString(value.cacheKey)
  ) {
    throw new ChangeProtocolError();
  }
  if (
    value.availability === "available" &&
    (!isNonEmptyString(value.capturedDocumentHash) ||
      !isObject(value.capturedDocument) ||
      value.capturedDocument.schema !== "pointbreak.review-revision" ||
      value.capturedDocument.version !== 3 ||
      !isRevisionRef(value.capturedDocument.revisionRef) ||
      !sameRevisionRef(
        value.capturedDocument.revisionRef as unknown as RevisionRefV1,
        revision,
      ))
  ) {
    throw new ChangeProtocolError();
  }
  if (
    value.availability !== "available" &&
    (value.capturedDocument !== undefined ||
      value.capturedDocumentHash !== undefined)
  ) {
    throw new ChangeProtocolError();
  }
  return value as unknown as RevisionResourceDoc;
}

export function validateChangeRevision(
  value: unknown,
  changeId: string,
  revision: RevisionRefV1,
  projectionStamp?: string,
): ChangeRevisionDoc {
  if (
    !isObject(value) ||
    value.schema !== "pointbreak.review-change-revision" ||
    value.version !== 1 ||
    value.changeId !== changeId ||
    !isRevisionRef(value.revision) ||
    !sameRevisionRef(value.revision as unknown as RevisionRefV1, revision) ||
    !Array.isArray(value.membershipSupport) ||
    !isNonEmptyString(value.revisionCurrency) ||
    !isNonEmptyString(value.relationClassification) ||
    !Array.isArray(value.factPresentations) ||
    !value.factPresentations.every((fact) =>
      isFactPresentation(fact, changeId),
    ) ||
    !Array.isArray(value.associations) ||
    !value.associations.every((association) =>
      isExactAssociation(association, revision),
    ) ||
    !isResourceAvailability(value.availability) ||
    !isStringArray(value.diagnostics) ||
    !isNonEmptyString(value.projectionStamp) ||
    (projectionStamp !== undefined && value.projectionStamp !== projectionStamp)
  ) {
    throw new ChangeProtocolError();
  }
  const resource = validateRevisionResource(
    value.exactRevisionDocument,
    revision,
  );
  if (resource.availability !== value.availability) {
    throw new ChangeProtocolError();
  }
  return value as unknown as ChangeRevisionDoc;
}

export function validateRevisionInterdiff(
  value: unknown,
  from: RevisionRefV1,
  to: RevisionRefV1,
): RevisionInterdiffDoc {
  if (
    !isObject(value) ||
    value.schema !== "pointbreak.review-revision-interdiff" ||
    value.version !== 1 ||
    !isObject(value.interdiff) ||
    !isRevisionRef(value.interdiff.from) ||
    !isRevisionRef(value.interdiff.to) ||
    !sameRevisionRef(value.interdiff.from as unknown as RevisionRefV1, from) ||
    !sameRevisionRef(value.interdiff.to as unknown as RevisionRefV1, to) ||
    !isNonEmptyString(value.availability) ||
    !isStringArray(value.diagnostics) ||
    !isNonEmptyString(value.cacheKey)
  ) {
    throw new ChangeProtocolError();
  }
  return value as unknown as RevisionInterdiffDoc;
}

export function sameRevisionRef(
  left: RevisionRefV1,
  right: RevisionRefV1,
): boolean {
  return (
    left.revisionId === right.revisionId &&
    left.objectArtifactContentHash === right.objectArtifactContentHash
  );
}

function isChangeSummary(value: unknown, projectionStamp: string): boolean {
  return (
    isObject(value) &&
    isNonEmptyString(value.changeId) &&
    isNonEmptyString(value.declarationState) &&
    Number.isSafeInteger(value.memberCount) &&
    Number(value.memberCount) >= 0 &&
    Array.isArray(value.currentRevisionRefs) &&
    value.currentRevisionRefs.every(isRevisionRef) &&
    isNonEmptyString(value.topology) &&
    isNonEmptyString(value.lifecycle) &&
    isNonEmptyString(value.attentionSummary) &&
    isNonEmptyString(value.availabilitySummary) &&
    isStringArray(value.diagnostics) &&
    value.projectionStamp === projectionStamp
  );
}

function isRevisionRef(value: unknown): boolean {
  return (
    isObject(value) &&
    isNonEmptyString(value.revisionId) &&
    isNonEmptyString(value.objectArtifactContentHash)
  );
}

function isFactPresentation(value: unknown, changeId: string): boolean {
  return (
    isObject(value) &&
    isNonEmptyString(value.factId) &&
    isNonEmptyString(value.family) &&
    isRevisionRef(value.originRevision) &&
    value.contextChangeId === changeId &&
    (value.presentedInRevision === undefined ||
      isRevisionRef(value.presentedInRevision)) &&
    isNonEmptyString(value.revisionCurrency) &&
    isNonEmptyString(value.familyState) &&
    isNonEmptyString(value.availability)
  );
}

function isExactAssociation(value: unknown, revision: RevisionRefV1): boolean {
  return (
    isObject(value) &&
    isObject(value.comparison) &&
    isRevisionRef(value.comparison.revision) &&
    sameRevisionRef(
      value.comparison.revision as unknown as RevisionRefV1,
      revision,
    )
  );
}

function isResourceAvailability(
  value: unknown,
): value is RevisionResourceDoc["availability"] {
  return (
    value === "available" ||
    value === "removed" ||
    value === "missing" ||
    value === "mismatch" ||
    value === "non_textual"
  );
}

function isAvailability(value: unknown): value is ChangeReaderAvailability {
  return (
    value === "migration_required" ||
    value === "migration_in_progress" ||
    value === "ready"
  );
}

function isVersionMap(value: unknown): value is Record<string, number> {
  return (
    isObject(value) &&
    Object.values(value).every((version) => Number.isSafeInteger(version))
  );
}

function isStringArray(value: unknown): value is string[] {
  return (
    Array.isArray(value) && value.every((entry) => typeof entry === "string")
  );
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
