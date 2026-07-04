import { describe, expect, it } from "vitest";
import { entryRevisionId, entryTrack } from "../src/projection";
import {
  fieldMatches,
  matchesQuery,
  parseSearchQuery,
  tokenizeQuery,
} from "../src/query";
import type { HistoryEntry, SearchIndex } from "../src/types";
import historyJson from "./fixtures/history.json";

const history = historyJson as unknown as { entries: HistoryEntry[] };

function entryOfType(type: string): HistoryEntry {
  const found = history.entries.find((e) => e.eventType === type);
  if (!found) throw new Error(`no ${type} entry in fixture`);
  return found;
}

describe("tokenizeQuery", () => {
  it("splits on whitespace", () => {
    expect(tokenizeQuery("foo bar")).toEqual(["foo", "bar"]);
    expect(tokenizeQuery("")).toEqual([]);
  });

  it("keeps quoted phrases — bare, negated, and field-prefixed — intact", () => {
    expect(tokenizeQuery('type:observation "quoted phrase"')).toEqual([
      "type:observation",
      '"quoted phrase"',
    ]);
    expect(tokenizeQuery('-"neg phrase"')).toEqual(['-"neg phrase"']);
    expect(tokenizeQuery('track:"agent codex"')).toEqual([
      'track:"agent codex"',
    ]);
    expect(tokenizeQuery("-status:failed")).toEqual(["-status:failed"]);
  });
});

describe("parseSearchQuery", () => {
  it("parses a known field:value clause, lowercasing field and value", () => {
    expect(parseSearchQuery("TYPE:Observation")).toEqual([
      { kind: "field", field: "type", value: "observation", negate: false },
    ]);
  });

  it("marks a leading - as negation", () => {
    expect(parseSearchQuery("-status:failed")).toEqual([
      { kind: "field", field: "status", value: "failed", negate: true },
    ]);
    expect(parseSearchQuery("-observed")).toEqual([
      { kind: "text", value: "observed", negate: true },
    ]);
  });

  it("strips quotes from field values and free-text phrases", () => {
    expect(parseSearchQuery('track:"agent:codex"')).toEqual([
      { kind: "field", field: "track", value: "agent:codex", negate: false },
    ]);
    expect(parseSearchQuery('"quoted phrase"')).toEqual([
      { kind: "text", value: "quoted phrase", negate: false },
    ]);
  });

  it("treats an unrecognized field as free text", () => {
    expect(parseSearchQuery("nope:value")).toEqual([
      { kind: "text", value: "nope:value", negate: false },
    ]);
  });

  it("splits bare terms into free-text clauses", () => {
    expect(parseSearchQuery("free text")).toEqual([
      { kind: "text", value: "free", negate: false },
      { kind: "text", value: "text", negate: false },
    ]);
    expect(parseSearchQuery("")).toEqual([]);
  });
});

describe("fieldMatches", () => {
  const idx: SearchIndex = {
    text: "haystack",
    type: "review_observation_recorded",
    track: "agent:codex",
    revision: "rev:sha256:abcdef",
  };

  it("matches the type field by human label or raw id, exactly", () => {
    expect(fieldMatches(idx, "type", "observation")).toBe(true);
    expect(fieldMatches(idx, "type", "review_observation_recorded")).toBe(true);
    expect(fieldMatches(idx, "type", "assessment")).toBe(false);
  });

  it("matches non-type fields as a substring of the lowercased index", () => {
    // The index field is lowercased; the value is matched verbatim (the parser
    // lowercases values before they reach here), so short ids work.
    expect(fieldMatches(idx, "revision", "abcd")).toBe(true);
    expect(fieldMatches(idx, "track", "codex")).toBe(true);
    expect(fieldMatches(idx, "revision", "zzz")).toBe(false);
  });

  it("treats a missing field as no match", () => {
    expect(fieldMatches(idx, "snapshot", "anything")).toBe(false);
  });
});

describe("matchesQuery", () => {
  const idx: SearchIndex = {
    text: "observed change in src/lib.rs",
    type: "review_observation_recorded",
    track: "agent:codex",
  };

  it("matches when there are no clauses", () => {
    expect(matchesQuery(idx, [])).toBe(true);
  });

  it("ANDs field and free-text clauses", () => {
    expect(
      matchesQuery(idx, parseSearchQuery("type:observation observed")),
    ).toBe(true);
    expect(
      matchesQuery(idx, parseSearchQuery("type:observation missing")),
    ).toBe(false);
  });

  it("honors negation", () => {
    expect(matchesQuery(idx, parseSearchQuery("-observed"))).toBe(false);
    expect(matchesQuery(idx, parseSearchQuery("-missing"))).toBe(true);
  });
});

describe("query grammar over a real entry's projected fields", () => {
  it("filters a fixture observation by its projected fields and free text", () => {
    // The haystack is now built server-side; the client grammar still matches the
    // projected fields (type/track/revision) and a lowercased free-text haystack.
    const obs = entryOfType("review_observation_recorded");
    const idx: SearchIndex = {
      text: "observed change in src/lib.rs",
      type: obs.eventType,
      track: entryTrack(obs),
      revision: entryRevisionId(obs),
    };
    expect(matchesQuery(idx, parseSearchQuery("type:observation"))).toBe(true);
    expect(matchesQuery(idx, parseSearchQuery("track:codex"))).toBe(true);
    expect(matchesQuery(idx, parseSearchQuery("type:assessment"))).toBe(false);
    expect(matchesQuery(idx, parseSearchQuery("observed"))).toBe(true);
  });
});
