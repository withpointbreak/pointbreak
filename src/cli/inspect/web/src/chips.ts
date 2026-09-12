// Applied-filter chip derivation and clause minting: a pure view over
// `filterText` (argument-driven, no DOM, no state — the shape of query.ts). The
// surface-aware parser is the only key-set and canonicalization authority and
// `tokenizeQuery` the only tokenization authority; this module never re-lists
// keys or reimplements quoting/negation splitting, and when it mints a clause it
// asks the parser whether an equivalent clause already filters.

import { parseSearchQueryFor, type QuerySurface, tokenizeQuery } from "./query";

/** One removable chip: a parsed field clause plus the index of the raw token
 * that produced it in `tokenizeQuery(filterText)`. */
export interface FilterChip {
  tokenIndex: number;
  field: string;
  value: string;
  negate: boolean;
}

/**
 * The field-clause chips derived from `filterText` for `surface` — one per raw
 * token that parses as a supported field clause; a free-text token, or a clause
 * the surface parse drops with a diagnostic, produces no chip. Re-parsing each
 * token individually (rather than diffing the whole-string parse against its
 * clause list) keeps a repeated key's occurrences distinct: two `tag:a` tokens
 * are two chips, each carrying its own `tokenIndex`, so removing the second one
 * never touches the first.
 */
export function filterChipsFor(
  filterText: string,
  surface: QuerySurface,
): FilterChip[] {
  const chips: FilterChip[] = [];
  tokenizeQuery(filterText).forEach((raw, tokenIndex) => {
    const clause = parseSearchQueryFor(raw, surface).clauses[0];
    if (clause && clause.kind === "field") {
      chips.push({
        tokenIndex,
        field: clause.field,
        value: clause.value,
        negate: clause.negate,
      });
    }
  });
  return chips;
}

/** Remove the token at `tokenIndex` from `filterText`, preserving every other
 * token (free text and other qualifier clauses) in order. */
export function removeFilterChipToken(
  filterText: string,
  tokenIndex: number,
): string {
  const tokens = tokenizeQuery(filterText);
  tokens.splice(tokenIndex, 1);
  return tokens.join(" ");
}

/**
 * Append an `actor:<id>` clause to `filterText`, preserving every existing clause.
 *
 * The actor is a query clause, never a scope param. The clause mints the short
 * form because the parser canonicalizes `agent:x` and `actor:agent:x` to the same
 * value, and quotes a whitespace-bearing id so it tokenizes as one field token. A
 * repeated call is a no-op: the candidate clause and the existing query are
 * compared through the parser, so it is the parser — not this module — that
 * decides what "the same actor" means.
 */
export function appendActorFilterClause(
  filterText: string,
  actorId: string,
  surface: QuerySurface,
): string {
  const current = filterText.trim();
  const short = actorId.replace(/^actor:/, "");
  // The phrase form cannot carry a literal quote, so an id containing one has no
  // representation in this grammar; leave the query untouched rather than emit a
  // token that would re-tokenize as something else.
  if (!short || short.includes('"')) return current;
  const clause = /\s/.test(short) ? `actor:"${short}"` : `actor:${short}`;
  const minted = parseSearchQueryFor(clause, surface).clauses[0];
  if (minted?.kind !== "field" || minted.field !== "actor") return current;
  const already = parseSearchQueryFor(current, surface).clauses.some(
    (existing) =>
      existing.kind === "field" &&
      existing.field === "actor" &&
      !existing.negate &&
      existing.value === minted.value,
  );
  if (already) return current;
  return current ? `${current} ${clause}` : clause;
}
