import path from "node:path";
import { describe, expect, it } from "vitest";
import type {
  ReviewSnapshotDoc,
  ReviewSnapshotFile,
  ReviewSnapshotRow,
} from "../src/cli";
import {
  liveSelectionToSnapshot,
  repoRelativeFile,
  snapshotToLive,
} from "../src/sourceMapping";
import type { SnapshotRangeTarget } from "../src/webviewProtocol";

describe("snapshotToLive", () => {
  it.each([
    {
      name: "new-side context",
      target: target("src/lib.rs", "new", 1),
      live: ["alpha", "new", "omega"],
      kind: "exact",
      start: 0,
    },
    {
      name: "new-side addition",
      target: target("src/lib.rs", "new", 2),
      live: ["alpha", "new", "omega"],
      kind: "exact",
      start: 1,
    },
    {
      name: "old-side context on a modified path",
      target: target("src/lib.rs", "old", 1),
      live: ["alpha", "new", "omega"],
      kind: "exact",
      start: 0,
    },
    {
      name: "removed row with a surviving context anchor",
      target: target("src/lib.rs", "old", 2),
      live: ["alpha", "new", "omega"],
      kind: "drifted",
      start: 1,
    },
  ])("maps $name conservatively", ({
    target: rowTarget,
    live,
    kind,
    start,
  }) => {
    const landing = snapshotToLive(modifiedSnapshot(), rowTarget, live);

    expect(landing).toMatchObject({
      kind,
      filePath: "src/lib.rs",
      range: {
        start: { line: start, character: 0 },
        end: { line: start, character: live[start]?.length ?? 0 },
      },
    });
  });

  it("maps multi-line Unicode content without treating UTF-16 columns as byte offsets", () => {
    const snapshot = document([
      file("src/unicode.ts", "src/unicode.ts", [
        row("context", 7, 7, "const emoji = '👩🏽‍💻';"),
        row("context", 8, 8, "const café = true;"),
      ]),
    ]);
    const live = Array.from({ length: 6 }, () => "").concat([
      "const emoji = '👩🏽‍💻';",
      "const café = true;",
    ]);

    expect(
      snapshotToLive(snapshot, target("src/unicode.ts", "new", 7, 8), live),
    ).toEqual({
      kind: "exact",
      filePath: "src/unicode.ts",
      range: {
        start: { line: 6, character: 0 },
        end: { line: 7, character: "const café = true;".length },
      },
    });
  });

  it("uses a bounded unique match and reports visible drift when lines move", () => {
    const snapshot = document([
      file("src/a.ts", "src/a.ts", [
        row("context", 10, 10, "before"),
        row("added", null, 11, "selected"),
        row("context", 11, 12, "after"),
      ]),
    ]);
    const live = Array.from(
      { length: 10 },
      (_, index) => `line ${index + 1}`,
    ).concat(["inserted", "before", "selected", "after"]);

    const landing = snapshotToLive(
      snapshot,
      target("src/a.ts", "new", 11),
      live,
    );

    expect(landing).toMatchObject({
      kind: "drifted",
      filePath: "src/a.ts",
      range: {
        start: { line: 12, character: 0 },
        end: { line: 12, character: "selected".length },
      },
    });
  });

  it("prefers preserved context over unrelated matching content", () => {
    const snapshot = document([
      file("src/a.ts", "src/a.ts", [
        row("context", 1, 1, "before"),
        row("removed", 2, null, "gone"),
        row("context", 3, 2, "after"),
      ]),
    ]);
    const live = ["before", "replacement", "after"]
      .concat(Array.from({ length: 7 }, (_, index) => `filler ${index}`))
      .concat("gone");

    expect(
      snapshotToLive(snapshot, target("src/a.ts", "old", 2), live),
    ).toMatchObject({
      kind: "drifted",
      range: {
        start: { line: 1, character: 0 },
        end: { line: 1, character: "replacement".length },
      },
    });
  });

  it("never chooses between ambiguous live matches", () => {
    const snapshot = document([
      file("src/a.ts", "src/a.ts", [row("added", null, 2, "repeat")]),
    ]);

    expect(
      snapshotToLive(snapshot, target("src/a.ts", "new", 2), [
        "repeat",
        "changed",
        "repeat",
      ]),
    ).toMatchObject({ kind: "unavailable", reason: "ambiguous" });
  });

  it.each([
    {
      name: "missing snapshot path",
      snapshot: modifiedSnapshot(),
      target: target("src/missing.rs", "new", 1),
      reason: "missing-file",
    },
    {
      name: "out-of-range target",
      snapshot: modifiedSnapshot(),
      target: target("src/lib.rs", "new", 99),
      reason: "missing-row",
    },
    {
      name: "deleted old-side-only file",
      snapshot: document([
        file("src/deleted.rs", null, [row("removed", 1, null, "gone")]),
      ]),
      target: target("src/deleted.rs", "old", 1),
      reason: "old-side-only",
    },
    {
      name: "renamed old side",
      snapshot: document([
        file("src/old.rs", "src/new.rs", [row("context", 1, 1, "same")]),
      ]),
      target: target("src/old.rs", "old", 1),
      reason: "old-side-only",
    },
    {
      name: "binary file",
      snapshot: document([
        {
          ...file("asset.bin", "asset.bin", []),
          is_binary: true,
        },
      ]),
      target: target("asset.bin", "new", 1),
      reason: "no-lines",
    },
    {
      name: "mode-only file",
      snapshot: document([
        {
          ...file("script.sh", "script.sh", []),
          is_mode_only: true,
        },
      ]),
      target: target("script.sh", "new", 1),
      reason: "no-lines",
    },
  ])("degrades $name with an explicit reason", ({
    snapshot,
    target,
    reason,
  }) => {
    expect(snapshotToLive(snapshot, target, ["live"])).toMatchObject({
      kind: "unavailable",
      reason,
      message: expect.any(String),
    });
  });

  it("opens the new path of a rename at identity coordinates", () => {
    const snapshot = document([
      file("src/old.rs", "src/new.rs", [row("context", 1, 1, "same")]),
    ]);

    expect(
      snapshotToLive(snapshot, target("src/new.rs", "new", 1), ["same"]),
    ).toMatchObject({ kind: "exact", filePath: "src/new.rs" });
  });
});

