import { beforeEach, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  ensure: vi.fn(),
  startBrowser: vi.fn(),
}));

vi.mock("vscode", () => ({
  commands: {
    executeCommand: vi.fn(async () => undefined),
    registerCommand: vi.fn(() => ({ dispose: vi.fn() })),
  },
  window: {
    activeTextEditor: undefined,
    createOutputChannel: vi.fn(() => ({
      appendLine: vi.fn(),
      dispose: vi.fn(),
      show: vi.fn(),
    })),
    createTreeView: vi.fn(() => ({ dispose: vi.fn() })),
    onDidChangeActiveTextEditor: vi.fn(() => ({ dispose: vi.fn() })),
  },
  workspace: {
    getConfiguration: vi.fn(() => ({ get: vi.fn() })),
    onDidCloseTextDocument: vi.fn(() => ({ dispose: vi.fn() })),
    workspaceFolders: [],
  },
}));

vi.mock("../src/attentionView", () => ({
  AttentionTreeProvider: class {
    attachTreeView = vi.fn(() => ({ dispose: vi.fn() }));
    dispose = vi.fn();
    refresh = vi.fn();
  },
  refreshAfterWrite: vi.fn(async () => undefined),
}));
vi.mock("../src/binary", () => ({
  resolveBinary: vi.fn(() => ({ path: "/shore", source: "setting" })),
}));
vi.mock("../src/cli", () => ({ PointbreakCli: class {} }));
vi.mock("../src/commands/capture", () => ({ runCaptureCommand: vi.fn() }));
vi.mock("../src/commands/addObservationFromSelection", () => ({
  runAddObservationFromSelectionCommand: vi.fn(),
}));
vi.mock("../src/commands/recordProblemsSnapshot", () => ({
  runRecordProblemsSnapshotCommand: vi.fn(),
}));
vi.mock("../src/commands/runTaskAndRecordValidation", () => ({
  runTaskAndRecordValidationCommand: vi.fn(),
}));
vi.mock("../src/commands/openAnnotatedDiff", () => ({
  runOpenAnnotatedDiffCommand: vi.fn(),
}));
vi.mock("../src/commands/openInSource", () => ({
  OpenInSourceCommand: class {
    dispose = vi.fn();
    open = vi.fn();
  },
  SourceReviewContextStore: class {
    delete = vi.fn();
  },
}));
vi.mock("../src/commands/openInReview", () => ({
  runOpenInReviewCommand: mocks.startBrowser,
}));
vi.mock("../src/inspectChild", () => ({
  InspectChildManager: class {
    ensure = mocks.ensure;
    dispose = vi.fn();
    stop = vi.fn(async () => undefined);
  },
}));
vi.mock("../src/inspectConnectionStore", () => ({
  InspectConnectionStore: class {},
}));
vi.mock("../src/diffDataSource", () => ({
  InspectApiDiffDataSource: class {},
}));
vi.mock("../src/freshnessCoordinator", () => ({
  FreshnessCoordinator: class {
    dispose = vi.fn();
    refreshAll = vi.fn();
    refreshAfterWrite = vi.fn();
  },
}));
vi.mock("../src/logger", () => ({
  Logger: class {
    dispose = vi.fn();
    error = vi.fn();
    warn = vi.fn();
  },
}));
vi.mock("../src/reviewPanel", () => ({
  ReviewPanelManager: class {
    dispose = vi.fn();
    open = vi.fn();
    reloadActive = vi.fn();
  },
}));
vi.mock("../src/targetResolver", () => ({
  resolveTargets: vi.fn(async () => []),
}));
vi.mock("../src/taskRunner", () => ({
  TaskRunner: class {
    dispose = vi.fn();
    run = vi.fn();
  },
}));

import { activate } from "../src/extension";

beforeEach(() => {
  mocks.ensure.mockReset();
  mocks.startBrowser.mockReset();
});

it("does not probe, spawn, or open a terminal during activation", async () => {
  const context = {
    extensionPath: "/extension",
    extensionUri: {},
    secrets: {},
    subscriptions: [] as Array<{ dispose(): unknown }>,
    workspaceState: {},
  };

  await activate(context as never);

  expect(mocks.ensure).not.toHaveBeenCalled();
  expect(mocks.startBrowser).not.toHaveBeenCalled();
});
