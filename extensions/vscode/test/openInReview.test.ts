import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Memento, WorkspaceFolder } from "vscode";
import type { ResolvedBinary } from "../src/binary";
import type { PointbreakCli } from "../src/cli";
import {
  probeReview,
  type ReviewIdentity,
  type ReviewProbeResult,
  restoreReviewServers,
  reviewDeepLink,
  runOpenInReviewCommand,
} from "../src/commands/openInReview";
import {
  type ReviewServerRecord,
  ReviewServerRegistry,
} from "../src/reviewServerRegistry";
import type { TargetResolution } from "../src/targetResolver";
import { workspaceFolder } from "./helpers/vscodeMock";

const vscodeMocks = vi.hoisted(() => ({
  executeCommand: vi.fn(),
  openLocalhostLinks: false,
  openExternal: vi.fn(),
  reviewUrl: "",
  remoteName: undefined as string | undefined,
  showErrorMessage: vi.fn(),
  showInformationMessage: vi.fn(),
  showQuickPick: vi.fn(),
}));

vi.mock("vscode", () => ({
  commands: { executeCommand: vscodeMocks.executeCommand },
  Uri: { parse: (value: string) => value },
  env: {
    get remoteName() {
      return vscodeMocks.remoteName;
    },
    openExternal: vscodeMocks.openExternal,
  },
  window: {
    showErrorMessage: vscodeMocks.showErrorMessage,
    showInformationMessage: vscodeMocks.showInformationMessage,
    showQuickPick: vscodeMocks.showQuickPick,
  },
  workspace: {
    getConfiguration: (section: string) => ({
      get: (_key: string, fallback: unknown) => {
        if (section === "workbench.browser") {
          return vscodeMocks.openLocalhostLinks;
        }
        if (section === "pointbreak") {
          return vscodeMocks.reviewUrl;
        }
        return fallback;
      },
    }),
  },
}));

const EXPECTED_IDENTITY: ReviewIdentity = {
  storeIdentity: "store:sha256:store",
  contextIdentity: "context:sha256:repo",
};
const MATCH: ReviewProbeResult = { kind: "match" };
const UNAVAILABLE: ReviewProbeResult = { kind: "unavailable" };
const INCOMPATIBLE: ReviewProbeResult = { kind: "incompatible" };
const MISMATCH: ReviewProbeResult = { kind: "mismatch" };

beforeEach(() => {
  vscodeMocks.executeCommand.mockReset();
  vscodeMocks.openLocalhostLinks = false;
  vscodeMocks.openExternal.mockReset();
  vscodeMocks.reviewUrl = "";
  vscodeMocks.remoteName = undefined;
  vscodeMocks.showErrorMessage.mockReset();
  vscodeMocks.showInformationMessage.mockReset();
  vscodeMocks.showQuickPick.mockReset();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("reviewDeepLink", () => {
  it("builds the revision deep link exactly", () => {
    expect(reviewDeepLink("http://127.0.0.1:7878", "rev:sha256:abc")).toBe(
      "http://127.0.0.1:7878/#/revision/rev:sha256:abc",
    );
  });

  it("preserves the attention lens for an attention-item link", () => {
    expect(
      reviewDeepLink("http://127.0.0.1:7878", "rev:sha256:abc", "attention"),
    ).toBe("http://127.0.0.1:7878/#/revision/rev:sha256:abc?lens=attention");
  });
});

describe("probeReview", () => {
  it("accepts only an inspector serving the expected store and context", async () => {
    const fetch = vi.fn(async () => ({
      ok: true,
      json: async () => ({
        schema: "pointbreak.inspect-identity",
        ...EXPECTED_IDENTITY,
      }),
    }));
    vi.stubGlobal("fetch", fetch);

    await expect(
      probeReview("http://127.0.0.1:63831", EXPECTED_IDENTITY),
    ).resolves.toEqual(MATCH);
    expect(fetch).toHaveBeenCalledWith(
      "http://127.0.0.1:63831/api/identity",
      expect.objectContaining({ method: "GET" }),
    );
  });

  it("rejects a reachable inspector for a different context", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({
        ok: true,
        json: async () => ({
          schema: "pointbreak.inspect-identity",
          storeIdentity: EXPECTED_IDENTITY.storeIdentity,
          contextIdentity: "context:sha256:other",
        }),
      })),
    );

    await expect(
      probeReview("http://127.0.0.1:7878", EXPECTED_IDENTITY),
    ).resolves.toEqual(MISMATCH);
  });

  it("identifies an older inspector that omits repository identities", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({
        ok: true,
        json: async () => ({
          schema: "pointbreak.inspect-identity",
          repository: "/repo",
        }),
      })),
    );

    await expect(
      probeReview("http://127.0.0.1:7878", EXPECTED_IDENTITY),
    ).resolves.toEqual(INCOMPATIBLE);
  });
});