describe("repoRelativeFile", () => {
  it("emits repository-relative forward slashes under Windows", () => {
    expect(
      repoRelativeFile(
        "C:\\work\\repo",
        "C:\\work\\repo\\src\\nested\\file.ts",
        path.win32,
      ),
    ).toBe("src/nested/file.ts");
  });

  it.each([
    ["/repo", "/other/file.ts", path.posix],
    ["C:\\repo", "D:\\outside\\file.ts", path.win32],
    ["/repo", "src/file.ts", path.posix],
    ["/repo", "/repo", path.posix],
  ])("rejects outside, non-absolute, and non-file paths", (root, filePath, api) => {
    expect(() => repoRelativeFile(root, filePath, api)).toThrow();
  });

  it("preserves a safe snapshot path identity", () => {
    expect(repoRelativeFile("/repo", "/repo/src/lib.rs", path.posix)).toBe(
      "src/lib.rs",
    );
  });
});

describe("liveSelectionToSnapshot", () => {
  it.each([
    ["caret", selection(1, 2, 1, 2), 2, 2],
    ["column-zero end", selection(0, 2, 2, 0), 1, 2],
    ["included final line", selection(0, 2, 2, 1), 1, 3],
  ])("converts %s to 1-based inclusive lines", (_name, live, start, end) => {
    expect(
      liveSelectionToSnapshot(modifiedSnapshot(), "src/lib.rs", live, [
        "alpha",
        "new",
        "omega",
      ]),
    ).toMatchObject({
      kind: "verified",
      target: {
        filePath: "src/lib.rs",
        side: "new",
        startLine: start,
        endLine: end,
      },
    });
  });

  it.each([
    [
      "comparable text changed",
      modifiedSnapshot(),
      "src/lib.rs",
      ["alpha", "changed", "omega"],
      selection(1, 0, 1, 3),
      "drifted",
    ],
    [
      "line outside captured hunks",
      document([
        file("src/lib.rs", "src/lib.rs", [row("context", 1, 1, "alpha")]),
      ]),
      "src/lib.rs",
      ["alpha", "outside"],
      selection(0, 0, 1, 7),
      "unverified",
    ],
    [
      "uncovered line plus mismatch",
      document([
        file("src/lib.rs", "src/lib.rs", [row("context", 1, 1, "alpha")]),
      ]),
      "src/lib.rs",
      ["changed", "outside"],
      selection(0, 0, 1, 7),
      "drifted",
    ],
    [
      "file absent from snapshot",
      modifiedSnapshot(),
      "src/other.rs",
      ["other"],
      selection(0, 0, 0, 5),
      "not-in-snapshot",
    ],
  ] as const)("classifies %s as %s", (_name, snapshot, filePath, lines, live, kind) => {
    expect(
      liveSelectionToSnapshot(snapshot, filePath, live, lines),
    ).toMatchObject({ kind, message: expect.any(String) });
  });

  it("maps a uniquely shifted live selection back to captured coordinates", () => {
    expect(
      liveSelectionToSnapshot(
        modifiedSnapshot(),
        "src/lib.rs",
        selection(2, 0, 2, 3),
        ["inserted", "alpha", "new", "omega"],
        -1,
      ),
    ).toEqual({
      kind: "verified",
      target: {
        filePath: "src/lib.rs",
        side: "new",
        startLine: 2,
        endLine: 2,
      },
    });
  });

  it("does not re-anchor changed selection text to an unrelated captured line", () => {
    const snapshot = document([
      file("src/lib.rs", "src/lib.rs", [
        row("context", 2, 2, "expected"),
        row("context", 10, 10, "changed"),
      ]),
    ]);

    expect(
      liveSelectionToSnapshot(snapshot, "src/lib.rs", selection(1, 0, 1, 7), [
        "before",
        "changed",
      ]),
    ).toMatchObject({
      kind: "drifted",
      target: { startLine: 2, endLine: 2 },
    });
  });
});

