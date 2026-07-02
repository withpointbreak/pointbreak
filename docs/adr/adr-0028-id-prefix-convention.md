# ADR-0028: ID Prefix Convention — Ratify Minted Strings, Internal Registry as the Enumerable Source

**Status:** Proposed — ratified by the owner landing the registry change for #162.
**Date:** 2026-07-02
**See also:** **ADR-0017** (identity layering — the object/revision split whose ids these
prefixes open), **ADR-0018** (event-borne supersession — the reshape that retired the
`review-unit` lineage vocabulary this ADR now records as legacy), `docs/store-migration.md`
(what a content-id re-derivation costs; the reason renaming a prefix is a store break, not an
edit), and the "IDs are opaque" section of `docs/review-workflow.md` (the external contract this
ADR reaffirms). Grounding issue: **#162**. Display-membership follow-up: **#344**.

## Context

Shoreline's prefixed ids (`evt:sha256:…`, `obs:sha256:…`, `input-request:sha256:…`) were minted
as inline `format!` literals at each construction site. There was no canonical registry, no
recorded rationale for any individual prefix choice, and the closest thing to a complete
enumeration was the inspector's reference-detection regex — UI code kept in sync by hand, in two
parallel copies.

The de-facto pattern the choices had converged on: a small set of core content primitives
abbreviated (`evt`, `rev`, `obj`, `obs`, `assess`), everything else spelled out and hyphenated
(`engagement`, `input-request`, `input-request-response`, `task-attempt`, `assoc-commit`, …).
`review-unit` and `snap` survive only as inspector display entries for ids embedded in older
stores' fact bodies; production has minted `rev`/`obj` since the supersession reshape.

Two facts make the strings expensive to change:

- **Prefix strings feed content-derived ids.** Two clones capturing identical content converge
  on the same revision/object/observation ids only because they mint byte-identical strings.
  Renaming `evt` to `event` would fork every newly minted id away from every existing store.
- **Stores in the wild already carry the current strings** — dogfood stores and the shared
  fixture stores. Convergence with them is a live property, not a hypothetical.

Externally, none of this is contract: `docs/review-workflow.md` already requires consumers to
treat ids as opaque strings. This ADR is about internal consistency and discoverability only.

## Decision

1. **Ratify every currently minted prefix string as-is, and freeze it.** The authoritative
   enumeration is the registry in `src/model/id_prefix.rs`; its frozen-value test pins each
   constant to today's literal, so changing one is a deliberate, test-breaking act.
2. **The abbreviation set is closed.** New prefixes are spelled-out, hyphenated, lowercase
   domain names matching `[a-z][a-z-]*` with no trailing hyphen. No new abbreviations; the
   existing ones (`evt`, `rev`, `obj`, `obs`, `assess`, plus legacy `snap`) are grandfathered,
   not precedent.
3. **The registry is the single enumerable source, and it stays internal.** Production code
   mints through the registry's `pub(crate)` constants; the registry is never re-exported from
   the library's public surface, and prefixes remain opaque to consumers. `docs/id-prefixes.md`
   is the contributor-facing companion with the add-a-prefix checklist.
4. **The inspector's linkification list derives, it is not hand-maintained.** One web-side list
   (`REF_ID_PREFIXES` in `classNames.ts`) derives both `REF_KINDS` and the `REF_RE` regex; the
   registry mirrors it via per-entry `linkified` flags, and a drift test in the cargo gate fails
   when the two diverge. Changing what the inspector linkifies is a display decision made
   deliberately — tracked in #344, which inventories the current membership and shape gaps.
5. **Normalizing existing strings is rejected as a default.** If spelled-out names are ever
   wanted for the abbreviated five, that is an owner-gated store-format migration — re-derive
   content ids under a convergence gate per `docs/store-migration.md`, in the same class as the
   #327-gated breaks — never a rename in place.

## Consequences

- One place answers "what prefixes exist": the registry table, with the doc table as a readable
  snapshot. Adding an id type has a checklist instead of an improvisation.
- Drift between the Rust registry and the inspector's regex fails `just test`; drift between
  the derived regex and its intended bytes fails the web suite's alternation lock.
- Published-crate tarballs skip the drift test (the package excludes `src/cli/inspect/web/**`);
  the guard is meaningful only in the repo, and says so when it skips.
- `review-unit` and `snap` remain linkified for old fact bodies until #344 decides otherwise;
  the registry marks them `minted: false` so the honesty guards keep them justified.
- Two const-declared sentinel values (`hunk:stale`, `hunk:orphaned` in `src/stream/build.rs`)
  sit outside the registry: `HunkId` is path-based and prefix-free by design, these are
  reserved values rather than a prefix family, and a `const` cannot be built from a registry
  constant without moving it to runtime construction. They are documented in
  `docs/id-prefixes.md` instead.
