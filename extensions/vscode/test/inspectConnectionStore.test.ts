import { describe, expect, it, vi } from "vitest";
import type { Memento, SecretStorage } from "vscode";
import {
  type InspectConnectionRecord,
  InspectConnectionStore,
} from "../src/inspectConnectionStore";

describe("InspectConnectionStore", () => {
  it("keeps only opaque metadata in workspace state and the bearer in SecretStorage", async () => {
    const state = memento();
    const secrets = secretStorage();
    const store = new InspectConnectionStore(state, secrets);

    await store.remember(record(), "secret-bearer");

    expect(state.update).toHaveBeenCalledWith("pointbreak.inspectConnection", {
      version: 1,
      ...record(),
    });
    const persisted = JSON.stringify(state.update.mock.calls[0]?.[1]);
    expect(persisted).not.toContain("secret-bearer");
    expect(persisted).not.toContain("/repo");
    expect(persisted).not.toContain("file:");
    expect(secrets.store).toHaveBeenCalledWith(
      expect.stringMatching(
        /^pointbreak\.inspectConnection\.v1\.[a-f0-9]{64}$/,
      ),
      "secret-bearer",
    );
  });

  it("loads only the exact target candidate with its separately stored bearer", async () => {
    const state = memento({ version: 1, ...record() });
    const secrets = secretStorage("secret-bearer");
    const store = new InspectConnectionStore(state, secrets);

    await expect(store.load(record().targetKey)).resolves.toEqual({
      record: record(),
      token: "secret-bearer",
    });
    await expect(store.load("store/context-other")).resolves.toBeUndefined();
    expect(secrets.get).toHaveBeenCalledOnce();
  });

  it("clears invalid metadata and candidates whose bearer is missing", async () => {
    const state = memento({
      version: 1,
      ...record(),
      port: 0,
      folderUri: "file:///repo",
    });
    const store = new InspectConnectionStore(state, secretStorage());

    await expect(store.load(record().targetKey)).resolves.toBeUndefined();
    expect(state.update).toHaveBeenCalledWith(
      "pointbreak.inspectConnection",
      undefined,
    );

    const missingSecretState = memento({ version: 1, ...record() });
    const missingSecretStore = new InspectConnectionStore(
      missingSecretState,
      secretStorage(),
    );
    await expect(
      missingSecretStore.load(record().targetKey),
    ).resolves.toBeUndefined();
    expect(missingSecretState.update).toHaveBeenCalledWith(
      "pointbreak.inspectConnection",
      undefined,
    );
  });
});

function record(): InspectConnectionRecord {
  return {
    targetKey: "store/context",
    host: "127.0.0.1",
    port: 63831,
    storeIdentity: "store",
    contextIdentity: "context",
  };
}

function memento(
  value?: unknown,
): Memento & { update: ReturnType<typeof vi.fn> } {
  return {
    keys: () => ["pointbreak.inspectConnection"],
    get: vi.fn(() => value) as Memento["get"],
    update: vi.fn(async () => undefined),
  };
}

function secretStorage(value?: string): SecretStorage & {
  get: ReturnType<typeof vi.fn>;
  store: ReturnType<typeof vi.fn>;
  delete: ReturnType<typeof vi.fn>;
} {
  return {
    get: vi.fn(async () => value),
    store: vi.fn(async () => undefined),
    delete: vi.fn(async () => undefined),
    onDidChange: vi.fn(),
  } as unknown as SecretStorage & {
    get: ReturnType<typeof vi.fn>;
    store: ReturnType<typeof vi.fn>;
    delete: ReturnType<typeof vi.fn>;
  };
}
