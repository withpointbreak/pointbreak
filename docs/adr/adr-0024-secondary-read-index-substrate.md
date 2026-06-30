# ADR-0024: Secondary Read-Index Substrate (redb), Maintenance Model, and Containment

**Status:** Accepted (owner-approved 2026-06-29). **Decided; not yet implemented** — this ADR chooses the
substrate every future read-index must use and the model for maintaining/containing it, but it deliberately
builds nothing: no read-index exists in code, and the build trigger stays demand-gated and owner-gated
(ADR-0023 D7). Recording the substrate is **not** authorizing the build. It lands as a decision record; a
later read-index implementation effort will build the index on this substrate and update this status to
`landed via <that work>`. Reviewed through an independent `adr-review` pass (2 rounds) before approval.
**Date:** 2026-06-29
**See also:** **ADR-0023** (the *shape* this substrate must carry — the locator row, positions-not-bodies,
`eventSetHash`-gated disposability, private derived state; its **D7** defers exactly the substrate this ADR
now records), **ADR-0020** (its **D11** names the projection/secondary-index as a deferred *third* seam
"whose substrate is its own future ADR" — this is that ADR for the substrate dimension; its **D6**
earn-the-seam test and the three-layer journal/content/projection model), **ADR-0016** (content-targeted
removal — `compact` is the *single* physical-erasure surface; the index must never become a second one),
**ADR-0021** (server-side projection — the *computed* classification/DAG views the index *feeds*, never
duplicates), **ADR-0019** (blackboard liveness — no-runtime, pull-only). Grounding issues: **#212** (the
shape decision this builds on), **#215** (the cross-cutting consumers and their keys — the index's real
mandate), **#254** (the pagination cursor), **#135** (the validation-projection perf signal), **#206**
(the earn-the-seam test), **#202** (event-store partition-by-journal — couples to the per-partition marker
option), **#255** (the *separate*, index-independent inspector caching / single-pass-fold effort that fixes
today's measured lag).

## Context

Reads today are a full directory scan plus a JSON decode of every event, with no index, and the inspector
reuses those same projection paths per HTTP request — including a full-scan freshness probe on a 3-second
poll (`src/session/store/event_store.rs`, `src/cli/inspect/api.rs:949`). ADR-0023 pre-committed the *shape*
of any future read-index — one row per event (`eventId` + a re-read locator, `eventType`, `occurredAt`, the
full `EventTarget` triple, `actorId`, content-hash refs), holding **positions/keys, never bodies**,
`eventSetHash`-gated and disposable, private derived state — and **deliberately deferred the substrate and
the build trigger** (D7). ADR-0020 D11 had already named the projection/secondary-index seam as a deferred
third layer "whose substrate is its own future ADR." This ADR is that ADR for the **substrate, maintenance,
and containment** dimensions — and only the decision: it builds no index.

The shape ADR-0023 committed turned the index into an **OLTP locator** — point lookups, equality filters,
and an ordered `(occurredAt, eventId)` cursor over small keys — at small cardinality (the live store is
~1,842 events; projected scale is ~10k–100k, not the millions an analytical engine is built for). Three
standing constraints bound the substrate choice:

- **The inspector is deliberately no-runtime, thread-per-connection** — "one OS thread per connection …
  introduces no async runtime and no third-party HTTP crate" (`src/cli/inspect/server.rs`). A substrate
  whose handle is `Send + Sync` is shared cleanly across connection threads.
- **The shipped dependency path is pure-Rust** — the `shore` binary links no vendored-C `*-sys` crate and
  the repo has no `build.rs` (`Cargo.toml`, `edition = "2024"`, `rust-version = "1.95"`); the only native
  build-script in the lockfile (`cc`/`alloca`) is transitive through the `criterion` **dev/bench**
  dependency (`Cargo.toml` dev-dependencies), compiles nothing first-party, and is absent from the shipped
  binary. There has also been dedicated Windows-CI compile/spawn-cost work. The distinction is load-bearing
  for the substrate choice: redb keeps the **shipped** path pure-Rust, whereas a SQLite or DuckDB index
  would put a vendored-C(++) amalgamation into the **production** build (and, for DuckDB, onto the Windows
  cross-compile path) — a materially different exposure from a dev-only transitive shim.
