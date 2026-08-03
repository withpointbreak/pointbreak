# ADR-0041: Bodyless SQLite Derived Access over Loose Authority

**Status:** Accepted (2026-08-02); default-on rollout implemented and natively qualified (2026-08-03).
**Date:** 2026-08-02
**See also:** [ADR-0020](./adr-0020-durable-storage-backend-seam.md) (authoritative Journal and
ContentStore seam), [ADR-0023](./adr-0023-secondary-read-index-shape.md) (positions-not-bodies and selected
reread), [ADR-0024](./adr-0024-secondary-read-index-substrate.md) (substrate and maintenance),
[ADR-0039](./adr-0039-exact-logical-bundles-and-import-receipts.md) (logical transfer and physical recovery),
and [ADR-0016](./adr-0016-content-targeted-artifact-removal-and-compaction.md) (single authoritative content
removal surface).

## Context

Pointbreak's loose event and content stores remain strong at their primitive jobs: durable cross-process
create-once, keyed event reread, independently addressed content, strict replay, exact logical transfer,
backup, repair, and content removal. The scaling failure was above those primitives. Ordinary semantic reads,
chronological windows, revision detail, freshness, append refresh, and restart repeatedly discovered,
decoded, validated, folded, sorted, and retained complete history. At 102,400 events the loose-access process
held roughly 3 GiB of empty-adjusted resident memory and fixed-output operations remained proportional to
history. Replacing authoritative truth was not justified; bounded incremental derived access was.

The first bodyless SQLite implementation exposed correctness and complexity defects and was rejected. The
repaired implementation then passed native lifecycle and retained-scale semantic qualification. Product
integration subsequently added mixed-writer handling, background rebuild, explicit fallback, snapshot-bound
pagination, bounded authority, and selected-carrier validation. A final allocation/page-composition pass at
commit `217a3c1905610a96b8c6b1f7d90a5b019c529072` passed the unchanged qualification criteria with no failed or
unknown rows. Default-on was selected as the rollout target after considering the remaining exceptional
rebuild cost. This ADR records the architecture that exists; it does not perform that rollout.

## Decision

### D1. Loose Journal and ContentStore remain the sole authority

Authoritative event and content bytes stay in the existing loose `Journal` and `ContentStore`. They alone own
identity, durable create-once classification, point reread, validation, strict replay, exact transfer, backup,
repair, and content removal (`src/session/store/backend/mod.rs:283-389`). The SQLite state is private,
bodyless, disposable, and reconstructible. It may accelerate a decision about what to reread; it may not
repair, overwrite, erase, or replace truth.

The selected architecture has four explicit planes:

1. loose authoritative event and content carriers;
2. a private cursor ledger for writer intent, receipts, truth head, epoch, and bounded recovery;
3. a bodyless ordered locator for identities, keys, references, and validation witnesses; and
4. separately versioned bodyless semantic tables and checkpoints for compact product facts.

Public commands and Inspector documents remain domain-shaped. SQLite paths, row ids, pages, WAL frames, and
SQL are private implementation details.

### D2. Append, display, and replay order are separate contracts

`TruthCursor { epoch, sequence }` orders admitted appends and derived catch-up. It is private operational
metadata and never an event id, timestamp, signature input, page cursor, or logical-transfer coordinate
(`src/session/derived_access/cursor.rs:8-35`). Normalized `(occurredAt, eventId)` orders chronological
display. The existing deterministic replay key orders strict semantic reconstruction. Backdated and
canonical-earlier appends advance the truth cursor while entering earlier display/replay positions; no order
may substitute for another.

### D3. Bounded native authority admits current reads and governed writers

A current generation binds its cursor head to a platform-specific local change stamp in one SQLite snapshot.
Ordinary reads continue that stamp without walking `events/` or opening carriers. `Stable` is the only
serving verdict; a changed, truncated, reset, capped, gapped, unsupported, or otherwise indeterminate result
requires audit, catch-up, or rebuild. The bounded claim covers supported accidental and mixed-version local
appends, not hostile tampering or in-place replacement of an existing carrier
(`src/session/derived_access/lifecycle.rs:862-919`).

This architecture introduces a store-directory-scoped derived-access writer lock
(`src/session/store/resolution.rs:264-267`; `src/session/derived_access/sqlite/writer_lock.rs:17-58`).
Governed writers hold it while continuing authority, publishing loose truth, and finalizing the
receipt/head/stamp transition. Direct loose `create_once` remains lock-free and cross-process safe. A
uniquely created event consumes one sequence. Equal or conflicting duplicates do not advance the cursor. If
truth publication succeeds but receipt finalization does not, truth remains authoritative and the derived
generation fails closed; it cannot report current.

