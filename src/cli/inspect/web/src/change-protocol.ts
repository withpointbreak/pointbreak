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
  relationClaims: Array<{
    claimId: string;
    active: boolean;
    successor: RevisionRef;
    predecessor: RevisionRef;
    supports: Array<{ actorId: string; eventId: string }>;
    withdrawals: Array<{ actorId: string; eventId: string }>;
  }>;
  diagnostics: string[];
  projectionStamp: string;
}

export interface ChangeRevisionDetail {
  schema: "pointbreak.review-change-revision";
  version: 1;
  changeId: string;
  revision: RevisionRef;
  revisionCurrency: string;
  relationClassification: string;
  availability: string;
  factPresentations: Array<{
    factId: string;
    family: string;
    originRevision: RevisionRef;
    revisionCurrency: string;
    familyState: string;
    availability: string;
  }>;
  factContentPresentations?: Record<
    string,
    {
      contentType: "text/plain" | "text/markdown";
      bodyContentState: "present" | "suppressed_present" | "physically_removed";
      content: Record<string, unknown>;
    }
  >;
  associations: Array<{
    state: string;
    proofAvailability: string;
    comparison: { revision: RevisionRef; commitOid: string };
  }>;
  diagnostics: string[];
  projectionStamp: string;
}

export interface RevisionResource {
  schema: "pointbreak.review-revision-resource";
  version: 1;
  resource: { revision: RevisionRef; objectId: string };
  availability: string;
  capturedDocumentHash?: string;
  capturedDocument?: unknown;
  diagnostics: string[];
}

export interface RevisionInterdiff {
  schema: "pointbreak.review-revision-interdiff";
  version: 1;
  interdiff: { from: RevisionRef; to: RevisionRef };
  availability: string;
  comparison?: unknown;
  diagnostics: string[];
}

export function decodeChangeDetail(value: unknown): ChangeDetail {
  const detail = object(value, "Change detail");
  const summary = detail.summary;
  const stamp = detail.projectionStamp;
  const relationClaims = detail.relationClaims;
  const diagnostics = detail.diagnostics;
  if (
    detail.schema !== "pointbreak.review-change" ||
    detail.version !== 1 ||
    !nonEmptyString(stamp) ||
    !isChangeSummary(summary, stamp) ||
    !Array.isArray(relationClaims) ||
    !relationClaims.every(isRelationClaim) ||
    !isStringArray(diagnostics)
  ) {
    throw new Error("invalid Change detail DTO");
  }
  return {
    schema: "pointbreak.review-change",
    version: 1,
    summary,
    relationClaims,
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
    !Array.isArray(factPresentations) ||
    !factPresentations.every(isFactPresentation) ||
    (factContentPresentations !== undefined &&
      !isFactContentPresentations(factContentPresentations)) ||
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
    revisionCurrency,
    relationClassification,
    availability,
    factPresentations,
    factContentPresentations,
    associations,
    diagnostics,
    projectionStamp: detail.projectionStamp,
  };
}

export function decodeRevisionResource(value: unknown): RevisionResource {
  const document = object(value, "Revision resource");
  const resource = document.resource;
  const diagnostics = document.diagnostics;
  const availability = document.availability;
  const capturedDocumentHash = document.capturedDocumentHash;
  if (
    document.schema !== "pointbreak.review-revision-resource" ||
    document.version !== 1 ||
    !isRecord(resource) ||
    !isRevisionRef(resource.revision) ||
    !nonEmptyString(resource.objectId) ||
    !isOneOf(availability, CONTENT_AVAILABILITY_VALUES) ||
    (capturedDocumentHash !== undefined &&
      !nonEmptyString(capturedDocumentHash)) ||
    (availability === "available") !==
      (capturedDocumentHash !== undefined &&
        document.capturedDocument !== undefined) ||
    !isStringArray(diagnostics)
  ) {
    throw new Error("invalid Revision resource DTO");
  }
  return {
    schema: "pointbreak.review-revision-resource",
    version: 1,
    resource: { revision: resource.revision, objectId: resource.objectId },
    availability,
    capturedDocumentHash,
    capturedDocument: document.capturedDocument,
    diagnostics,
  };
}

export function decodeRevisionInterdiff(value: unknown): RevisionInterdiff {
  const document = object(value, "Revision interdiff");
  const interdiff = document.interdiff;
  const diagnostics = document.diagnostics;
  const availability = document.availability;
  if (
    document.schema !== "pointbreak.review-revision-interdiff" ||
    document.version !== 1 ||
    !isRecord(interdiff) ||
    !isRevisionRef(interdiff.from) ||
    !isRevisionRef(interdiff.to) ||
    !isOneOf(availability, INTERDIFF_AVAILABILITY_VALUES) ||
    !isStringArray(diagnostics) ||
    (availability === "available") !== (document.comparison !== undefined)
  ) {
    throw new Error("invalid Revision interdiff DTO");
  }
  return {
    schema: "pointbreak.review-revision-interdiff",
    version: 1,
    interdiff: { from: interdiff.from, to: interdiff.to },
    availability,
    comparison: document.comparison,
    diagnostics,
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

function isClaimSupport(
  value: unknown,
): value is { actorId: string; eventId: string } {
  return (
    isRecord(value) &&
    nonEmptyString(value.actorId) &&
    nonEmptyString(value.eventId)
  );
}

function isRelationClaim(
  value: unknown,
): value is ChangeDetail["relationClaims"][number] {
  return (
    isRecord(value) &&
    nonEmptyString(value.claimId) &&
    typeof value.active === "boolean" &&
    isRevisionRef(value.successor) &&
    isRevisionRef(value.predecessor) &&
    Array.isArray(value.supports) &&
    value.supports.every(isClaimSupport) &&
    Array.isArray(value.withdrawals) &&
    value.withdrawals.every(isClaimSupport)
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
    isOneOf(value.revisionCurrency, REVISION_CURRENCY_VALUES) &&
    isOneOf(value.familyState, FACT_FAMILY_STATE_VALUES) &&
    isOneOf(value.availability, CONTENT_AVAILABILITY_VALUES)
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
        isRecord(presentation.content),
    )
  );
}

function isAssociation(
  value: unknown,
): value is ChangeRevisionDetail["associations"][number] {
  return (
    isRecord(value) &&
    isOneOf(value.state, ASSOCIATION_STATE_VALUES) &&
    isOneOf(value.proofAvailability, ASSOCIATION_PROOF_VALUES) &&
    isRecord(value.comparison) &&
    isRevisionRef(value.comparison.revision) &&
    nonEmptyString(value.comparison.commitOid)
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

function isStringArray(value: unknown): value is string[] {
  return (
    Array.isArray(value) && value.every((item) => typeof item === "string")
  );
}