- **There is no store-dir lock by design** — "Concurrency safety rests on content-addressed exclusive-create
  writes plus a regenerable atomic-rename projection: there is no store-dir lock" (`resolution.rs:117-119`).
  Anything that requires synchronous shared-index mutation fights this.

A reframe worth recording, because it changes *why* the index exists: there **is** measured, user-visible
read pain today — `/api/revisions` ≈ 11.7 s on the live store, bounding the inspector page load — but its
root cause is an **N+1 recompute** (a per-revision re-fold of the whole log), which a **separate,
index-independent** caching / single-pass-fold effort fixes with no index at all (#255). A locator narrows
*which* events each fold touches; it does **not** remove the fold. So the index's mandate is **not** today's
lag. Its mandate is the **future cross-cutting queries** — #215's by-actor view, the store-wide
open-input-request queue, and the by-content-hash view — and #254's `(occurredAt, eventId)` pagination
cursor, none of which has any store-wide read today.

## Decision

### D1. Substrate = redb (pure-Rust embedded KV); SQLite is the named fallback

Any future read-index is built on **redb** (a pure-Rust embedded key-value store; its documented design is
ACID, with copy-on-write B-trees, a stable on-disk file format, and a `Send + Sync` `Database` — see Prior
art and sources). The reasons, in order:

- **Pure-Rust, no C toolchain, no `build.rs`** — preserves the pure-Rust **shipped** build and adds **no**
  vendored native compile to it or the Windows-CI path (the same purity win a hand-rolled sidecar would
  have).
- **`Database` is `Send + Sync`** — the inspector's thread-per-connection server shares **one** handle
  read-only, rather than a connection-per-thread model with a per-request open cost.
- **The manual-secondary-index cost is bounded** — the ADR-0023 query set is **closed and pre-enumerated**
  (D2 there: new keys are additive and rare), and the index is **drop/rebuildable**, so a stale or buggy
  index is recovered by a rebuild from the log, not by a migration. That materially de-risks
  hand-maintaining redb's index tables.
- **`range()` over an ordered tuple key cleanly serves the `(occurredAt, eventId)` cursor** (ADR-0023 D5) —
  the one query shape that most distinguishes a real index from a scan.

**SQLite (`rusqlite`, bundled) is the named fallback.** It is the "Fossil-like" lean (Fossil is built on
SQLite) and wins the **query-model axis** outright: `CREATE INDEX` + `WHERE … ORDER BY … LIMIT` is less code
than redb's hand-maintained index tables, and new or ad-hoc queries cost no code. It **loses the dependency
axis** redb wins: the `rusqlite` bundled feature compiles a single C amalgamation (lighter than DuckDB's,
and the crate documents it as a good fit where linking is complicated such as Windows), but it still puts a
vendored-C compile into the **shipped** path the project has so far avoided. **Pick SQLite if** the query
set proves open-ended or the redb
hand-maintained-index cost balloons past expectation (Revisit Triggers).

### D2. DuckDB is rejected for this workload and scale

DuckDB — the lead candidate ADR-0020 D11 originally named — is **rejected**. ADR-0023 made the index an OLTP
point/range/cursor locator, the workload an OLAP columnar engine is **worst** at, at ~1.8k–100k events —
roughly four to five orders of magnitude below DuckDB's design point and below the KurrentDB
secondary-index precedent that motivated the lean (its published design indexes ~130 M events, one row per
event with positions not bodies, and mutates that DuckDB index **in place** — the opposite of this
drop-and-rebuild posture; see Prior art and sources). The `duckdb` crate's own documentation describes the
bundled build as a vendored-C++ amalgamation compiled from source, with cross-compilation (including
Windows) "best-effort … not covered by CI" and crates.io's 10 MB package limit forcing ICU out of the
bundled package — so adopting it would add the project's **first first-party native compile** and land it
squarely on the Windows-CI path the project has spent effort shaving. The analytical queries that once justified DuckDB (cross-revision observations, validation-over-
time, supersession-DAG traversal) are, under ADR-0023, **computed in the projection layer from located
events — not run as SQL in the index** (ADR-0023 D1 locator-not-cache, D6 consumers never address the index
directly). The locator framing undercut the OLAP rationale. Revisit **only** if a genuinely analytical
cross-cutting query (aggregation/grouping over the whole log) emerges — the original ADR-0020 D11 trigger.

### D3. Honest scope of the consolidation benefit

redb is already a proven, pinned dependency in the owner's adjacent **`shoreline-relay`** workspace (in its
`boardwalk` git submodule — a *separate* Cargo workspace, used by neither the relay's own crate nor
`shoreline`). But it is used there as a **full-scan KV-blob store**: no `range()`, no secondary-index
tables, no multimap; the one by-attribute lookup is a linear scan. So the consolidation benefit is **shared
engine + author expertise**, **not** a shared lockfile and **not** proven in-house index idioms. The
range-cursor plus the ~4–6 secondary-index / multimap tables the ADR-0023 query set needs are **net-new code
in either repo**. This is recorded so the substrate pick is not over-credited: redb wins on the dependency
and forward-compatibility axes (D1, D7), not because the index machinery already exists somewhere.