describe("runOpenInReviewCommand", () => {
  it("opens externally when the configured endpoint has the expected identity", async () => {
    const probe = vi.fn(async () => MATCH);
    const start = vi.fn();

    await runOpenInReviewCommand(cli(), binary(), [resolved()], reviewNode(), {
      probe,
      reviewUrl: "http://127.0.0.1:7878",
      running: () => undefined,
      start,
    });

    expect(probe).toHaveBeenCalledWith(
      "http://127.0.0.1:7878",
      EXPECTED_IDENTITY,
    );
    expect(vscodeMocks.openExternal).toHaveBeenCalledWith(
      "http://127.0.0.1:7878/#/revision/rev:sha256:abc",
    );
    expect(start).not.toHaveBeenCalled();
  });

  it("uses the integrated browser when the localhost preference is enabled", async () => {
    vscodeMocks.openLocalhostLinks = true;

    await runOpenInReviewCommand(cli(), binary(), [resolved()], reviewNode(), {
      probe: vi.fn(async () => MATCH),
      reviewUrl: "http://127.0.0.1:7878",
      running: () => undefined,
    });

    expect(vscodeMocks.executeCommand).toHaveBeenCalledWith(
      "workbench.action.browser.open",
      "http://127.0.0.1:7878/#/revision/rev:sha256:abc",
    );
    expect(vscodeMocks.openExternal).not.toHaveBeenCalled();
  });

  it("falls back externally when the integrated browser command is unavailable", async () => {
    vscodeMocks.openLocalhostLinks = true;
    vscodeMocks.executeCommand.mockRejectedValue(
      new Error("command not found"),
    );

    await runOpenInReviewCommand(cli(), binary(), [resolved()], reviewNode(), {
      probe: vi.fn(async () => MATCH),
      reviewUrl: "http://127.0.0.1:7878",
      running: () => undefined,
    });

    expect(vscodeMocks.openExternal).toHaveBeenCalledWith(
      "http://127.0.0.1:7878/#/revision/rev:sha256:abc",
    );
  });

  it("keeps configured non-local Review servers in the external browser", async () => {
    vscodeMocks.openLocalhostLinks = true;

    await runOpenInReviewCommand(cli(), binary(), [resolved()], reviewNode(), {
      probe: vi.fn(async () => MATCH),
      reviewUrl: "https://review.example.com",
      running: () => undefined,
    });

    expect(vscodeMocks.executeCommand).not.toHaveBeenCalled();
    expect(vscodeMocks.openExternal).toHaveBeenCalledWith(
      "https://review.example.com/#/revision/rev:sha256:abc",
    );
  });

  it("reuses an extension-started ephemeral server without prompting", async () => {
    const probe = vi.fn(async () => MATCH);

    await runOpenInReviewCommand(cli(), binary(), [resolved()], reviewNode(), {
      probe,
      running: () => Promise.resolve("http://127.0.0.1:63831/"),
    });

    expect(probe).toHaveBeenCalledOnce();
    expect(probe).toHaveBeenCalledWith(
      "http://127.0.0.1:63831",
      EXPECTED_IDENTITY,
    );
    expect(vscodeMocks.showInformationMessage).not.toHaveBeenCalled();
    expect(vscodeMocks.openExternal).toHaveBeenCalledWith(
      "http://127.0.0.1:63831/#/revision/rev:sha256:abc",
    );
  });

  it("reuses a remembered workspace port when its identity still matches", async () => {
    const { registry } = registryWith([rememberedRecord()]);
    const probe = vi.fn(async () => MATCH);

    await runOpenInReviewCommand(cli(), binary(), [resolved()], reviewNode(), {
      probe,
      registry,
      running: () => undefined,
    });

    expect(probe).toHaveBeenCalledWith(
      "http://127.0.0.1:63831",
      EXPECTED_IDENTITY,
    );
    expect(vscodeMocks.showInformationMessage).not.toHaveBeenCalled();
    expect(vscodeMocks.openExternal).toHaveBeenCalledWith(
      "http://127.0.0.1:63831/#/revision/rev:sha256:abc",
    );
  });

  it("restarts a missing remembered server on the same port", async () => {
    vscodeMocks.showInformationMessage.mockResolvedValue(
      "Start `shore inspect` here",
    );
    const { registry, update } = registryWith([rememberedRecord()]);
    const probe = vi
      .fn<
        (
          baseUrl: string,
          identity: ReviewIdentity,
        ) => Promise<ReviewProbeResult>
      >()
      .mockResolvedValueOnce(UNAVAILABLE)
      .mockResolvedValueOnce(MATCH);
    const start = vi.fn(async () => "http://127.0.0.1:63831/");

    await runOpenInReviewCommand(cli(), binary(), [resolved()], reviewNode(), {
      probe,
      registry,
      running: () => undefined,
      sleep: vi.fn(),
      start,
    });

    expect(start).toHaveBeenCalledWith(
      binary(),
      expectWorkspaceFolder(),
      rememberedRecord().targetKey,
      { port: 63831 },
    );
    expect(update).toHaveBeenCalledOnce();
    expect(registry.get(rememberedRecord().targetKey)?.port).toBe(63831);
    expect(vscodeMocks.openExternal).toHaveBeenCalledWith(
      "http://127.0.0.1:63831/#/revision/rev:sha256:abc",
    );
  });

  it("does not commandeer a remembered port serving another repository", async () => {
    vscodeMocks.showInformationMessage.mockResolvedValue(
      "Start `shore inspect` here",
    );
    const { registry } = registryWith([rememberedRecord()]);
    const probe = vi
      .fn<
        (
          baseUrl: string,
          identity: ReviewIdentity,
        ) => Promise<ReviewProbeResult>
      >()
      .mockResolvedValueOnce(MISMATCH)
      .mockResolvedValueOnce(MATCH);
    const start = vi.fn(async () => "http://127.0.0.1:64000/");

    await runOpenInReviewCommand(cli(), binary(), [resolved()], reviewNode(), {
      probe,
      registry,
      running: () => undefined,
      sleep: vi.fn(),
      start,
    });

    expect(start).toHaveBeenCalledWith(
      binary(),
      expectWorkspaceFolder(),
      rememberedRecord().targetKey,
      { port: 0 },
    );
    expect(registry.get(rememberedRecord().targetKey)?.port).toBe(64000);
  });

  it("opens an attention item over the attention lens", async () => {
    const probe = vi.fn(async () => MATCH);
    const node = { ...reviewNode(), lens: "attention" as const };

    await runOpenInReviewCommand(cli(), binary(), [resolved()], node, {
      probe,
      reviewUrl: "http://127.0.0.1:7878",
      running: () => undefined,
    });

    expect(vscodeMocks.openExternal).toHaveBeenCalledWith(
      "http://127.0.0.1:7878/#/revision/rev:sha256:abc?lens=attention",
    );
  });

  it("starts an ephemeral-port terminal and opens the announced endpoint", async () => {
    vscodeMocks.showInformationMessage.mockResolvedValue(
      "Start `shore inspect` here",
    );
    const announcedUrl = "http://127.0.0.1:63831/";
    const start = vi.fn(async () => announcedUrl);
    const probe = vi
      .fn<
        (
          baseUrl: string,
          identity: ReviewIdentity,
        ) => Promise<ReviewProbeResult>
      >()
      .mockResolvedValueOnce(UNAVAILABLE)
      .mockResolvedValueOnce(MATCH);

    const { registry } = registryWith();
    await runOpenInReviewCommand(cli(), binary(), [resolved()], reviewNode(), {
      probe,
      registry,
      running: () => undefined,
      sleep: vi.fn(),
      start,
    });

    expect(start).toHaveBeenCalledWith(
      binary(),
      expectWorkspaceFolder(),
      "store:sha256:store/context:sha256:repo",
      { port: 0 },
    );
    expect(probe).toHaveBeenNthCalledWith(
      2,
      "http://127.0.0.1:63831",
      EXPECTED_IDENTITY,
    );
    expect(vscodeMocks.openExternal).toHaveBeenCalledWith(
      "http://127.0.0.1:63831/#/revision/rev:sha256:abc",
    );
    expect(registry.get(rememberedRecord().targetKey)).toEqual(
      rememberedRecord(),
    );
  });

  it("does not replace an explicit configured endpoint for another repository", async () => {
    const probe = vi.fn(async () => MISMATCH);
    const start = vi.fn();

    await runOpenInReviewCommand(cli(), binary(), [resolved()], reviewNode(), {
      probe,
      reviewUrl: "http://127.0.0.1:7878",
      running: () => undefined,
      start,
    });

    expect(vscodeMocks.showErrorMessage).toHaveBeenCalledWith(
      expect.stringMatching(/configured.*serves a different repository/i),
    );
    expect(vscodeMocks.showInformationMessage).not.toHaveBeenCalled();
    expect(start).not.toHaveBeenCalled();
    expect(vscodeMocks.openExternal).not.toHaveBeenCalled();
  });

  it("gives up honestly after the announced server fails identity probes", async () => {
    vscodeMocks.showInformationMessage.mockResolvedValue(
      "Start `shore inspect` here",
    );
    const probe = vi.fn(async () => UNAVAILABLE);
    const sleep = vi.fn(async () => undefined);

    await runOpenInReviewCommand(cli(), binary(), [resolved()], reviewNode(), {
      probe,
      running: () => undefined,
      sleep,
      start: vi.fn(async () => "http://127.0.0.1:63831/"),
    });

    expect(probe).toHaveBeenCalledTimes(10);
    expect(sleep).toHaveBeenCalledTimes(10);
    expect(vscodeMocks.openExternal).not.toHaveBeenCalled();
    expect(vscodeMocks.showErrorMessage).toHaveBeenCalledWith(
      expect.stringMatching(/terminal.*did not become available/i),
    );
  });

  it("reports an incompatible launched CLI without retrying a static identity document", async () => {
    vscodeMocks.showInformationMessage.mockResolvedValue(
      "Start `shore inspect` here",
    );
    const probe = vi
      .fn<
        (
          baseUrl: string,
          identity: ReviewIdentity,
        ) => Promise<ReviewProbeResult>
      >(async () => INCOMPATIBLE)
      .mockResolvedValueOnce(UNAVAILABLE);
    const sleep = vi.fn(async () => undefined);

    await runOpenInReviewCommand(cli(), binary(), [resolved()], reviewNode(), {
      probe,
      running: () => undefined,
      sleep,
      start: vi.fn(async () => "http://127.0.0.1:63831/"),
    });

    expect(probe).toHaveBeenCalledTimes(2);
    expect(sleep).toHaveBeenCalledTimes(2);
    expect(vscodeMocks.openExternal).not.toHaveBeenCalled();
    expect(vscodeMocks.showErrorMessage).toHaveBeenCalledWith(
      expect.stringMatching(/shore CLI.*incompatible.*versions match/i),
    );
  });

  it("reports terminal startup failures without probing a guessed port", async () => {
    vscodeMocks.showInformationMessage.mockResolvedValue(
      "Start `shore inspect` here",
    );
    const probe = vi.fn(async () => UNAVAILABLE);

    await runOpenInReviewCommand(cli(), binary(), [resolved()], reviewNode(), {
      probe,
      running: () => undefined,
      start: vi.fn(async () => {
        throw new Error("no URL announced");
      }),
    });

    expect(probe).not.toHaveBeenCalled();
    expect(vscodeMocks.showErrorMessage).toHaveBeenCalledWith(
      expect.stringContaining("no URL announced"),
    );
  });

  it("stops when the selected target is no longer resolved", async () => {
    const probe = vi.fn(async () => MATCH);
    const node = reviewNode();
    node.targetKey = "missing";

    await runOpenInReviewCommand(cli(), binary(), [resolved()], node, {
      probe,
    });

    expect(probe).not.toHaveBeenCalled();
    expect(vscodeMocks.showErrorMessage).toHaveBeenCalledWith(
      expect.stringMatching(/identify.*refresh/i),
    );
  });

  it("disables itself honestly in remote workspaces", async () => {
    vscodeMocks.remoteName = "ssh-remote";
    const probe = vi.fn(async () => MATCH);

    await runOpenInReviewCommand(cli(), binary(), [resolved()], reviewNode(), {
      probe,
      reviewUrl: "http://127.0.0.1:7878",
    });

    expect(vscodeMocks.showInformationMessage).toHaveBeenCalledWith(
      expect.stringMatching(/not available in remote workspaces yet/i),
    );
    expect(probe).not.toHaveBeenCalled();
    expect(vscodeMocks.openExternal).not.toHaveBeenCalled();
  });
});

