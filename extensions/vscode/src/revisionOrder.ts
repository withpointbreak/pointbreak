import type { RevisionListDoc } from "./cli";

type RevisionEntry = RevisionListDoc["entries"][number];

export function newestRevisionEntries<T extends RevisionEntry>(
  entries: readonly T[],
): T[] {
  return entries
    .map((entry, index) => ({ entry, index, time: capturedAtMillis(entry) }))
    .sort((left, right) => right.time - left.time || right.index - left.index)
    .map(({ entry }) => entry);
}

function capturedAtMillis(entry: RevisionEntry): number {
  const unixMillis = entry.capturedAt.match(/^unix-ms:([+-]?\d+)$/);
  const value = unixMillis
    ? Number(unixMillis[1])
    : Date.parse(entry.capturedAt);
  return Number.isFinite(value) ? value : Number.NEGATIVE_INFINITY;
}