### D4. Maintenance model — separate "detect" from "confirm"; read-side lazy drop-and-rebuild

The per-read staleness **detector** is a **cheap monotonic append-marker** (the event-file count today; a
future additive `Journal::head_marker()` for true O(1) across backends) — **not** a recomputed
`eventSetHash`. `eventSetHash` stays the **stored gate token and the rebuild-time stamp**, recomputed only
when the marker says "changed" (i.e. only when a rebuild is already happening). On detected staleness the
index is **dropped and rebuilt to a temp location, then atomic-renamed** into place — the `state.json`
projection discipline — **never trusted, never repaired in place** (ADR-0023 D3).

This separation is load-bearing because the freshness token is itself O(n): `event_set_hash_for_events`
(`src/session/projection/freshness.rs:23`) consumes **already-decoded** events, so a literal "recompute the
hash to check freshness" *is* the full read + decode + rehash the index exists to avoid — exactly what the
3-second poll pays today.

The cheap marker has **identical coverage to `eventSetHash` for the change-detection job the design relies
on — API-level appends.** The `Journal` is strictly append-only with **no remove** (a content removal is
itself an appended `ArtifactRemoved` event — ADR-0016; `compact` touches `artifacts/`, never `events/`), so
every legitimate mutation adds an `events/` file: it bumps the marker *and* adds an `(eventId, payloadHash)`
pair that flips `eventSetHash`. For detecting real change — the actual job, the same role `eventSetHash`
already plays for the inspector poll and #255's cache — the marker is exactly as good.

Neither is a tamper detector (ADR-0023 D3 states this of `eventSetHash` itself). The honest limit, stated so
the marker is not over-credited: against an **out-of-band on-disk edit** the marker is *strictly weaker than
a per-read `eventSetHash` recompute*. `eventId` derives only from the idempotency key and `payloadHash` only
from the payload, and `validate_event` re-derives and checks both (`src/session/store/event_store.rs`), so a
coherent rewrite of an *unsigned* event's payload **plus** its `payloadHash` passes validation and **flips
`eventSetHash`** (a full recompute would catch it) yet leaves the file count unchanged — the marker misses
it; an *envelope-only* edit (`occurredAt` / `target` / `actorId` / `eventType`) is missed by **both**, since
it moves neither digest (ADR-0023 D3 scope). This gap is acceptable precisely because the design **does
not** use a per-read `eventSetHash` recompute as the detector — that O(n) cost is the thing being removed —
and no existing read path defends against hand-edited store files; if tamper-evidence over the indexed
columns is ever required, that is the deliberate future extension ADR-0023 already flags, not part of this
maintenance model.

