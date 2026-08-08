import type { AuthorityCursorV2 } from "../../src/change-protocol";

export function authorityCursor(eventCount = 0): AuthorityCursorV2 {
  const hash = (seed: number) =>
    `sha256:${seed.toString(16).padStart(64, "0")}`;
  return {
    schema: "pointbreak.authority-cursor.v2",
    journalRecordCount: eventCount,
    eventCount,
    journalRecordSetHash: hash(eventCount + 1),
    eventSetHash: hash(eventCount + 2),
    capabilitySetHash: hash(eventCount + 3),
  };
}
