// Structured query grammar: the tokenizer, the field:value + free-text parser,
// and the field/predicate matchers, still used by the revisions lens.
// Ported from the served app.js query cluster. All pure (argument-driven, no DOM,
// no state).
//
// The timeline history query (its haystack, filter predicates, and facet counts)
// moved to the server; the client no longer indexes or searches history entries.
// The revisions lens keeps matching client-side over the fully-loaded list, so
// this grammar stays. Import direction stays downward.

import { QUERY_FIELDS, type SearchIndex, TYPES } from "./types";

/** A parsed query clause: a field:value equality or a free-text term, negatable. */
export type QueryClause =
  | { kind: "field"; field: string; value: string; negate: boolean }
  | { kind: "text"; value: string; negate: boolean };

// Split a query into tokens, honoring "quoted phrases" (optionally negated /
// field-prefixed) and bare runs.
/** Tokenize a raw query string into its words and quoted phrases. */
export function tokenizeQuery(q: string): string[] {
  const out: string[] = [];
  const re = /-?(?:[a-z]+:)?"[^"]*"|\S+/gi;
  let m = re.exec(q);
  while (m !== null) {
    out.push(m[0]);
    m = re.exec(q);
  }
  return out;
}

// Parse a query string into clauses. A `field:value` whose field is a recognized
// grammar field is a field clause; everything else is a free-text clause.
/** Parse a raw query string into negatable field and free-text clauses. */
export function parseSearchQuery(q: string): QueryClause[] {
  const clauses: QueryClause[] = [];
  for (let tok of tokenizeQuery(q || "")) {
    let negate = false;
    if (tok.length > 1 && tok[0] === "-") {
      negate = true;
      tok = tok.slice(1);
    }
    const colon = tok.indexOf(":");
    let field = colon > 0 ? tok.slice(0, colon).toLowerCase() : "";
    // Legacy `object:` aliases to the renamed `snapshot` field (#334 transition);
    // the token is user-typed, so old queries keep working.
    if (field === "object") field = "snapshot";
    if (field && QUERY_FIELDS.includes(field)) {
      // The value is matched as a substring of the stored field so short ids work.
      const raw = tok.slice(colon + 1).replace(/^"|"$/g, "");
      clauses.push({ kind: "field", field, value: raw.toLowerCase(), negate });
    } else {
      const term = tok.replace(/^"|"$/g, "").toLowerCase();
      if (term) clauses.push({ kind: "text", value: term, negate });
    }
  }
  return clauses;
}

/** Match a single field clause against a search record. */
export function fieldMatches(
  idx: SearchIndex,
  field: string,
  value: string,
): boolean {
  if (field === "type") {
    // Accept the human label (e.g. "observation") or the raw event-type id.
    const known = TYPES.find((t) => t.label === value || t.id === value);
    return idx.type === (known ? known.id : value);
  }
  return (idx[field] || "").toLowerCase().includes(value);
}

/** Match a search record against every clause (AND, honoring negation). */
export function matchesQuery(
  idx: SearchIndex,
  clauses: QueryClause[],
): boolean {
  for (const c of clauses) {
    const hit =
      c.kind === "field"
        ? fieldMatches(idx, c.field, c.value)
        : idx.text.includes(c.value);
    if (c.negate ? hit : !hit) return false;
  }
  return true;
}
