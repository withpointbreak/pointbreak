import { expect, it } from "vitest";
import type { RevisionListDoc } from "../src/cli";
import { newestRevisionEntries } from "../src/revisionOrder";

type RevisionEntry = RevisionListDoc["entries"][number];

it("orders mixed captured-at formats newest-first without mutating the source", () => {
  const entries = [
    entry("oldest", "2026-07-12T00:00:00.000Z"),
    entry("newest", `unix-ms:${Date.parse("2026-07-15T00:00:00.000Z")}`),
    entry("middle", "2026-07-14T00:00:00.000Z"),
  ];

  expect(
    newestRevisionEntries(entries).map(({ revisionId }) => revisionId),
  ).toEqual(["rev:sha256:newest", "rev:sha256:middle", "rev:sha256:oldest"]);
  expect(entries.map(({ revisionId }) => revisionId)).toEqual([
    "rev:sha256:oldest",
    "rev:sha256:newest",
    "rev:sha256:middle",
  ]);
});

function entry(revisionId: string, capturedAt: string): RevisionEntry {
  return {
    revisionId: `rev:sha256:${revisionId}`,
    capturedAt,
    mergeStatus: "open",
  };
}
