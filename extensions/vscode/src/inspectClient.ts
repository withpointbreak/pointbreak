import http from "node:http";
import https from "node:https";
import {
  type ChangeDetailDoc,
  ChangeProtocolError,
  ChangeReaderUnavailableError,
  type ChangeRevisionDoc,
  type ReaderProfileDoc,
  type RevisionRefV1,
  requireReadyReaderProfile,
  validateChangeDetail,
  validateChangeRevision,
  validateReaderProfile,
} from "./changeProtocol";
import {
  type InspectFreshnessDoc,
  type ReviewSnapshotDoc,
  type RevisionDoc,
  type VersionDoc,
  verifyHandshake,
} from "./cli";

const REQUEST_TIMEOUT_MS = 1_000;
const MAX_RESPONSE_BYTES = 1024 * 1024;

export type FetchFn = (
  url: URL,
  init: { headers: Record<string, string>; signal: AbortSignal },
) => Promise<{ status: number; text(): Promise<string> }>;

export interface InspectIdentity {
  readonly storeIdentity: string;
  readonly contextIdentity: string;
}

interface InspectIdentityDoc extends InspectIdentity {
  readonly schema: "pointbreak.inspect-identity";
}

export type InspectClientErrorKind =
  | "unauthorized"
  | "unreachable"
  | "protocol"
  | "version-incompatible"
  | "identity-mismatch"
  | "migration-required"
  | "migration-in-progress";

/** Checks whether an exact inspector revision is still a writable thread head. */
export function revisionIsCurrent(
  document: RevisionDoc,
  revisionId: string,
): boolean {
  if (document.revision.id !== revisionId) return false;
  const supersession = document.revisionSupersession;
  if (supersession === undefined) return true;
  if (!isObject(supersession) || !Array.isArray(supersession.heads)) {
    return false;
  }
  return (
    supersession.heads.every((head) => typeof head === "string") &&
    supersession.heads.includes(revisionId)
  );
}

export class InspectClientError extends Error {
  constructor(readonly kind: InspectClientErrorKind) {
    super(errorMessage(kind));
    this.name = "InspectClientError";
  }
}

/** Credential-holding request boundary for authenticated inspect handshakes. */
export class InspectClient {
  readonly #origin: URL;
  readonly #token: string;
  readonly #fetch: FetchFn;
  readonly #timeoutMs: number;
  #versionVerification?: Promise<VersionDoc>;
  #profileVerification?: Promise<ReaderProfileDoc>;

  constructor(
    origin: string,
    token: string,
    fetch: FetchFn = defaultFetch,
    timeoutMs = REQUEST_TIMEOUT_MS,
  ) {
    const parsed = new URL(origin);
    if (
      (parsed.protocol !== "http:" && parsed.protocol !== "https:") ||
      parsed.username ||
      parsed.password ||
      parsed.pathname !== "/" ||
      parsed.search ||
      parsed.hash
    ) {
      throw new InspectClientError("protocol");
    }
    this.#origin = new URL(parsed.origin);
    this.#token = token;
    this.#fetch = fetch;
    this.#timeoutMs = timeoutMs;
  }

  async verify(identity: InspectIdentity): Promise<void> {
    await this.profile();
    await this.verifyVersion();
    await this.verifyIdentity(identity);
  }

  async profile(): Promise<ReaderProfileDoc> {
    this.#profileVerification ??= this.readProfile();
    return this.#profileVerification;
  }

  async verifyVersion(): Promise<VersionDoc> {
    this.#versionVerification ??= this.readVersion();
    return this.#versionVerification;
  }