describe("restoreReviewServers", () => {
  it("leaves a matching remembered server running", async () => {
    const { registry } = registryWith([rememberedRecord()]);
    const probe = vi.fn(async () => MATCH);
    const start = vi.fn();

    await restoreReviewServers(binary(), [resolved()], registry, {
      probe,
      start,
    });

    expect(probe).toHaveBeenCalledWith(
      "http://127.0.0.1:63831",
      EXPECTED_IDENTITY,
    );
    expect(start).not.toHaveBeenCalled();
  });

  it("restores a missing server on its remembered port without revealing it", async () => {
    const { registry } = registryWith([rememberedRecord()]);
    const probe = vi
      .fn<
        (
          baseUrl: string,
          identity: ReviewIdentity,
        ) => Promise<ReviewProbeResult>
      >()
      .mockResolvedValueOnce(UNAVAILABLE)
      .mockResolvedValueOnce(MATCH);
    const start = vi.fn(async () => "http://127.0.0.1:63831/");

    await restoreReviewServers(binary(), [resolved()], registry, {
      probe,
      sleep: vi.fn(),
      start,
    });

    expect(start).toHaveBeenCalledWith(
      binary(),
      expectWorkspaceFolder(),
      rememberedRecord().targetKey,
      { port: 63831, reveal: false },
    );
    expect(registry.get(rememberedRecord().targetKey)?.port).toBe(63831);
  });

  it("uses an ephemeral port when the remembered port has another identity", async () => {
    const { registry } = registryWith([rememberedRecord()]);
    const probe = vi
      .fn<
        (
          baseUrl: string,
          identity: ReviewIdentity,
        ) => Promise<ReviewProbeResult>
      >()
      .mockResolvedValueOnce(MISMATCH)
      .mockResolvedValueOnce(MATCH);
    const start = vi.fn(async () => "http://127.0.0.1:64000/");

    await restoreReviewServers(binary(), [resolved()], registry, {
      probe,
      sleep: vi.fn(),
      start,
    });

    expect(start).toHaveBeenCalledWith(
      binary(),
      expectWorkspaceFolder(),
      rememberedRecord().targetKey,
      { port: 0, reveal: false },
    );
    expect(registry.get(rememberedRecord().targetKey)?.port).toBe(64000);
  });

  it("does not restore a folder with an explicit Review URL override", async () => {
    vscodeMocks.reviewUrl = "http://127.0.0.1:7878";
    const { registry } = registryWith([rememberedRecord()]);
    const probe = vi.fn();
    const start = vi.fn();

    await restoreReviewServers(binary(), [resolved()], registry, {
      probe,
      start,
    });

    expect(probe).not.toHaveBeenCalled();
    expect(start).not.toHaveBeenCalled();
  });
});

