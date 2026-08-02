# ADR-0023: Pre-Committed Shape for the Secondary Read-Index (Positions/Keys, Never Bodies)

**Status:** Accepted (owner-approved 2026-06-29). **Decision recorded; not yet implemented** — this ADR
pre-commits a *shape* and deliberately builds nothing: no read-index exists in code, and the substrate and
build trigger are intentionally deferred (Decision D7). It lands as a decision record ahead of any
implementation; a later read-index implementation effort will build the index to this shape and update this
status to `landed via <that work>`. Reviewed through an independent `adr-review` pass (3 rounds) before
approval. Tracked by issue #212.
**Date:** 2026-06-29
**See also:** Relates to **ADR-0020** (durable-storage seam — its **D11** names this projection/secondary-index
as a *deferred third seam* whose substrate is "its own future ADR," and its **D4** establishes that on-disk
listing order is hash-sorted, **not** causal — the gap an index fills), **ADR-0016** (content-targeted
removal — `compact` physically erases blobs, the *single* erasure surface; the journal is append-only),
**ADR-0021** (inspector server-side projection — the *computed* classification/DAG views the index *feeds*,
never duplicates), **ADR-0019** (blackboard liveness — no-runtime, pull-only). Grounding issues:
**#212** (this decision), **#215** (cross-cutting views — the consumers and their keys), **#254** (pagination
cursor), **#135** (validation projection linear scan). Architectural framing: the store is **Fossil-like** —
canonical immutable facts plus rebuildable projections/indexes, each carrying an `eventSetHash`.

## Context

Reads today are a full directory scan plus a JSON decode of every event, with no index. `EventStore`
lists every file under `events/` and decodes each one, and the inspector reuses those same projection
paths per HTTP request, including a full-scan freshness probe (`src/session/store/event_store.rs`,
`src/cli/inspect/api.rs`). `state.json` is a bounded *summary* projection (counts, cursors,
`eventSetHash`), not a queryable index. This is fine at the current local-first scale and is the explicit
"not yet."

The architecture is deliberately Fossil-like: the durable truth layer is plain, append-only, content-
addressed files, and every higher read structure (`state.json`, ledger/history documents, full revision
projections, and any future read index) is **derived state** — safe to delete and rebuild from the
canonical events/artifacts, carrying an `eventSetHash` derived from sorted `(eventId, payloadHash)` pairs.
ADR-0020 D11 already deferred the *projection / secondary-index seam* as its own future decision, naming a
concrete trip-wire (a slow read path or an analytical query the on-the-fly reducer serves poorly) and
candidate consumers (cross-revision observations, validation-over-time, supersession-DAG traversal).

This ADR does **not** reopen that deferral or build an index. It does the one thing that is cheap now and
expensive to retrofit later: it **pre-commits the *shape* of any future read-index** so the deferred build
cannot drift into a privacy or consistency hazard. The shape is grounded in the consumers that are already
specified:

- **#254** (pagination) needs a stable cursor; its proposed key is `(occurred_at, event_id)`.
- **#215** (cross-cutting views) independently specifies the *same* index discipline and names the queries
  that have no store-wide read today: a **by-actor** view (and notes `track ≠ actor` — actor lives in the
  envelope), a **store-wide open-input-request queue** (`eventType` + target), and an **events-touching-
  content-hash** view (content-hash refs).
- **#135** (validation projection) is the perf signal that may trip the build trigger; it keys on
  `eventType` + target.
- **ADR-0021**'s supersession-DAG and per-revision classification are *computed* from the located events;
  the index helps *find* them (by `eventType` + target) but must not *store* the verdicts.

The precedent is KurrentDB's secondary index (its DuckDB `idx_all`): one row per event with positions/keys
and **no body column**, re-reading the log for bodies at query time. We adopt that *discipline*, not that
*substrate*.

## Decision

### D1. The index is a locator, not a projection cache

The read-index maps **small keys → a per-event locator** (a pointer to the event in the log — D2; never the
event's body). It holds **no event bodies and no derived/semantic state** — no classifications, no
supersession verdicts, no coverage/attention state, no payloads. Bodies and all projections are re-derived by
re-reading the located event files and folding them through the existing projections. This is the line that
separates the index from `state.json` (a summary projection) and from ADR-0021's server-side views (computed
classifications): those are *derived answers*; the index is only a *locator for inputs*.