  async revision(revisionId: string): Promise<RevisionDoc> {
    await this.verifyVersion();
    const document = await this.document(
      resourceUrl(this.#origin, "revisions", revisionId),
    );
    if (!isRevisionDocument(document) || document.revision.id !== revisionId) {
      throw new InspectClientError("protocol");
    }
    return document;
  }

  async snapshot(
    snapshotId: string,
    contentHash?: string,
  ): Promise<ReviewSnapshotDoc> {
    await this.verifyVersion();
    const url = resourceUrl(this.#origin, "snapshots", snapshotId);
    if (contentHash) {
      url.searchParams.set("contentHash", contentHash);
    }
    const document = await this.document(url);
    if (
      !isReviewSnapshotDocument(document) ||
      document.snapshot.object_id !== snapshotId ||
      (contentHash !== undefined && document.contentHash !== contentHash)
    ) {
      throw new InspectClientError("protocol");
    }
    return document;
  }

  async freshness(): Promise<InspectFreshnessDoc> {
    const profile = await this.readProfile();
    const eventCount = profile.authorityCursor.eventCount;
    if (!Number.isSafeInteger(eventCount) || Number(eventCount) < 0) {
      throw new InspectClientError("protocol");
    }
    return {
      schema: "pointbreak.inspect-freshness",
      version: 1,
      eventCount: Number(eventCount),
      commitGraphStamp: profile.commitGraphStamp,
    };
  }

  async changeRevision(
    changeId: string,
    revision: RevisionRefV1,
    projectionStamp?: string,
  ): Promise<ChangeRevisionDoc> {
    try {
      requireReadyReaderProfile(await this.profile());
      const url = changeRevisionUrl(this.#origin, changeId, revision);
      return validateChangeRevision(
        await this.document(url),
        changeId,
        revision,
        projectionStamp,
      );
    } catch (error) {
      throw inspectProtocolError(error);
    }
  }

  async changeDetail(
    changeId: string,
    projectionStamp: string,
  ): Promise<ChangeDetailDoc> {
    try {
      requireReadyReaderProfile(await this.profile());
      if (!changeId || !projectionStamp) {
        throw new InspectClientError("protocol");
      }
      const document = await this.document(
        new URL(
          `/api/v2/changes/${encodeURIComponent(changeId)}`,
          this.#origin,
        ),
      );
      return validateChangeDetail(document, changeId, projectionStamp);
    } catch (error) {
      throw inspectProtocolError(error);
    }
  }

  private async readProfile(): Promise<ReaderProfileDoc> {
    try {
      return validateReaderProfile(
        await this.document(new URL("/api/v2/profile", this.#origin)),
      );
    } catch (error) {
      if (error instanceof InspectClientError) throw error;
      throw new InspectClientError("version-incompatible");
    }
  }

  private async readVersion(): Promise<VersionDoc> {
    const document = await this.document(new URL("/api/version", this.#origin));
    if (!isVersionDocument(document)) {
      throw new InspectClientError("protocol");
    }
    const result = verifyHandshake(document);
    if (!result.ok) {
      throw new InspectClientError("version-incompatible");
    }
    return document;
  }

  async verifyIdentity(expected: InspectIdentity): Promise<InspectIdentity> {
    await this.verifyVersion();
    const document = await this.document(
      new URL("/api/identity", this.#origin),
    );
    if (!isInspectIdentityDocument(document)) {
      throw new InspectClientError("protocol");
    }
    if (
      document.storeIdentity !== expected.storeIdentity ||
      document.contextIdentity !== expected.contextIdentity
    ) {
      throw new InspectClientError("identity-mismatch");
    }
    return {
      storeIdentity: document.storeIdentity,
      contextIdentity: document.contextIdentity,
    };
  }

  private async document(url: URL): Promise<unknown> {
    const controller = new AbortController();
    let rejectTimeout: ((reason: InspectClientError) => void) | undefined;
    const timedOut = new Promise<never>((_resolve, reject) => {
      rejectTimeout = reject;
    });
    const timeout = setTimeout(() => {
      controller.abort();
      rejectTimeout?.(new InspectClientError("unreachable"));
    }, this.#timeoutMs);
    try {
      const response = await Promise.race([
        this.#fetch(url, {
          headers: {
            Host: this.#origin.host,
            Authorization: `Bearer ${this.#token}`,
          },
          signal: controller.signal,
        }),
        timedOut,
      ]);

      if (response.status === 401) {
        throw new InspectClientError("unauthorized");
      }
      if (response.status < 200 || response.status >= 300) {
        throw new InspectClientError("protocol");
      }
      const body = await Promise.race([response.text(), timedOut]);
      if (Buffer.byteLength(body) > MAX_RESPONSE_BYTES) {
        throw new InspectClientError("protocol");
      }
      return JSON.parse(body);
    } catch (error) {
      if (error instanceof InspectClientError) {
        throw error;
      }
      if (error instanceof SyntaxError) {
        throw new InspectClientError("protocol");
      }
      throw new InspectClientError("unreachable");
    } finally {
      clearTimeout(timeout);
    }
  }
}

function resourceUrl(origin: URL, collection: string, id: string): URL {
  if (!id) {
    throw new InspectClientError("protocol");
  }
  return new URL(`/api/${collection}/${encodeURIComponent(id)}`, origin);
}

function changeRevisionUrl(
  origin: URL,
  changeId: string,
  revision: RevisionRefV1,
): URL {
  if (
    !changeId ||
    !revision.revisionId ||
    !revision.objectArtifactContentHash
  ) {
    throw new InspectClientError("protocol");
  }
  const url = new URL(
    `/api/v2/changes/${encodeURIComponent(changeId)}/revisions/${encodeURIComponent(revision.revisionId)}`,
    origin,
  );
  url.searchParams.set("artifactHash", revision.objectArtifactContentHash);
  return url;
}

function inspectProtocolError(error: unknown): InspectClientError {
  if (error instanceof InspectClientError) return error;
  if (error instanceof ChangeReaderUnavailableError) {
    return new InspectClientError(
      error.availability === "migration_required"
        ? "migration-required"
        : "migration-in-progress",
    );
  }
  if (error instanceof ChangeProtocolError) {
    return new InspectClientError("protocol");
  }
  return new InspectClientError("protocol");
}

function isVersionDocument(value: unknown): value is VersionDoc {
  return (
    isObject(value) &&
    value.schema === "pointbreak.version" &&
    value.version === 1 &&
    typeof value.cliVersion === "string" &&
    isObject(value.documents)
  );
}

function isRevisionDocument(value: unknown): value is RevisionDoc {
  return (
    isObject(value) &&
    value.schema === "pointbreak.review-revision" &&
    value.version === 2 &&
    isObject(value.revision) &&
    typeof value.revision.id === "string" &&
    isOptionalString(value.revision.objectId) &&
    isOptionalString(value.revision.objectArtifactContentHash) &&
    isObjectArray(value.observations) &&
    isObjectArray(value.inputRequests) &&
    isObjectArray(value.assessments)
  );
}

function isInspectIdentityDocument(
  value: unknown,
): value is InspectIdentityDoc {
  return (
    isObject(value) &&
    value.schema === "pointbreak.inspect-identity" &&
    typeof value.storeIdentity === "string" &&
    typeof value.contextIdentity === "string"
  );
}

function isReviewSnapshotDocument(value: unknown): value is ReviewSnapshotDoc {
  return (
    isObject(value) &&
    value.schema === "pointbreak.review-snapshot" &&
    value.version === 1 &&
    typeof value.contentHash === "string" &&
    isObject(value.snapshot) &&
    typeof value.snapshot.review_id === "string" &&
    typeof value.snapshot.object_id === "string" &&
    Array.isArray(value.snapshot.files) &&
    value.snapshot.files.every(isReviewSnapshotFile)
  );
}

function isReviewSnapshotFile(value: unknown): boolean {
  return (
    isObject(value) &&
    typeof value.id === "string" &&
    Array.isArray(value.hunks) &&
    value.hunks.every(isReviewSnapshotHunk)
  );
}

function isReviewSnapshotHunk(value: unknown): boolean {
  return (
    isObject(value) &&
    typeof value.id === "string" &&
    typeof value.header === "string" &&
    Array.isArray(value.rows) &&
    value.rows.every(isReviewSnapshotRow)
  );
}

function isReviewSnapshotRow(value: unknown): boolean {
  return (
    isObject(value) &&
    typeof value.kind === "string" &&
    isLine(value.old_line) &&
    isLine(value.new_line) &&
    typeof value.text === "string" &&
    (value.tokens === undefined || Array.isArray(value.tokens)) &&
    (value.emphasis === undefined || Array.isArray(value.emphasis))
  );
}

function isObjectArray(value: unknown): value is Record<string, unknown>[] {
  return Array.isArray(value) && value.every(isObject);
}

function isOptionalString(value: unknown): boolean {
  return value === undefined || typeof value === "string";
}

function isLine(value: unknown): boolean {
  return value === null || (Number.isSafeInteger(value) && Number(value) >= 0);
}

function errorMessage(kind: InspectClientErrorKind): string {
  switch (kind) {
    case "unauthorized":
      return "Pointbreak Review authentication was rejected.";
    case "unreachable":
      return "Pointbreak Review could not be reached.";
    case "protocol":
      return "Pointbreak Review returned an invalid response.";
    case "version-incompatible":
      return "Pointbreak Review is incompatible with this extension.";
    case "identity-mismatch":
      return "Pointbreak Review belongs to another review target.";
    case "migration-required":
      return "Pointbreak store migration is required before Change state can be read.";
    case "migration-in-progress":
      return "Pointbreak store migration is in progress; partial Change state is unavailable.";
  }
}

const defaultFetch: FetchFn = (url, init) =>
  new Promise((resolve, reject) => {
    const transport = url.protocol === "https:" ? https : http;
    const request = transport.request(
      url,
      {
        method: "GET",
        headers: init.headers,
        signal: init.signal,
      },
      (response) => {
        const chunks: Buffer[] = [];
        let bytes = 0;
        response.on("data", (chunk: Buffer) => {
          bytes += chunk.length;
          if (bytes > MAX_RESPONSE_BYTES) {
            request.destroy(new Error("response too large"));
            return;
          }
          chunks.push(chunk);
        });
        response.once("end", () => {
          const body = Buffer.concat(chunks).toString("utf8");
          resolve({
            status: response.statusCode ?? 0,
            text: async () => body,
          });
        });
      },
    );
    request.once("error", reject);
    request.end();
  });

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
