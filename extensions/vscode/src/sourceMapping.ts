import path from "node:path";
import type {
  ReviewSnapshotDoc,
  ReviewSnapshotFile,
  ReviewSnapshotRow,
} from "./cli";
import type { SnapshotRangeTarget } from "./webviewProtocol";

const SEARCH_RADIUS = 80;

export interface ZeroBasedPosition {
  readonly line: number;
  readonly character: number;
}

export interface ZeroBasedSelection {
  readonly start: ZeroBasedPosition;
  readonly end: ZeroBasedPosition;
}

export type SourceUnavailableReason =
  | "missing-file"
  | "old-side-only"
  | "no-lines"
  | "missing-row"
  | "ambiguous"
  | "unmappable";

export type SourceLanding =
  | {
      readonly kind: "exact" | "drifted";
      readonly filePath: string;
      readonly range: ZeroBasedSelection;
      readonly message?: string;
    }
  | {
      readonly kind: "unavailable";
      readonly reason: SourceUnavailableReason;
      readonly message: string;
    };

export type SourceFileResolution =
  | { readonly kind: "available"; readonly filePath: string }
  | Extract<SourceLanding, { kind: "unavailable" }>;

export type SelectionVerification =
  | { readonly kind: "verified"; readonly target: SnapshotRangeTarget }
  | {
      readonly kind: "drifted";
      readonly target: SnapshotRangeTarget;
      readonly message: string;
    }
  | {
      readonly kind: "unverified";
      readonly target: SnapshotRangeTarget;
      readonly message: string;
    }
  | { readonly kind: "not-in-snapshot"; readonly message: string };

interface IndexedRow {
  readonly row: ReviewSnapshotRow;
  readonly line: number;
}

interface IndexedFile {
  readonly oldPath?: string;
  readonly newPath?: string;
  readonly oldRows: Map<number, ReviewSnapshotRow>;
  readonly newRows: Map<number, ReviewSnapshotRow>;
  readonly oldOrdered: readonly IndexedRow[];
  readonly newOrdered: readonly IndexedRow[];
}

/** Maps a captured snapshot range onto a current live buffer conservatively. */
export function snapshotToLive(
  snapshot: ReviewSnapshotDoc,
  target: SnapshotRangeTarget,
  liveLines: readonly string[],
): SourceLanding {
  const located = locateFile(snapshot, target);
  if (located.kind === "unavailable") return located;

  const index = located.index;
  const rows = target.side === "old" ? index.oldRows : index.newRows;
  const ordered = target.side === "old" ? index.oldOrdered : index.newOrdered;
  if (ordered.length === 0) {
    return unavailable(
      "no-lines",
      "This captured file has no source lines that can be opened.",
    );
  }

  const captured: ReviewSnapshotRow[] = [];
  for (let line = target.startLine; line <= target.endLine; line += 1) {
    const row = rows.get(line);
    if (!row) {
      return unavailable(
        "missing-row",
        "That range is outside the captured source lines.",
      );
    }
    captured.push(row);
  }

  const expectedStart = target.startLine - 1;
  const expectedEnd = target.endLine - 1;
  if (
    expectedStart >= 0 &&
    expectedEnd < liveLines.length &&
    captured.every(
      (row, offset) => liveLines[expectedStart + offset] === row.text,
    )
  ) {
    return {
      kind: "exact",
      filePath: located.filePath,
      range: liveRange(liveLines, expectedStart, expectedEnd),
    };
  }

  const anchor = nearestUniqueContextAnchor(
    ordered,
    liveLines,
    target.startLine,
  );
  if (anchor.kind === "ambiguous") {
    return unavailable(
      "ambiguous",
      "The captured context does not identify one live location.",
    );
  }
  if (anchor.kind === "found") {
    const start = expectedStart + anchor.offset;
    const end = expectedEnd + anchor.offset;
    if (start >= 0 && end < liveLines.length) {
      return driftedLanding(located.filePath, liveLines, start, end);
    }
  }

  const sequenceMatches = matchingSequences(
    liveLines,
    captured.map((row) => row.text),
    expectedStart,
  );
  if (sequenceMatches.length > 1) {
    return unavailable(
      "ambiguous",
      "The captured source matches more than one live location.",
    );
  }
  if (sequenceMatches.length === 1) {
    const start = sequenceMatches[0];
    return driftedLanding(
      located.filePath,
      liveLines,
      start,
      start + captured.length - 1,
    );
  }

  return unavailable(
    "unmappable",
    "The live source has changed too much to map this range safely.",
  );
}