Native qualification covers the bounded protocol and lifecycle on macOS/APFS and real Windows/NTFS. APFS
uses directory identity/change metadata plus direct entry count. NTFS continues the unprivileged USN journal
and rejects gaps, resets, parsing failures, and work-cap exhaustion. Existing-carrier overwrite remains a
selected-reread or exhaustive-audit concern. Linux remains a compile/CI surface for this decision rather than
a retained-scale production-evidence claim.

### D4. Derived rows stay bodyless and every selected carrier is validated

SQLite may persist identities, cursor/order keys, event kind, full target, actor, content references,
lengths, compact semantic statuses/counts/edges, validation witnesses, and two short output-bearing labels:
input-request title and validation check name. Those labels are the authorized exception pinned by the
sidecar-bytes qualification test; SQLite may not persist complete event bytes, object or note-body bytes,
summaries, reasons, snippets, tokens, embeddings, or other body-derived search material
(`src/session/derived_access/semantic/mod.rs:32-38`;
`src/bench_support/derived_access/sqlite_semantic_tests.rs:852-876`). Consequently content removal remains a
single authoritative surface; deleting the sidecar never restores removed content.

Every returned domain result reopens and validates its selected event/content carriers through the
authoritative stores. Selection expands to the removal carriers for referenced content and detached
signatures over selected or removal events (`src/session/derived_access/history.rs:1167-1318`). Strict full
replay remains the semantic oracle and rebuild proof.

`projectionStamp` identifies one served derived snapshot by store identity, profile, semantic schema, epoch,
and applied sequence (`src/session/derived_access/history.rs:1417-1441`). It is neither truth identity nor a
signature input. Active responses use it; loose responses retain `eventSetHash`.

### D5. SQLite-WAL is the selected contained substrate

The implementation is `sqlite-wal-bodyless-v1`, shared by product and qualification callers. SQLite-specific
implementation details are confined to `src/session/derived_access/`: the `sqlite/` adapter core plus sibling
query and lifecycle modules. No substrate type crosses the `session` public boundary; higher-level consumers
use domain-shaped services. Its `derived/` generation is rooted beneath the
exact resolved authoritative store directory, so clone-local, ephemeral, and user-level family stores each
own their corresponding sidecar without a second path registry
(`src/session/derived_access/history.rs:199-216`; `src/session/derived_access/generation.rs:154-159`). The
stable store-level container name is `derived/`. “Projection” remains the query-serving
subset; the container also owns cursor, receipt, authority, and generation-lifecycle state. The checked-in
path authority selects `derived/` for a store with no derived namespace and continues to reuse a legacy-only
`.pointbreak-derived/` root. It does not move or rebuild existing state. The compatible transition must cover
the container and every sibling `.pointbreak-derived*` lock, lease, quarantine, and retired artifact. Bundled
SQLite is in the normal binary dependency closure (`Cargo.toml:89,91`). A public `ProjectionStore` trait is
not introduced for one implementation.

The cursor ledger uses WAL plus `synchronous=FULL` because its receipt/head transaction participates in
writer admission and recovery. The locator and semantic store uses WAL plus `synchronous=NORMAL` because it
is an atomic, rebuildable projection; if the latest transaction is lost, the separately durable cursor head
forces catch-up. Both enable foreign-key and cell-size checking; macOS also enables `fullfsync`
(`src/session/derived_access/sqlite/cursor.rs:1098-1148`;
`src/session/derived_access/sqlite/locator.rs:555-587`).

SQLite is selected for derived access, not for authoritative truth. The accepted native dependency, WAL/SHM
lifecycle, schema/projector versioning, backup exclusion and reconstruction, one-writer coordination,
quarantine, and package closure are ongoing support costs.

### D6. Lifecycle is completion-last, explicit, and recoverable

First build and deliberate rebuild may remain proportional to retained history. They run one validated
population traversal, release that population, run strict verification separately, recheck authority, and
publish an immutable generation last. Progress reports phase, completed/total events, bytes when known,
elapsed time, and a defensible ETA when available (`src/session/derived_access/generation.rs:84-115`).
Cancellation discards staging and remains latched until explicit retry. An existing compatible generation may
remain readable while its replacement builds, but authority drift invalidates that option.

