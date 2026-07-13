import { createHash } from "node:crypto";
import type { Memento, SecretStorage } from "vscode";

const CONNECTION_STATE_KEY = "pointbreak.inspectConnection";
const CONNECTION_STATE_VERSION = 1;
const SECRET_KEY_PREFIX = "pointbreak.inspectConnection.v1";

export interface InspectConnectionRecord {
  readonly targetKey: string;
  readonly host: string;
  readonly port: number;
  readonly storeIdentity: string;
  readonly contextIdentity: string;
}

interface ConnectionState extends InspectConnectionRecord {
  readonly version: typeof CONNECTION_STATE_VERSION;
}

export interface StoredInspectConnection {
  readonly record: InspectConnectionRecord;
  readonly token: string;
}

/** Stores one lazy reconnect candidate without placing its bearer in Memento. */
export class InspectConnectionStore {
  private value: unknown;
  private pendingWrite: Promise<void> = Promise.resolve();

  constructor(
    private readonly state: Memento,
    private readonly secrets: SecretStorage,
  ) {
    this.value = state.get<unknown>(CONNECTION_STATE_KEY);
  }

  async load(targetKey: string): Promise<StoredInspectConnection | undefined> {
    const document = readConnectionState(this.value);
    if (!document) {
      if (this.value !== undefined) {
        await this.clearInvalidState();
      }
      return undefined;
    }
    if (document.targetKey !== targetKey) {
      return undefined;
    }

    const token = await this.secrets.get(secretKey(targetKey));
    if (!token) {
      await this.forget(targetKey);
      return undefined;
    }
    return { record: connectionRecord(document), token };
  }

  async remember(
    record: InspectConnectionRecord,
    token: string,
  ): Promise<void> {
    if (!isConnectionRecord(record) || !token) {
      throw new Error("invalid Pointbreak inspect connection");
    }
    const previous = readConnectionState(this.value);
    const key = secretKey(record.targetKey);
    await this.secrets.store(key, token);
    const document: ConnectionState = {
      version: CONNECTION_STATE_VERSION,
      ...record,
    };
    try {
      await this.updateState(document);
    } catch (error) {
      await this.secrets.delete(key).then(undefined, () => undefined);
      throw error;
    }
    if (previous && previous.targetKey !== record.targetKey) {
      await this.secrets
        .delete(secretKey(previous.targetKey))
        .then(undefined, () => undefined);
    }
  }

  async forget(targetKey: string): Promise<void> {
    const document = readConnectionState(this.value);
    if (document?.targetKey === targetKey) {
      await this.updateState(undefined);
    }
    await this.secrets.delete(secretKey(targetKey));
  }

  private async clearInvalidState(): Promise<void> {
    const targetKey = invalidTargetKey(this.value);
    await this.updateState(undefined);
    if (targetKey) {
      await this.secrets.delete(secretKey(targetKey));
    }
  }

  private updateState(value: ConnectionState | undefined): Promise<void> {
    const write = this.pendingWrite
      .catch(() => undefined)
      .then(() => this.state.update(CONNECTION_STATE_KEY, value))
      .then(() => {
        this.value = value;
      });
    this.pendingWrite = write;
    return write;
  }
}

function readConnectionState(value: unknown): ConnectionState | undefined {
  if (!isObject(value) || value.version !== CONNECTION_STATE_VERSION) {
    return undefined;
  }
  const allowed = new Set([
    "version",
    "targetKey",
    "host",
    "port",
    "storeIdentity",
    "contextIdentity",
  ]);
  if (Object.keys(value).some((key) => !allowed.has(key))) {
    return undefined;
  }
  if (!isConnectionRecord(value)) {
    return undefined;
  }
  return {
    version: CONNECTION_STATE_VERSION,
    targetKey: value.targetKey,
    host: value.host,
    port: value.port,
    storeIdentity: value.storeIdentity,
    contextIdentity: value.contextIdentity,
  };
}

function connectionRecord(state: ConnectionState): InspectConnectionRecord {
  return {
    targetKey: state.targetKey,
    host: state.host,
    port: state.port,
    storeIdentity: state.storeIdentity,
    contextIdentity: state.contextIdentity,
  };
}

function isConnectionRecord(value: unknown): value is InspectConnectionRecord {
  return (
    isObject(value) &&
    typeof value.targetKey === "string" &&
    value.targetKey.length > 0 &&
    typeof value.host === "string" &&
    value.host.length > 0 &&
    typeof value.port === "number" &&
    Number.isInteger(value.port) &&
    value.port > 0 &&
    value.port <= 65_535 &&
    typeof value.storeIdentity === "string" &&
    value.storeIdentity.length > 0 &&
    typeof value.contextIdentity === "string" &&
    value.contextIdentity.length > 0
  );
}

function invalidTargetKey(value: unknown): string | undefined {
  return isObject(value) && typeof value.targetKey === "string"
    ? value.targetKey
    : undefined;
}

function secretKey(targetKey: string): string {
  const digest = createHash("sha256").update(targetKey).digest("hex");
  return `${SECRET_KEY_PREFIX}.${digest}`;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
