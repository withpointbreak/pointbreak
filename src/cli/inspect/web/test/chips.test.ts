import { describe, expect, it } from "vitest";
import {
  appendActorFilterClause,
  filterChipsFor,
  removeFilterChipToken,
} from "../src/chips";
import { parseSearchQueryFor } from "../src/query";

// The applied-filter chips are a pure view of `filterText`: one chip per raw
// token that parses as a supported field clause on the active surface, each
// carrying the index of the token that produced it so removal deletes exactly
// that occurrence. Free text never becomes a chip; key-set membership is
// entirely delegated to the surface-aware parser (never re-listed here).

describe("filterChipsFor", () => {
  it("returns one chip per parsed field clause, in token order", () => {
    const chips = filterChipsFor(
      "type:observation pinned track:codex",
      "event",
    );
    expect(chips.map((c) => c.field)).toEqual(["type", "track"]);
  });

  it("produces no chip for a free-text term", () => {
    expect(filterChipsFor("pinned", "event")).toEqual([]);
  });

  it("keeps duplicate-key clauses as distinct chips, one per exact token occurrence", () => {
    const chips = filterChipsFor("tag:a pinned tag:a", "event");
    expect(chips).toHaveLength(2);
    expect(chips[0]).toMatchObject({ field: "tag", value: "a", tokenIndex: 0 });
    expect(chips[1]).toMatchObject({ field: "tag", value: "a", tokenIndex: 2 });
  });

  it("marks a negated clause's chip", () => {
    // Use a non-aliased key: `status:` parses to the aliased field `check:`,
    // so a `-status:` chip would carry field "check", not "status".
    const chips = filterChipsFor("-check:failed", "event");
    expect(chips[0]).toMatchObject({
      field: "check",
      value: "failed",
      negate: true,
    });
  });

  it("resolves keys against the given surface, using each surface's key set", () => {
    // `actor:` is a member of both surfaces' key sets — this pins that chip
    // derivation calls the surface-aware parser rather than hardcoding one
    // surface's keys, even where the two surfaces happen to agree.
    const eventChips = filterChipsFor("actor:codex", "event");
    const revisionChips = filterChipsFor("actor:codex", "revision");
    expect(revisionChips.map((c) => c.field)).toContain("actor");
    expect(eventChips.map((c) => c.field)).toContain("actor");
  });

  it("carries the parser's canonical value, whichever actor spelling was typed", () => {
    // The parser canonicalizes a prefix-less actor value to the stored full id;
    // both spellings of the same id yield the same chip value.
    const short = filterChipsFor("actor:agent:codex", "event");
    const full = filterChipsFor("actor:actor:agent:codex", "event");
    expect(short[0]?.value).toBe(full[0]?.value);
  });

  it("produces no chip for a clause the surface parse drops with a diagnostic", () => {
    // `type:` is known but unsupported on the revision surface: the parser
    // drops the clause (never silent-empty), so no chip claims it is active.
    expect(filterChipsFor("type:observation", "revision")).toEqual([]);
  });
});

describe("removeFilterChipToken", () => {
  it("deletes exactly the token at the given index, preserving the rest in order", () => {
    expect(removeFilterChipToken("tag:a pinned tag:a", 2)).toBe("tag:a pinned");
    expect(removeFilterChipToken("tag:a pinned tag:a", 0)).toBe("pinned tag:a");
  });

  it("keeps a whitespace-bearing quoted clause intact as one token", () => {
    // The tokenizer, not this module, owns quoting: the quoted actor clause is
    // one token, so deleting its neighbor never splits it.
    expect(
      removeFilterChipToken('actor:"git-name:Kevin Swiber" tag:a', 1),
    ).toBe('actor:"git-name:Kevin Swiber"');
  });
});

// Clause minting is the other half of the pure view: the row's writer click
// appends an `actor:<id>` clause, and the parser — never this suite or the
// module — decides whether two spellings name the same actor.
describe("appendActorFilterClause", () => {
  it("appends an actor clause without repeating the actor: prefix", () => {
    expect(
      appendActorFilterClause(
        "type:observation",
        "actor:agent:codex-loop",
        "change-timeline",
      ),
    ).toBe("type:observation actor:agent:codex-loop");
  });

  it("quotes a whitespace-bearing id so the clause survives tokenization", () => {
    const next = appendActorFilterClause(
      "",
      "actor:git-name:Kevin Swiber",
      "change-timeline",
    );
    expect(next).toBe('actor:"git-name:Kevin Swiber"');
    expect(parseSearchQueryFor(next, "change-timeline").clauses).toEqual([
      {
        kind: "field",
        field: "actor",
        value: "actor:git-name:kevin swiber",
        negate: false,
      },
    ]);
  });

  it("is a no-op when the same actor already filters, in any spelling", () => {
    const once = appendActorFilterClause(
      "",
      "actor:agent:codex-loop",
      "change-timeline",
    );
    expect(
      appendActorFilterClause(
        once,
        "actor:agent:codex-loop",
        "change-timeline",
      ),
    ).toBe(once);
    expect(
      appendActorFilterClause(
        "actor:actor:agent:codex-loop",
        "actor:agent:codex-loop",
        "change-timeline",
      ),
    ).toBe("actor:actor:agent:codex-loop");
  });

  it("is a no-op for a repeated did:key actor", () => {
    const did = "did:key:z6MkehRgf7yJbgaGfYsdoAsKdBPE3dj2CYhowQdcjqSJgvVd";
    const once = appendActorFilterClause("", did, "change-timeline");
    expect(once).toBe(`actor:${did}`);
    expect(appendActorFilterClause(once, did, "change-timeline")).toBe(once);
  });

  it("still appends when the existing actor clause is negated", () => {
    expect(
      appendActorFilterClause(
        "-actor:agent:codex-loop",
        "actor:agent:codex-loop",
        "change-timeline",
      ),
    ).toBe("-actor:agent:codex-loop actor:agent:codex-loop");
  });

  it("returns the query unchanged for an empty id or an id the grammar cannot express", () => {
    expect(
      appendActorFilterClause("type:observation", "", "change-timeline"),
    ).toBe("type:observation");
    expect(
      appendActorFilterClause("", 'actor:weird"quote', "change-timeline"),
    ).toBe("");
  });

  it("preserves free text and other clauses in order", () => {
    expect(
      appendActorFilterClause(
        "rebase track:author",
        "actor:agent:codex",
        "change-timeline",
      ),
    ).toBe("rebase track:author actor:agent:codex");
  });
});
