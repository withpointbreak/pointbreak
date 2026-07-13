import type { Memento } from "vscode";

const REVIEW_SERVERS_STATE_KEY = "pointbreak.reviewServers";
const REVIEW_SERVERS_STATE_VERSION = 1;

export interface ReviewServerRecord {
  targetKey: string;
  storeIdentity: string;
  contextIdentity: string;
  folderUri: string;
  port: number;
}

interface ReviewServersState {
  version: typeof REVIEW_SERVERS_STATE_VERSION;
  servers: ReviewServerRecord[];
}

/** Workspace-scoped last-known ports for extension-managed Review servers. */
export class ReviewServerRegistry {
  private readonly servers: Map<string, ReviewServerRecord>;
  private pendingWrite: Promise<void> = Promise.resolve();

  constructor(private readonly state: Memento) {
    this.servers = new Map(
      readState(state.get<unknown>(REVIEW_SERVERS_STATE_KEY)).map((record) => [
        record.targetKey,
        record,
      ]),
    );
  }

  get(targetKey: string): ReviewServerRecord | undefined {
    return this.servers.get(targetKey);
  }

  entries(): ReviewServerRecord[] {
    return [...this.servers.values()];
  }

  remember(record: ReviewServerRecord): Promise<void> {
    if (!isReviewServerRecord(record)) {
      return Promise.reject(
        new Error("invalid Pointbreak Review server state"),
      );
    }
    this.servers.set(record.targetKey, record);
    const document: ReviewServersState = {
      version: REVIEW_SERVERS_STATE_VERSION,
      servers: this.entries(),
    };
    const write = this.pendingWrite
      .catch(() => undefined)
      .then(() => this.state.update(REVIEW_SERVERS_STATE_KEY, document));
    this.pendingWrite = write;
    return write;
  }
}

export function reviewServerUrl(record: ReviewServerRecord): string {
  return `http://127.0.0.1:${record.port}`;
}

function readState(value: unknown): ReviewServerRecord[] {
  if (
    !isObject(value) ||
    value.version !== REVIEW_SERVERS_STATE_VERSION ||
    !Array.isArray(value.servers)
  ) {
    return [];
  }
  return value.servers.filter(isReviewServerRecord);
}

function isReviewServerRecord(value: unknown): value is ReviewServerRecord {
  return (
    isObject(value) &&
    typeof value.targetKey === "string" &&
    typeof value.storeIdentity === "string" &&
    typeof value.contextIdentity === "string" &&
    typeof value.folderUri === "string" &&
    typeof value.port === "number" &&
    Number.isInteger(value.port) &&
    value.port > 0 &&
    value.port <= 65_535
  );
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