function binary(): ResolvedBinary {
  return { path: "/usr/local/bin/shore", source: "path" };
}

function cli(): PointbreakCli {
  return {} as PointbreakCli;
}

function resolved(): TargetResolution {
  return {
    kind: "resolved",
    folder: workspaceFolder("/repo", "repo") as WorkspaceFolder,
    target: {
      key: "store:sha256:store/context:sha256:repo",
      label: "repo",
      ...EXPECTED_IDENTITY,
    },
    emptyInventory: false,
  };
}

function reviewNode() {
  return {
    revisionId: "rev:sha256:abc",
    targetKey: "store:sha256:store/context:sha256:repo",
    folder: workspaceFolder("/repo", "repo") as WorkspaceFolder,
  };
}

function expectWorkspaceFolder() {
  return expect.objectContaining({
    name: "repo",
    uri: expect.objectContaining({ fsPath: "/repo" }),
  });
}

function rememberedRecord(
  overrides: Partial<ReviewServerRecord> = {},
): ReviewServerRecord {
  return {
    targetKey: "store:sha256:store/context:sha256:repo",
    ...EXPECTED_IDENTITY,
    folderUri: "file:///repo",
    port: 63831,
    ...overrides,
  };
}

function registryWith(records: ReviewServerRecord[] = []): {
  registry: ReviewServerRegistry;
  update: ReturnType<typeof vi.fn>;
} {
  const update = vi.fn(async () => undefined);
  const state = {
    keys: () => ["pointbreak.reviewServers"],
    get: vi.fn(() => ({ version: 1, servers: records })) as Memento["get"],
    update,
  };
  return {
    registry: new ReviewServerRegistry(state),
    update,
  };
}