### D2. The pre-committed row: logical address + small keys + content-hash refs, never bodies

One row per event carries exactly:

- the **event identity + a re-read locator** — the row carries the event's stable `eventId`
  (`evt:sha256:<keyDigest>`; used as the D5 cursor tiebreak and the D3 `eventSetHash` set member) **and** a
  locator that re-reads the event's bytes through the `Journal`. The shape commitment is that this is a
  **pointer, never the body**. The current `Journal` point-read is keyed by the **raw `idempotencyKey`**
  (`read_event_bytes(idempotencyKey)`, `src/session/store/backend/mod.rs`) — which the decoded event already
  carries — so the row stores the `idempotencyKey` as that locator, *or* the build adds a backend-neutral
  read-by-`eventId` / digest `Journal` method (a future additive method, ADR-0020 D11); which one is a
  substrate detail deferred to D7. The index **never stores a file path**: the file backend's
  `events/<keyDigest>.json` mapping (`keyDigest = sha256(idempotencyKey)`) is backend-owned (ADR-0020 D1/D2).
  This is the analog of KurrentDB's `log_position`: the index points at the event; the log stays truth.
- `eventType`;
- `occurredAt`;
- the **target** — the full `EventTarget` triple: `journalId`, `subject`, and the optional `trackId`
  (`src/session/event/target.rs`). `trackId` is load-bearing, not coarse: #215's open-input-request queue and
  #135's validation reads are **track-scoped**, so the row must carry the track, not just `journalId` +
  `subject`.
