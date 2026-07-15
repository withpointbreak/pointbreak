import { repoRelativeFile } from "./sourceMapping";

export const PROBLEMS_SNAPSHOT_CAVEAT =
  "This is an incomplete point-in-time view of diagnostics VS Code currently surfaces.";

interface ProblemsUri {
  readonly fsPath: string;
}

interface ProblemsPosition {
  readonly line: number;
  readonly character: number;
}

interface ProblemsRange {
  readonly start: ProblemsPosition;
  readonly end: ProblemsPosition;
}

interface ProblemsCodeTarget {
  readonly value: string | number;
  readonly target: unknown;
}

interface ProblemsDiagnostic {
  readonly message: string;
  readonly range: ProblemsRange;
  readonly severity?: number;
  readonly source?: string;
  readonly code?: string | number | ProblemsCodeTarget;
}

export type ProblemsSample = readonly (readonly [
  ProblemsUri,
  readonly ProblemsDiagnostic[],
])[];

export interface ProblemsSnapshotOptions {
  readonly repoRoot: string;
  readonly targetLabel: string;
  readonly timestamp: string;
}

interface SnapshotEntry {
  readonly path: string;
  readonly range: ProblemsRange;
  readonly severity: number;
  readonly source: string;
  readonly code: string;
  readonly message: string;
}

const SEVERITY_LABELS = ["Error", "Warning", "Information", "Hint"] as const;
const SEVERITY_COUNT_KEYS = [
  "error",
  "warning",
  "information",
  "hint",
] as const;

export function buildProblemsSnapshot(
  sample: ProblemsSample,
  options: ProblemsSnapshotOptions,
): string {
  const entries = targetEntries(sample, options.repoRoot).sort(compareEntries);
  const counts = countSeverities(entries);
  const lines = [
    "# Problems snapshot",
    "",
    `**Target:** ${escapeMarkdown(options.targetLabel)}`,
    `**Sampled:** ${options.timestamp}`,
    `**Counts:** ${entries.length} total; error ${counts.error}; warning ${counts.warning}; information ${counts.information}; hint ${counts.hint}; unknown ${counts.unknown}`,
    "",
    `> ${PROBLEMS_SNAPSHOT_CAVEAT}`,
    "",
    "## Diagnostics",
    "",
  ];

  if (entries.length === 0) {
    lines.push(
      "No diagnostics were currently reported.",
      "",
      `> ${PROBLEMS_SNAPSHOT_CAVEAT}`,
    );
  } else {
    lines.push(...entries.map(renderEntry));
  }
  return `${lines.join("\n")}\n`;
}

function targetEntries(
  sample: ProblemsSample,
  repoRoot: string,
): SnapshotEntry[] {
  const entries: SnapshotEntry[] = [];
  for (const [uri, diagnostics] of sample) {
    let relativePath: string;
    try {
      relativePath = repoRelativeFile(repoRoot, uri.fsPath);
    } catch {
      continue;
    }
    for (const diagnostic of diagnostics) {
      if (!validRange(diagnostic.range)) continue;
      entries.push({
        path: relativePath,
        range: diagnostic.range,
        severity: severitySortValue(diagnostic.severity),
        source: normalizeLine(diagnostic.source ?? ""),
        code: normalizeLine(diagnosticCode(diagnostic.code)),
        message: normalizeMultiline(diagnostic.message),
      });
    }
  }
  return entries;
}

function compareEntries(left: SnapshotEntry, right: SnapshotEntry): number {
  return (
    compareText(left.path, right.path) ||
    comparePosition(left.range.start, right.range.start) ||
    comparePosition(left.range.end, right.range.end) ||
    left.severity - right.severity ||
    compareText(left.source, right.source) ||
    compareText(left.code, right.code) ||
    compareText(left.message, right.message)
  );
}

function compareText(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

function comparePosition(
  left: ProblemsPosition,
  right: ProblemsPosition,
): number {
  return left.line - right.line || left.character - right.character;
}

function renderEntry(entry: SnapshotEntry): string {
  const location = `${escapeMarkdown(entry.path)}:${entry.range.start.line + 1}:${entry.range.start.character + 1}–${entry.range.end.line + 1}:${entry.range.end.character + 1}`;
  const source = entry.source
    ? ` — source: ${escapeMarkdown(entry.source)}`
    : "";
  const code = entry.code ? ` — code: ${escapeMarkdown(entry.code)}` : "";
  return `- **${severityLabel(entry.severity)}** — ${location}${source}${code} — ${escapeMultiline(entry.message)}`;
}

function normalizeMultiline(value: string): string {
  const lines = value.replace(/\r\n?/g, "\n").split("\n");
  while (lines.length > 0 && !lines[0].trim()) lines.shift();
  while (lines.length > 0 && !lines.at(-1)?.trim()) lines.pop();
  return lines.map(normalizeLine).join("\n");
}

function normalizeLine(value: string): string {
  return value.trim();
}

function escapeMultiline(value: string): string {
  return value.split("\n").map(escapeMarkdown).join("<br>");
}

function escapeMarkdown(value: string): string {
  return value.replace(/([\\`*_[\]{}()<>#+\-.!|])/g, "\\$1");
}

function diagnosticCode(code: ProblemsDiagnostic["code"]): string {
  if (typeof code === "object" && code !== null) {
    return String(code.value);
  }
  return code === undefined ? "" : String(code);
}

function validRange(range: ProblemsRange): boolean {
  return (
    validPosition(range.start) &&
    validPosition(range.end) &&
    comparePosition(range.start, range.end) <= 0
  );
}

function validPosition(position: ProblemsPosition): boolean {
  return (
    Number.isSafeInteger(position.line) &&
    position.line >= 0 &&
    Number.isSafeInteger(position.character) &&
    position.character >= 0
  );
}

function severitySortValue(severity: number | undefined): number {
  return severity === 0 || severity === 1 || severity === 2 || severity === 3
    ? severity
    : 4;
}

function severityLabel(severity: number): string {
  return SEVERITY_LABELS[severity] ?? "Unknown";
}

function countSeverities(entries: readonly SnapshotEntry[]) {
  const counts = { error: 0, warning: 0, information: 0, hint: 0, unknown: 0 };
  for (const entry of entries) {
    const key = SEVERITY_COUNT_KEYS[entry.severity] ?? "unknown";
    counts[key] += 1;
  }
  return counts;
}
