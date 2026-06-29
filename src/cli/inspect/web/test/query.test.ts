import { describe, expect, it } from "vitest";
import { entryRevisionId, entryTrack } from "../src/projection";
import {
  buildHaystack,
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

describe("buildHaystack", () => {
  it("indexes the curated human fields, lowercased", () => {
    const obs = entryOfType("review_observation_recorded");
    const haystack = buildHaystack(obs);
    expect(haystack).toBe(haystack.toLowerCase());
    expect(haystack).toContain("observed change");
    expect(haystack).toContain("agent:codex");
  });

  it("indexes the validation check name", () => {
    expect(buildHaystack(entryOfType("validation_check_recorded"))).toContain(
      "cargo test",
    );
  });

  it("indexes body and tags but not unindexed wire fields like journalId", () => {
    const haystack = buildHaystack({
      eventType: "review_observation_recorded",
      eventId: "evt:1",
      trackId: "agent:x",
      summary: { title: "T", body: "Body Text", tags: ["Alpha", "Beta"] },
    });
    expect(haystack).toContain("body text");
    expect(haystack).toContain("alpha");
    expect(haystack).toContain("beta");
    // The curated haystack is not a whole-event stringify: journalId is not in it.
    expect(
      buildHaystack(entryOfType("review_observation_recorded")),
    ).not.toContain("journal");
  });
});

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
    expect(fieldMatches(idx, "object", "anything")).toBe(false);
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

describe("query grammar over a real entry index", () => {
  it("filters a fixture observation by its projected fields and haystack", () => {
    const obs = entryOfType("review_observation_recorded");
    const idx: SearchIndex = {
      text: buildHaystack(obs),
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