**Synchronous incremental write-time INSERT is rejected for the file backend:** cross-process-safe
concurrent mutation of a *shared* index needs the store-dir lock ADR-0020 deliberately omits
(`resolution.rs:117-119`). Incremental append at all is in tension with "never repaired in place," needs an
additive `Journal` delta/cursor, and is substrate-dependent — **deferred to the build** as an explicit owner
decision (Revisit Triggers). The recommended model — read-side lazy, lock-free, drop-and-rebuild — is
**substrate-independent** (any substrate can be dropped and rebuilt), and the **same cheap marker is the
prerequisite** that lets the separate inspector caching effort (#255) skip the no-change scan too.

### D5. Containment — isolate-by-module; defer the `ProjectionStore` trait

Confine all substrate specifics — the redb dependency, its table definitions and `range()` queries, and the
`redb::Error → ShoreError` boundary — to a **single index module** mirroring the existing store-backend
module layout (`src/session/store/backend/`), exposing the ADR-0023 row as **domain types** (no `redb::*` in
any signature). Do **not** introduce a `ProjectionStore` trait yet: it would abstract over **zero**
implementations (no index exists — ADR-0023 D7), failing the same earn-the-seam test the project applies
everywhere (#206; ADR-0020 D6, and D11 which itself calls designing the projection seam now "premature").
Keep the module's functions **object-safe-shaped** so a later *second* substrate — or an in-memory index
test-double — can promote them to a trait **mechanically**: the "cheap insurance" posture without paying for
`dyn` over zero impls.

Containment is achievable as **module discipline rather than interface design** because the ADR-0023 shape
already bounds the blast radius: the index's *output* is a fixed domain locator row (D2 there), it holds no
bodies and re-reads through the already-backend-neutral `Journal`/`ContentStore` byte-traits
(`src/session/store/backend/mod.rs` — `read_event_bytes`, `list_event_entries`, `get`), and **consumers
read *through* a projection/command, never addressing the index directly** (ADR-0023 D6). Every consumer
already funnels through one read choke point, so an index attaches at a single boundary, not across the
hundreds of full-fold write-path and test sites.

### D6. The build trigger stays demand-gated and owner-gated

This ADR records the substrate; it does **not** authorize building an index. Per ADR-0023 D7 the build stays
owner-gated, and its justification is the **future cross-cutting queries** — #215's by-actor view, the
store-wide open-input-request queue, and the by-content-hash view — plus #254's `(occurredAt, eventId)`
pagination cursor: the capability with **no store-wide read today**. It is **not** justified by the current
`/api/revisions` lag, which is an N+1 recompute being fixed **separately and index-independently** (#255 plus
a single-pass store-wide fold). This ADR makes **no claim** that the index fixes that lag — a locator
narrows each fold but does not remove it. Build the index only when the #215/#254 demand lands; until then,
this substrate decision sits ready and unexercised.

### D7. Forward note (sketch, not a decision) — redb-now hedges toward a one-engine future

Recorded as a forward option, **not decided here**: if the diffable-truth requirement is ever dropped (the
owner's deferred reconsideration), a single redb engine could span **both** the truth layer and the index —
**one engine, two stores**, *not* a collapse of the three-layer model (truth stays authoritative and
durable; the index stays derived and disposable; the layering is the value, not the file count). Picking
redb now is **forward-compatible** with that consolidation at **no present cost** — the same engine,
transaction, and in-memory-twin idioms carry across both layers — whereas picking SQLite would force either
two engines or re-deciding the index substrate at consolidation time. This argues *for* redb-now even though
both truth-layer backends (a diffable NDJSON event log, object compression) stay deferred. The consolidation
itself is a separate future decision, opened only if/when diffability is dropped (Revisit Triggers).

## Consequences

### Accepted

- **A future read-index has a chosen substrate with no open substrate question** — when the #215/#254 demand
  lands, the build starts to the ADR-0023 shape without re-litigating redb-vs-SQLite-vs-DuckDB.
- **The pure-Rust / no-C-toolchain shipped build is preserved** (D1, D2) — no first-party native compile
  enters the production path, the Windows-CI cost work intact.
- **The inspector's thread-per-connection server shares one `Send + Sync` handle** (D1), avoiding a
  per-request open cost.
- **The hot-path O(n) re-scan is removed** by the detect-vs-confirm maintenance model (D4), which is
  substrate-independent and lock-free — fitting the no-store-dir-lock architecture — and whose cheap marker
  also benefits the separate inspector caching effort (#255).
- **Containment at a fraction of a trait's cost** (D5): a module wall plus the ADR-0023 shape, with a
  mechanical promotion path preserved for the day a second substrate is earned.
- **Costs accepted:** redb's secondary-index/multimap tables and tuple-keyed `range()` cursor are **net-new
  code** (D3), bounded by the closed query set and the drop/rebuild safety net; a marginal MSRV-pin
  maintenance for the redb dependency; and the first read after a change pays the rebuild (read-side lazy,
  D4).

### Rejected

- **DuckDB** — an OLAP columnar engine asked to serve an OLTP locator at ~1.8k–100k rows; its vendored-C++
  build is antagonistic to the pure-Rust + Windows-CI posture, and the analytical rationale was undercut by
  the ADR-0023 locator framing (D2).
- **A `ProjectionStore` trait now** — it would abstract over zero implementations; earn-the-seam fails
  (D5; #206, ADR-0020 D6/D11).
- **Recomputing `eventSetHash` as the per-read detector** — re-pays the very O(n) scan the index exists to
  avoid; the cheap append-marker is the detector, `eventSetHash` the stored stamp / rebuild-time confirm
  (D4).
- **Synchronous incremental write-time INSERT on the file backend** — needs the store-dir lock ADR-0020
  deliberately omits (D4).
- **`sled`** (effectively abandoned — last stable 2021) and **`polars`/`arrow`** (in-memory analytical
  DataFrames, heavy compile, wrong shape for a persistent point-lookup locator).
- **A flat hand-rolled sidecar** — redb dominates it: the same pure-Rust purity plus a crash-safe ACID
  B-tree and a `range()` cursor you would otherwise hand-roll, for marginally more than a dependency the
  owner already ships. Keep it only as a zero-dependency fallback if the query set is ever truly minimal.
- **Building the index now, or treating this substrate choice as build authorization** — the build stays
  demand-gated and owner-gated (D6, ADR-0023 D7). This ADR is substrate-only by design.

## Revisit Triggers

- **The build trigger fires** — a #215 cross-cutting query (by-actor / store-wide open-input-request queue /
  by-content-hash) or the #254 cursor lands, or the owner's event-count / p95-latency target is crossed →
  build the index on redb, honoring the ADR-0023 shape and the D4 maintenance model.
- **The query set proves open-ended, or the redb hand-maintained-index cost balloons** → reconsider SQLite,
  the D1 named fallback (its free SQL indexing then wins).
- **A genuinely analytical cross-cutting query** (aggregation/grouping over the whole log) emerges →
  reconsider DuckDB, the original ADR-0020 D11 trigger the locator framing undercut.
- **The owner sanctions an incremental-append fast path** → add the additive `Journal` delta/cursor and
  ratify it alongside D4's drop-rebuild (D4 defers it; it is in tension with "never repaired in place").
- **A second index substrate, or an in-memory index test-double, is genuinely wanted** → promote the D5
  module functions to the `ProjectionStore` trait (the earned seam; reconcile with ADR-0020 D11, with the
  ADR-0023 row as its contract).
- **The diffable-truth requirement is dropped** → open the truth+index consolidation (one redb engine, two
  stores) as its own decision — D7's forward option.
- **Event-store partition-by-journal (#202) lands** → reconsider whether the freshness marker and index
  rebuild should be per-partition rather than store-wide (D4's marker granularity).

## Prior art and sources

The substrate comparison and the rejection rest on external prior art; the load-bearing claims are sourced
here so the decision is auditable.

- **redb** (pure-Rust embedded KV; ACID, copy-on-write B-trees, stable file format, `Send + Sync`
  `Database`) — <https://github.com/cberner/redb>, <https://crates.io/crates/redb>. Already a pinned
  dependency in the owner's adjacent `shoreline-relay` workspace (its `boardwalk` submodule), used there as
  a full-scan KV-blob store — see D3.
- **SQLite / `rusqlite`** (bundled single-C-amalgamation compile; documented as a good fit where linking is
  complicated, such as Windows; pregenerated bindings, no build-time `bindgen`) —
  <https://github.com/rusqlite/rusqlite>.
- **DuckDB / `duckdb-rs`** (bundled vendored-C++ amalgamation compiled from source; cross-compilation
  "best-effort … not covered by CI"; crates.io 10 MB package limit forcing ICU out of the bundled package) —
  <https://github.com/duckdb/duckdb-rs>, <https://crates.io/crates/duckdb>.
- **KurrentDB secondary index** (the positions-not-bodies, disposable/rebuildable DuckDB `idx_all`
  precedent at ~130 M events, mutated in place) — <https://www.kurrent.io/blog/secondary-indexes/>,
  <https://docs.kurrent.io/server/v26.0/features/indexes/secondary>.