- `actorId` — `event.writer.actorId` (`src/session/event/writer.rs`), the actor identity. This is distinct
  from `writer.producer` (the tool name/version), which is **not** indexed unless a future consumer needs it,
  and from `track`, which is a review lane, not an actor (#215).
- the **referenced content-hashes** — the blob refs the event points at (never the blob *contents*).

Everything else (body, payload, signatures) is re-read from the event file via its address at query time.
This key set is the union of what the named consumers require (#254 `(occurredAt, eventId)`; #215 by-actor /
open-queue / by-content-hash; #135 validation) — it is consumer-grounded, not speculative. New keys are an
additive extension (Revisit Triggers), not a reshape.

### D3. `eventSetHash`-gated freshness; the index is disposable derived state

The index carries the `eventSetHash` (a digest over the sorted set of `(eventId, payloadHash)` pairs — the
existing liveness token, `src/session/projection/freshness.rs`). On read, an index whose `eventSetHash` ≠ a
fresh scan's is **stale → dropped and rebuilt, never trusted**. The index is never authoritative and never
repaired in place; it is always safe to delete and rebuild from the canonical log. This reuses the exact
primitive the inspector already polls as its change signal (and the same key #255's projection cache uses).

This gate is sufficient for the store's own operations **even though `eventSetHash` does not digest the
indexed columns** (`eventType`, `occurredAt`, `target`, `actorId`), and the index needs no separate per-column
hash. The basis is the **append-only API contract**, not signatures (which are optional —
`ShoreEvent.signer` / `signature` are `Option`, `src/session/event/mod.rs`): the store exposes no in-place
event edit. Its only mutations are an **append** (adds an `(eventId, payloadHash)` pair → flips the hash) and
a **content removal**, which is itself an appended `ArtifactRemoved` event (ADR-0016) that adds a pair and
triggers a rebuild (a one-shot store migration likewise rebuilds the whole set and re-derives the hash). So
every legitimate change to an indexed column travels through an append that flips `eventSetHash`.

**Scope, stated honestly:** `eventSetHash` is a change-detection / liveness token over API-level appends — the
same role it already serves for the inspector poll and #255's cache — **not** a tamper detector. An
out-of-band on-disk edit to an *unsigned* event's envelope field (`occurredAt` / `target` / `actorId` /
`eventType`) would change neither `eventId` nor `payloadHash`, and `validate_event` re-derives only those two
(`src/session/store/event_store.rs:307`), so neither the gate nor the read path would catch it. This is no
weaker than any existing read path (none defend against hand-edited store files); defending the index
specifically would be inconsistent. If tamper-evidence over the indexed columns is ever required, that is a
deliberate future extension (Revisit Triggers), not part of this shape.

### D4. Positions-not-bodies keeps physical erasure single-surface (the privacy invariant)

Because the index holds content-hash *refs* but never blob *bytes*, it is **not a physical-erasure surface**.
`shore store gc` / `shore store compact` (ADR-0016) physically erases blobs only and **must not need to touch
the index**. Because a `remove` is itself an appended `ArtifactRemoved` event, it flips `eventSetHash` and
triggers a rebuild (D3); the rebuild re-reads the log — whose body artifacts now render "content removed" —
so the index can never hold removed bytes. This is the load-bearing constraint: a body-caching index would
become a *second* physical-erasure surface that `compact` must also sweep, converting a privacy *feature*
(single-surface erasure) into a privacy *bug*. Positions-not-bodies is what keeps erasure single-surface.

### D5. Chronological/cursor order comes from the index, never from listing order

On-disk filenames are hash-sorted storage addresses, not causal order (ADR-0020 D4). The index is precisely
what supplies a chronological cursor without a full decode-and-sort: order is the indexed `(occurredAt,
eventId)` key — exactly #254's proposed `(occurred_at, event_id)` cursor. No reader may derive chronological
order from filename or directory-listing order.

### D6. The index is private derived state, not a public contract

The row layout, the locator scheme, and the index format stay **internal** (consistent with keeping event
filenames, artifact paths, and row IDs opaque until explicitly promoted). Stable external contracts remain
the named JSON documents and commands, not the index. A consumer reads *through* a projection/command; it
never addresses the index directly.

### D7. Substrate and the build trigger remain deferred and owner-gated

This ADR commits the shape every candidate substrate must carry; it does **not** choose one. SQLite (the
Fossil-like lean), a DuckDB-style analytical store (ADR-0020 D11's lead candidate), and a flat rebuildable
sidecar all remain open and are decided in a *separate* future ADR/plan when the build is justified. The
**build trigger** likewise stays owner-gated per #212: build when (a) the inspector's per-request full scan
is visibly laggy on a long-lived store, or (b) a genuine cross-cutting analytical query lands — with a
concrete target (e.g. p95 inspector latency, or an event-count threshold) set when proposed. Not before.

## Consequences

### Accepted

- **The future index cannot drift into a privacy hazard.** Positions-not-bodies (D4) is pre-committed, so
  whoever builds the index inherits single-surface erasure rather than rediscovering it.
- **The future index cannot drift into a consistency hazard.** Locator-not-cache (D1) plus `eventSetHash`
  gating (D3) keep it disposable derived state — no second class of facts to keep consistent.
- **The consumers are unblocked to design against a fixed shape.** #254's cursor, #215's by-actor / open-
  queue / by-content-hash views, and #135's validation read can all assume `(eventId, eventType, occurredAt,
  target, actorId, content-hash refs)` will be there.
- **Cost accepted:** committing a key set before the index exists. Mitigated by grounding the keys in
  already-specified consumers (D2) and making additions strictly additive (Revisit Triggers). We also accept
  that re-reading bodies at query time (rather than caching them) trades some query-time I/O for the privacy
  and consistency guarantees — a deliberate, KurrentDB-aligned trade.

### Rejected

- **A body-caching index** — creates a second physical-erasure surface `compact` must sweep; a privacy bug
  (D4). This is the primary rejection.
- **Storing derived/semantic state in the index** (classifications, supersession verdicts, coverage) —
  re-introduces a second class of facts to keep consistent, the exact drift the recompute model and #215
  avoid; the index locates, projections derive (D1).
- **Derived link-event streams** (KurrentDB `$ce` / `$et` / `$bc` style) — persisting derived "events"
  re-introduces durable facts that must themselves be kept consistent; #215 rejects this for the same reason.
  The index is disposable derived state, not new durable facts.
- **Trusting the index without `eventSetHash` gating, or repairing it in place** — forfeits the disposable-
  derived-state property; staleness is resolved by drop-and-rebuild, never by trust or patch (D3).
- **A separate per-column row digest** (a hash over `eventType` / `occurredAt` / `target` / `actorId`) as a
  second freshness gate — unnecessary under the append-only API contract (D3): every legitimate change to an
  indexed column travels through an append that already flips `eventSetHash`. Such a digest would only add
  tamper-evidence for out-of-band edits to *unsigned* events — a threat no existing read path defends against
  (D3 scope) — so adopting it here would be inconsistent; deferred to a Revisit Trigger.
- **Deriving chronological order from filename/listing order** — hash-sorted addresses are not causal order
  (D5, ADR-0020 D4).
- **Choosing a substrate (SQLite / DuckDB / sidecar) now, or building the index now** — deferred and owner-
  gated (D7, ADR-0020 D11). This ADR is shape-only by design.
- **Promoting the index to a public contract** — it stays private derived state (D6).

## Revisit Triggers

- **The build trigger fires** (a measured slow read path, or the owner's event-count / p95-latency target is
  crossed, or a cross-cutting analytical query lands) → open the **substrate** decision (SQLite vs DuckDB vs
  sidecar) and the build as their own ADR/plan, honoring this shape. (This is ADR-0020 D11's deferred seam
  being opened.)
- **A consumer needs a key not in the pre-committed row (D2)** → extend the row **additively** and re-confirm
  positions-not-bodies (D4) and locator-not-cache (D1) still hold. A non-additive reshape reopens this ADR.
- **Any proposal to cache bodies, store derived state, or have `gc`/`compact` sweep the index** → reopen
  here; these are the load-bearing rejections (D1, D4).
- **Tamper-evidence over the indexed columns is required** — the index must detect an out-of-band edit to an
  *unsigned* event's envelope that `eventSetHash` and `validate_event` do not catch (D3 scope) → add a
  per-column row digest or extend read-path validation; this shape deliberately omits it.
- **ADR-0020 D11's `ProjectionStore` seam is opened** → this row shape is its contract; reconcile the two so
  the seam and the index do not drift apart.

## Amendment: Locator plus Bodyless Semantic Checkpoints (2026-08-02)

ADR-0023's locator-not-authority rule, positions-not-bodies privacy boundary, authoritative reread,
chronological display order, private format, and rebuildability stand. This amendment supersedes D1 only
where it prohibited compact semantic state, D3's whole-set `eventSetHash` polling and unconditional lazy
drop/rebuild maintenance, and D7's untriggered build deferral. The measured long-horizon access problem fired
the build trigger, and the resulting implementation is recorded in
[ADR-0041](./adr-0041-bodyless-sqlite-derived-access.md).

This amendment also supersedes the header's not-yet-implemented framing: the derived-access implementation
now exists, although the checked-in runtime default remains `off`.

The locator remains a bodyless input index. Beside it, separately versioned bodyless semantic tables persist
the compact revision, history, thread, attention, state, relation, and validation facts needed to avoid
repeating complete-history folds. These rows are derived answers, never authoritative facts. They carry
identities, ordering keys, targets, references, counts, statuses, edges, and validation witnesses, but never
complete event bytes, object or note-body bytes, summaries, reasons, snippets, tokens, embeddings, or other
body-derived search material. Input-request titles and validation check names are the two authorized short,
output-bearing label exceptions; the sidecar-bytes qualification test distinguishes them from prohibited
prose bodies (`src/session/derived_access/semantic/mod.rs:32-38,113-223`;
`src/bench_support/derived_access/sqlite_semantic_tests.rs:852-876`).

For an active `sqlite-wal-bodyless-v1` profile, ordinary freshness is a conjunction of two bounded checks:

1. the persisted platform change stamp must continue as `Stable`; changed, truncated, reset, capped, gapped,
   or indeterminate observations require audit or rebuild; and
2. the compatible derived checkpoint's `applied_cursor` must equal the validated truth head, or bounded
   catch-up applies successor receipts atomically with the new checkpoint
   (`src/session/derived_access/checkpoint.rs:29-108`).

`eventSetHash` remains an exhaustive replay/integrity receipt and the loose response token. It is not the
ordinary active freshness mechanism because recomputing it requires the complete event set. Ordinary active
reads and fresh writer admission perform no event-directory enumeration. Existing-carrier overwrite remains
outside the bounded change-stamp claim and is detected when the selected carrier is reread or by an explicit
exhaustive audit.

Every returned domain result reopens and validates its selected authoritative event/content carriers. The
selection includes removal carriers for referenced content and detached signatures over selected or removal
events (`src/session/derived_access/history.rs:1167-1265`). Exact revision detail scopes diagnostics to the
addressed fork-tolerant supersession component and its support carriers. Revision collection pages scope
diagnostics and body-removal effects to the returned page. The CLI's exact revision read remains the
store-wide audit surface.

The checked-in runtime default remains `off`. This amendment authorizes no rollout, truth migration,
production activation, or release promise.