/** Selects the current repository path for a captured source target. */
export function sourceFileForTarget(
  snapshot: ReviewSnapshotDoc,
  target: SnapshotRangeTarget,
): SourceFileResolution {
  const located = locateFile(snapshot, target);
  return located.kind === "unavailable"
    ? located
    : { kind: "available", filePath: located.filePath };
}

/** Classifies a live editor selection against captured new-side rows. */
export function liveSelectionToSnapshot(
  snapshot: ReviewSnapshotDoc,
  filePath: string,
  selection: ZeroBasedSelection,
  liveLines: readonly string[],
  lineOffset = 0,
): SelectionVerification {
  const lineRange = inclusiveSelectedLines(selection);
  if (!lineRange) {
    return {
      kind: "not-in-snapshot",
      message: "The active editor selection is not a valid source range.",
    };
  }
  const matches = snapshot.snapshot.files
    .map(indexFile)
    .filter((file) => file.newPath === filePath);
  if (matches.length !== 1) {
    return {
      kind: "not-in-snapshot",
      message: "The active file is not present in this captured snapshot.",
    };
  }

  const startLine = lineRange.start + 1 + lineOffset;
  const endLine = lineRange.end + 1 + lineOffset;
  if (startLine < 1 || endLine < startLine) {
    return {
      kind: "not-in-snapshot",
      message:
        "The active editor selection cannot be mapped into this snapshot.",
    };
  }
  const target: SnapshotRangeTarget = {
    filePath,
    side: "new",
    startLine,
    endLine,
  };
  let uncovered = false;
  let mismatch = false;
  for (let line = lineRange.start; line <= lineRange.end; line += 1) {
    const captured = matches[0].newRows.get(line + 1 + lineOffset);
    if (!captured) {
      uncovered = true;
    } else if (captured.text !== liveLines[line]) {
      mismatch = true;
    }
  }
  if (!mismatch && !uncovered) {
    return { kind: "verified", target };
  }

  if (mismatch) {
    return {
      kind: "drifted",
      target,
      message:
        "Some selected source has changed since this review was captured.",
    };
  }
  return {
    kind: "unverified",
    target,
    message:
      "Some selected source is outside the lines stored in this captured diff.",
  };
}

/** Converts an absolute file into the CLI's repository-relative `/` form. */
export function repoRelativeFile(
  repoRoot: string,
  absoluteFile: string,
  pathApi: Pick<
    typeof path,
    "isAbsolute" | "relative" | "normalize" | "sep"
  > = path,
): string {
  if (!pathApi.isAbsolute(repoRoot) || !pathApi.isAbsolute(absoluteFile)) {
    throw new Error("Repository and file paths must be absolute.");
  }
  const normalizedRoot = pathApi.normalize(repoRoot);
  const normalizedFile = pathApi.normalize(absoluteFile);
  const relative = pathApi.relative(normalizedRoot, normalizedFile);
  if (
    !relative ||
    pathApi.isAbsolute(relative) ||
    relative === ".." ||
    relative.startsWith(`..${pathApi.sep}`)
  ) {
    throw new Error("Source file is outside the repository.");
  }
  const portable = relative.split(pathApi.sep).join("/");
  if (
    portable.startsWith("/") ||
    portable.includes("\\") ||
    portable.split("/").some((part) => !part || part === "..")
  ) {
    throw new Error("Source file is not a safe repository-relative path.");
  }
  return portable;
}

type LocatedFile =
  | {
      readonly kind: "found";
      readonly index: IndexedFile;
      readonly filePath: string;
    }
  | Extract<SourceLanding, { kind: "unavailable" }>;

function locateFile(
  snapshot: ReviewSnapshotDoc,
  target: SnapshotRangeTarget,
): LocatedFile {
  const matches = snapshot.snapshot.files
    .map(indexFile)
    .filter((index) =>
      target.side === "old"
        ? index.oldPath === target.filePath
        : index.newPath === target.filePath,
    );
  if (matches.length !== 1) {
    return unavailable(
      matches.length > 1 ? "ambiguous" : "missing-file",
      matches.length > 1
        ? "The captured source path is ambiguous."
        : "That file is not present in the captured snapshot.",
    );
  }
  const index = matches[0];
  if (
    target.side === "old" &&
    (!index.newPath || index.newPath !== index.oldPath)
  ) {
    return unavailable(
      "old-side-only",
      "The captured old-side source is not available in the current worktree.",
    );
  }
  if (!index.newPath) {
    return unavailable("missing-file", "That file has no current source path.");
  }
  return { kind: "found", index, filePath: index.newPath };
}