The generation-independent Inspector status endpoint exposes absence, bootstrap, current, catch-up,
rebuild-required, quarantine, and unavailable states. During first build the user may wait or explicitly elect
the authoritative loose reader. That fallback is labeled, request-local, does not seed a retained whole-
history cache, and is limited to one concurrent request; overlap returns `429`. No state silently serves stale
derived rows as current (`src/session/derived_access/product_contract.rs:71-133`;
`src/cli/inspect/server.rs:753-783`).

In the checked-in integration, Inspector owns interactive first build: starting `pointbreak inspect` with
the active profile requests a background rebuild before the first API request
(`src/cli/inspect/server.rs:121-135`). Ordinary non-Inspector CLI reads stay on authoritative truth and there
is no public `pointbreak store build` command (`src/cli/store.rs:31-45`). An active writer requires a current
generation and otherwise refuses the write with a rebuild-required error
(`src/session/derived_access/writer.rs:37-50`). The rollout must therefore choose and document a safe
non-Inspector first-use policy; this architecture record does not silently choose one.

Rebuild is exceptional after activation: first activation, incompatible derived schema/projector, missing or
restored sidecar, corruption/quarantine, or an authority gap. Normal restart opens the current immutable
generation; normal governed append uses bounded catch-up.

### D7. Product reads are snapshot-bound and diagnostic scope is explicit

Revision collection is `pointbreak.inspect-revisions-page.v1`: default limit 100, maximum 500, normalized
`(capturedAt, revisionId)` descending order, indexed total count, and an opaque continuation bound to the
profile, projection snapshot, order, and cursor. A stale or wrong-profile continuation requires restart.
Active work selects at most `limit + 1` revision rows and returns only the requested page; the default-off
authoritative comparator returns the same document shape while remaining explicitly history-proportional
internally (`src/session/derived_access/revisions.rs:31-188`). The product contract freezes 500 independently;
the current carrier query also matches SQLite's default 500-term compound-select ceiling, so raising it first
requires query chunking or a deliberate internal-limit change
(`src/session/derived_access/revisions.rs:31-39,557-584`). Exact revision detail remains an independent
entity-primary route.

Inspector exact-detail diagnostics cover the addressed revision's fork-tolerant supersession component and
its authoritative support carriers. Revision-collection diagnostics and body-removal effects cover the
returned page. They do not repeat unrelated store-wide warnings on every detail/page read. The CLI exact
revision command remains the store-wide audit surface (`src/cli/inspect/api.rs:755-815,2111-2125`).

Body search remains an intentional, labeled history-proportional operation. Strict replay, audit, exact
transfer, backup verification, migration, copy-out repair, compaction/removal reconciliation, and deliberate
rebuild may also remain exhaustive; none is an ordinary-read shortcut.

### D8. Select the architecture and default-on target; keep rollout separate

The final criterion-identical package at `217a3c1905610a96b8c6b1f7d90a5b019c529072` passes with no failed or
unknown criteria. L100 derived allocation is 102,350,848 bytes, below the frozen 104,857,600-byte ceiling.
Representative revision-page p95 is 243 ms at L100 and 548 ms at C262; p95 across the pages in one complete
snapshot traversal is 1.803/3.025 seconds. Active pages and ordinary reads perform no event-directory walk.