function modifiedSnapshot(): ReviewSnapshotDoc {
  return document([
    file("src/lib.rs", "src/lib.rs", [
      row("context", 1, 1, "alpha"),
      row("removed", 2, null, "old"),
      row("added", null, 2, "new"),
      row("context", 3, 3, "omega"),
    ]),
  ]);
}

function target(
  filePath: string,
  side: "old" | "new",
  startLine: number,
  endLine = startLine,
): SnapshotRangeTarget {
  return { filePath, side, startLine, endLine };
}

function selection(
  startLine: number,
  startCharacter: number,
  endLine: number,
  endCharacter: number,
) {
  return {
    start: { line: startLine, character: startCharacter },
    end: { line: endLine, character: endCharacter },
  };
}

function row(
  kind: string,
  oldLine: number | null,
  newLine: number | null,
  text: string,
): ReviewSnapshotRow {
  return { kind, old_line: oldLine, new_line: newLine, text };
}

function file(
  oldPath: string | null,
  newPath: string | null,
  rows: ReviewSnapshotRow[],
): ReviewSnapshotFile {
  return {
    id: newPath ?? oldPath ?? "unknown",
    old_path: oldPath,
    new_path: newPath,
    hunks: rows.length ? [{ id: "hunk", header: "@@", rows }] : [],
  };
}

function document(files: ReviewSnapshotFile[]): ReviewSnapshotDoc {
  return {
    schema: "pointbreak.review-snapshot",
    version: 1,
    contentHash: "sha256:snapshot",
    snapshot: {
      review_id: "review:default",
      object_id: "obj:sha256:snapshot",
      files,
    },
  };
}
