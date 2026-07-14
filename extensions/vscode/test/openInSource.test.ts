import { describe, expect, it, vi } from "vitest";

vi.mock("vscode", () => ({
  Range: class {},
  Selection: class {},
  TextEditorRevealType: { InCenter: 0 },
  Uri: { file: vi.fn() },
  ViewColumn: { One: 1 },
  window: {
    createTextEditorDecorationType: vi.fn(),
    showInformationMessage: vi.fn(),
    showTextDocument: vi.fn(),
  },
  workspace: { openTextDocument: vi.fn() },
}));

import type { ReviewSnapshotDoc } from "../src/cli";
import {
  OpenInSourceCommand,
  SourceReviewContextStore,
} from "../src/commands/openInSource";

describe("OpenInSourceCommand", () => {
  it("revalidates, opens, reveals, decorates, and retains credential-free context", async () => {
    const document = { id: "document" };
    const host = hostWith(["alpha", "beta"], document);
    const contexts = new SourceReviewContextStore();
    const command = new OpenInSourceCommand(contexts, host);

    await command.open({
      repoRoot: "/repo",
      targetKey: "store/context",
      revisionId: "rev:sha256:one",
      snapshot: snapshot(),
      target: {
        filePath: "src/lib.rs",
        side: "new",
        startLine: 2,
        endLine: 2,
      },
    });

    expect(host.openDocument).toHaveBeenCalledWith("/repo/src/lib.rs");
    expect(host.reveal).toHaveBeenCalledWith(document, {
      start: { line: 1, character: 0 },
      end: { line: 1, character: 4 },
    });
    expect(host.decorateTemporarily).toHaveBeenCalledWith(document, {
      start: { line: 1, character: 0 },
      end: { line: 1, character: 4 },
    });
    expect(contexts.get(document)).toMatchObject({
      targetKey: "store/context",
      revisionId: "rev:sha256:one",
      filePath: "src/lib.rs",
      side: "new",
      lastLanding: { kind: "exact" },
    });
    expect(JSON.stringify(contexts.get(document))).not.toMatch(
      /token|bearer|origin|port|\/repo/,
    );
    contexts.delete(document);
    expect(contexts.get(document)).toBeUndefined();
  });

  it("shows drift before revealing the mapped range", async () => {
    const document = { id: "drifted" };
    const host = hostWith(["inserted", "alpha", "beta"], document);
    const contexts = new SourceReviewContextStore();
    const command = new OpenInSourceCommand(contexts, host);

    await command.open({
      repoRoot: "/repo",
      targetKey: "store/context",
      revisionId: "rev:sha256:one",
      snapshot: snapshot(),
      target: {
        filePath: "src/lib.rs",
        side: "new",
        startLine: 2,
        endLine: 2,
      },
    });

    expect(host.showInformationMessage).toHaveBeenCalledWith(
      expect.stringMatching(/changed|drift/i),
    );
    expect(host.reveal).toHaveBeenCalledWith(
      document,
      expect.objectContaining({ start: { line: 2, character: 0 } }),
    );
  });

  it("does not wait for the drift notification before revealing source", async () => {
    const document = { id: "drifted" };
    const host = hostWith(["inserted", "alpha", "beta"], document);
    let dismiss!: () => void;
    host.showInformationMessage.mockReturnValueOnce(
      new Promise<undefined>((resolve) => {
        dismiss = () => resolve(undefined);
      }),
    );
    const command = new OpenInSourceCommand(
      new SourceReviewContextStore(),
      host,
    );

    const opening = command.open({
      repoRoot: "/repo",
      targetKey: "store/context",
      revisionId: "rev:sha256:one",
      snapshot: snapshot(),
      target: {
        filePath: "src/lib.rs",
        side: "new",
        startLine: 2,
        endLine: 2,
      },
    });
    await Promise.resolve();
    await Promise.resolve();

    expect(host.reveal).toHaveBeenCalledWith(
      document,
      expect.objectContaining({ start: { line: 2, character: 0 } }),
    );
    dismiss();
    await opening;
  });

  it.each([
    {
      name: "outside-root target",
      filePath: "../secret.txt",
      openError: undefined,
    },
    {
      name: "missing file",
      filePath: "src/lib.rs",
      openError: new Error("ENOENT /repo/src/lib.rs"),
    },
  ])("blocks a $name without leaking an absolute path", async ({
    filePath,
    openError,
  }) => {
    const host = hostWith(["alpha", "beta"], { id: "document" });
    if (openError) host.openDocument.mockRejectedValueOnce(openError);
    const command = new OpenInSourceCommand(
      new SourceReviewContextStore(),
      host,
    );

    await command.open({
      repoRoot: "/repo",
      targetKey: "store/context",
      revisionId: "rev:sha256:one",
      snapshot: snapshot(),
      target: { filePath, side: "new", startLine: 2, endLine: 2 },
    });

    expect(host.reveal).not.toHaveBeenCalled();
    expect(host.decorateTemporarily).not.toHaveBeenCalled();
    expect(JSON.stringify(host.showInformationMessage.mock.calls)).not.toMatch(
      /\/repo|secret\.txt/,
    );
  });

  it("keeps the webview focused for unavailable old-side source", async () => {
    const host = hostWith(["gone"], { id: "document" });
    const command = new OpenInSourceCommand(
      new SourceReviewContextStore(),
      host,
    );
    const deleted = snapshot();
    deleted.snapshot.files[0] = {
      ...deleted.snapshot.files[0],
      new_path: null,
      hunks: [
        {
          id: "hunk",
          header: "@@",
          rows: [
            { kind: "removed", old_line: 1, new_line: null, text: "gone" },
          ],
        },
      ],
    };

    await command.open({
      repoRoot: "/repo",
      targetKey: "store/context",
      revisionId: "rev:sha256:one",
      snapshot: deleted,
      target: {
        filePath: "src/lib.rs",
        side: "old",
        startLine: 1,
        endLine: 1,
      },
    });

    expect(host.openDocument).not.toHaveBeenCalled();
    expect(host.reveal).not.toHaveBeenCalled();
    expect(host.showInformationMessage).toHaveBeenCalledWith(
      expect.stringMatching(/old|deleted/i),
    );
  });
});

function hostWith(lines: readonly string[], document: object) {
  return {
    openDocument: vi.fn(async () => ({ document, lines })),
    reveal: vi.fn(async () => undefined),
    decorateTemporarily: vi.fn(),
    showInformationMessage: vi.fn(async () => undefined),
    dispose: vi.fn(),
  };
}

function snapshot(): ReviewSnapshotDoc {
  return {
    schema: "pointbreak.review-snapshot",
    version: 1,
    contentHash: "sha256:snapshot",
    snapshot: {
      review_id: "review:default",
      object_id: "obj:sha256:snapshot",
      files: [
        {
          id: "src/lib.rs",
          old_path: "src/lib.rs",
          new_path: "src/lib.rs",
          hunks: [
            {
              id: "hunk",
              header: "@@",
              rows: [
                { kind: "context", old_line: 1, new_line: 1, text: "alpha" },
                { kind: "context", old_line: 2, new_line: 2, text: "beta" },
              ],
            },
          ],
        },
      ],
    },
  };
}