function indexFile(file: ReviewSnapshotFile): IndexedFile {
  const oldRows = new Map<number, ReviewSnapshotRow>();
  const newRows = new Map<number, ReviewSnapshotRow>();
  const oldOrdered: IndexedRow[] = [];
  const newOrdered: IndexedRow[] = [];
  for (const hunk of file.hunks ?? []) {
    for (const row of hunk.rows ?? []) {
      if (row.old_line != null) {
        oldRows.set(row.old_line, row);
        oldOrdered.push({ row, line: row.old_line });
      }
      if (row.new_line != null) {
        newRows.set(row.new_line, row);
        newOrdered.push({ row, line: row.new_line });
      }
    }
  }
  return {
    oldPath: stringField(file.old_path),
    newPath: stringField(file.new_path),
    oldRows,
    newRows,
    oldOrdered,
    newOrdered,
  };
}

function inclusiveSelectedLines(
  selection: ZeroBasedSelection,
): { readonly start: number; readonly end: number } | undefined {
  const { start, end } = selection;
  if (
    !Number.isInteger(start.line) ||
    start.line < 0 ||
    !Number.isInteger(start.character) ||
    start.character < 0 ||
    !Number.isInteger(end.line) ||
    end.line < start.line ||
    !Number.isInteger(end.character) ||
    end.character < 0
  ) {
    return undefined;
  }
  const inclusiveEnd =
    end.line > start.line && end.character === 0 ? end.line - 1 : end.line;
  return { start: start.line, end: Math.max(start.line, inclusiveEnd) };
}

function nearestUniqueContextAnchor(
  rows: readonly IndexedRow[],
  liveLines: readonly string[],
  targetLine: number,
):
  | { readonly kind: "none" }
  | { readonly kind: "ambiguous" }
  | { readonly kind: "found"; readonly offset: number } {
  const candidates = rows
    .filter(({ row }) => row.kind === "context")
    .map(({ row, line }) => ({
      distance: Math.abs(line - targetLine),
      line,
      matches: matchingLines(liveLines, row.text, line - 1),
    }))
    .filter(({ matches }) => matches.length === 1)
    .sort((left, right) => left.distance - right.distance);
  if (candidates.length === 0) return { kind: "none" };

  const nearestDistance = candidates[0].distance;
  const nearestOffsets = new Set(
    candidates
      .filter(({ distance }) => distance === nearestDistance)
      .map(({ line, matches }) => matches[0] - (line - 1)),
  );
  if (nearestOffsets.size !== 1) return { kind: "ambiguous" };
  return { kind: "found", offset: [...nearestOffsets][0] };
}

function matchingSequences(
  liveLines: readonly string[],
  captured: readonly string[],
  expectedStart: number,
): number[] {
  if (captured.length === 0) return [];
  const start = Math.max(0, expectedStart - SEARCH_RADIUS);
  const end = Math.min(
    liveLines.length - captured.length,
    expectedStart + SEARCH_RADIUS,
  );
  const matches: number[] = [];
  for (let candidate = start; candidate <= end; candidate += 1) {
    if (
      captured.every((text, offset) => liveLines[candidate + offset] === text)
    ) {
      matches.push(candidate);
    }
  }
  return matches;
}

function matchingLines(
  liveLines: readonly string[],
  text: string,
  expected: number,
): number[] {
  const start = Math.max(0, expected - SEARCH_RADIUS);
  const end = Math.min(liveLines.length - 1, expected + SEARCH_RADIUS);
  const matches: number[] = [];
  for (let candidate = start; candidate <= end; candidate += 1) {
    if (liveLines[candidate] === text) matches.push(candidate);
  }
  return matches;
}

function liveRange(
  liveLines: readonly string[],
  startLine: number,
  endLine: number,
): ZeroBasedSelection {
  return {
    start: { line: startLine, character: 0 },
    end: { line: endLine, character: liveLines[endLine]?.length ?? 0 },
  };
}

function driftedLanding(
  filePath: string,
  liveLines: readonly string[],
  startLine: number,
  endLine: number,
): SourceLanding {
  return {
    kind: "drifted",
    filePath,
    range: liveRange(liveLines, startLine, endLine),
    message: "The live source has changed since this review was captured.",
  };
}

function unavailable(
  reason: SourceUnavailableReason,
  message: string,
): Extract<SourceLanding, { kind: "unavailable" }> {
  return { kind: "unavailable", reason, message };
}

function stringField(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}