The frozen thresholds and public reproduction modes are recorded in
[`docs/benchmarking.md`](../benchmarking.md#incremental-derived-access-falsifier-contract) and implemented by
`src/bench_support/derived_access/contract.rs:388-424`. The `limit + 1` and zero-directory-walk readiness
ceilings are pinned by `src/session/derived_access/product_contract.rs:2024-2067`. The exact retained native
package is identified by SHA-256
`a04c6bf7c9d06509eda2de885809cec2809d883adcb6d269ce2a784ed23fae04` and manifest SHA-256
`3ef3d187fc875d23499a7d041f15b76aacadb79a0a9003eda3329facac934e6d`; its large roots and raw receipts are
retained outside the source tree rather than published as repository fixtures.

The accepted trade-off is exceptional first-build cost. On APFS, product first-ready is 89.210 seconds at
L100 and 481.850 seconds at C262, with 2.115/5.112 GB first-start peak RSS. Real-NTFS evidence covers native
correctness/lifecycle and retained bootstrap/replay integrity; the measured production-shaped L100 build
phases total 874.425 seconds in a constrained Parallels VM. C262 NTFS product rebuild timing and Linux
retained scale are unmeasured. Evidence-runner aggregates that include independent replay, per-event scans,
and final inventories are not product rebuild timings.

These residuals are accepted, and **default-on** is the target posture. This ADR records that
decision but does not change `DerivedAccessProfile::parse`: an unset selector still resolves to `off` in the
checked-in source (`src/session/derived_access/product_contract.rs:23-57`). A separate rollout change must
own the actual default, release/package behavior, user-visible diagnostics, first-build expectations,
fallback, non-Inspector first use, completion of the compatible `.pointbreak-derived*`-to-`derived/`
transition, and rollback. No truth migration or authoritative-store rewrite is required to activate a
disposable sidecar.

## Consequences

### Accepted

- Ordinary semantic reads, snapshot pages, current checks, and governed writer admission are bounded without
  replacing authoritative truth.
- Active restart retains tens of MiB rather than a complete hydrated history; bodyless allocation remains
  proportional but within the frozen L100/C262 profile.
- SQLite adds native build/provenance and WAL lifecycle obligations, one-writer coordination, schema and
  projector versioning, quarantine/rebuild, and platform-specific support work.
- First build remains expensive at forward-looking retained tiers. Progress, cancellation, explicit loose
  fallback, valid-old-generation service, and completion-last publication make that cost observable and
  recoverable rather than pretending it is cheap.
- The checked-in runtime remains default-off until the separate rollout decision is implemented and tested.

### Rejected

- Replacing the loose `Journal` or `ContentStore` merely to solve derived-read scaling.
- Treating SQLite rows, cursor metadata, or `projectionStamp` as authoritative truth.
- Persisting event/content bodies or body-derived search material in the default derived profile.
- Using append position as chronological display or canonical replay order.
- Returning stale derived rows when authority or checkpoint validation is indeterminate.
- Reopening a broad substrate bakeoff after the selected SQLite profile passed the complete evidence matrix.
- Hiding first-build cost, retained-platform gaps, or outer capture latency behind the passing decision.
- Flipping the runtime default, migrating an owner store, or making a release promise in this architecture
  record.

## Revisit Triggers

- Selected-carrier validation or strict replay finds a semantic divergence, false-current state, or removed
  body in the sidecar response path.
- A supported filesystem cannot provide bounded fail-closed change continuation for the mixed-writer scope.
- First-build cost becomes routine rather than exceptional, or measured user impact exceeds the rollout's
  fallback and recovery envelope.
- A bounded non-Inspector CLI query adopts derived access; it must reuse the selected service and preserve
  explicit authoritative audit commands rather than create a parallel cache or silently weaken validation.
- A concrete body-search requirement justifies persisted body-derived material; decide its privacy,
  invalidation, removal, allocation, and rebuild contract separately.
- A second substrate is proposed with evidence strong enough to justify the migration and support cost; only
  then earn an internal projection-storage seam.
- Authoritative truth topology, rather than derived access, becomes the measured bottleneck.

## Amendment: Default-On Derived-Access Rollout (2026-08-03)

The default-on target selected by D8 is now the as-built runtime posture. An unset
`POINTBREAK_DERIVED_ACCESS` selects `sqlite-wal-bodyless-v1`; explicit
`POINTBREAK_DERIVED_ACCESS=off` remains the immediate artifact-free rollback. Earlier statements in this ADR
that the checked-in runtime remains off describe the qualified pre-rollout source cut and are superseded only
for current selector and interaction behavior.

The rollout completed the prerequisites D6 left open. `pointbreak store derived status|build|rebuild`
provides read-only diagnosis plus explicit synchronous build/rebuild. Bounded history, attention, and
explicitly limited revision-list reads use a current generation and otherwise fall back to authoritative
loose work with one actionable hint per exact store and process. A write without a usable generation
publishes loose truth once and reports derived degradation; it does not synchronously reconstruct history.
Inspector keeps its asynchronous first-build and explicit authoritative fallback behavior.

The stable `derived/` container and sibling locks, leases, quarantine, and retired artifacts live beneath the
exact resolved authoritative store root. Compatible legacy `.pointbreak-derived*` state moves without replay
only under exclusive, lease-aware transition. If stable and legacy namespaces conflict, Pointbreak reports
both local paths and keeps loose reads/writes available; it never guesses, merges, deletes, or labels one
authoritative.

Disposable D0/L1/L7 rollout qualification passes on macOS/APFS and real Windows/NTFS. Linux remains
compile/CI qualified, and the prior retained L100/C262 packages remain the scale authority; no C524 or new
retained-scale run was required. The rollout contract is
`pointbreak.derived-access-rollout-contract.v1`, SHA-256
`69650f18f89329aac602ecbf34a552084decd9f48556ce0e3f5a4aa2cc711fb1`, and preserves the immutable historical
integration/readiness hashes. No authoritative truth migration, production-store rewrite, body persistence,
or broad substrate seam is authorized by activation.
