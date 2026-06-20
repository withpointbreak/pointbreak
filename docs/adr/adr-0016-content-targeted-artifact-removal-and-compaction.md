# ADR-0016: Content-Targeted Artifact Removal and Local Compaction for the Append-Only Store

**Status:** Accepted (owner-approved 2026-06-19); landed via the store-topology collapse implementation
work. Approved across two `adr-review` cycles: a first approval, then an owner-directed revision (event
renamed `ArtifactPruned` → **`ArtifactRemoved`**, since the `gc`/`compact` sweep is the git-sense
"prune"; a session-anchored, content-addressed **event-target model** made explicit, §2; and
**remove-only** stated outright, §5), re-approved.
**Date:** 2026-06-19
**See also:** [ADR-0015](./adr-0015-single-common-dir-store.md) (the single-common-dir collapse that
makes this day-one). In-repo (`docs/adr/`): [ADR-0008](./adr-0008-cross-peer-conflict-policy.md)
(cross-peer conflict policy — projection-vs-sync-plane allocation; no timestamp arbitration);
[ADR-0014](./adr-0014-reviewunit-commit-range-lifecycle.md) (ReviewUnit commit-range lifecycle — the
monotonic withdrawal family + `retraction_target_missing` precedent; this removal is **distinct**: it
retracts *content*, not a commit/ref association);
[ADR-0003](./adr-0003-agent-resource-claims-advisory-first.md) (advisory contract — recovery via later
events); [ADR-0002](./adr-0002-large-snapshot-artifact-policy.md) (snapshot-artifact policy). Also GitHub
#146 (snapshot-scoped, deduped artifact body — the fact that forces content-targeting).

## Context

- **Sensitive bytes live in artifacts, not events.** Shoreline's durable facts are content-addressed
  events; projections are pure functions of the event set (ADR-0008). The captured *content* — diff
  rows (file contents) and large note bodies — lives in content-addressed **artifacts**
  (`artifacts/snapshots/<sha256(snapshotId)>.json`, `artifacts/notes/<sha256>.json`; note bodies are
  hashed at `src/session/store/body_artifact.rs:76`). The `ReviewUnitCaptured` event
  (`ReviewUnitCapturedPayload`, `src/session/event/review.rs:32`) carries only `snapshot_id`
  (`review.rs:38`) + `snapshot_artifact_content_hash` (`review.rs:39`) — pointers, not content.
  `referenced_artifacts()` (`src/session/workflow/artifact_transfer.rs:124`) already enumerates both
  blob kinds by a normalized `sha256:` content hash.
- **The collapse removes today's accidental privacy guarantee.** Per-worktree isolation gives two
  *accidental* guarantees: `git worktree remove` discards that worktree's `.shore/data` (the de-facto
  GC) and physically deletes its snapshot artifacts, so sensitive captured contents die with the
  worktree (`docs/storage-model.md`). The store-topology collapse (ADR-0015: shared `.git/shore`
  + write-through) breaks both — bytes persist in the shared store, and read-time scoping **hides** but
  does not **delete** them. The day-one privacy finding named the concrete regression: an
  *ephemeral worktree reviewing sensitive untracked/customer files*. An explicit, non-automatic removal
  is therefore a **day-one** constraint of the collapse, not deferrable polish.
- **#146 forces the granularity.** The snapshot-scoped artifact change (#146) made snapshot artifacts
  snapshot-scoped (`src/session/store/snapshot_artifact.rs:15`) and **deduped** on snapshot-content match regardless of
  the referencing unit (`snapshot_artifact.rs:63`): one blob is shared by **many** ReviewUnits
  (`review_unit → snapshot_id` is many-to-one; the two-worktree shared-blob repro is
  `two_linked_worktrees_capture_same_range_into_shared_store`, `src/session/workflow/capture.rs:950`).
  This is decisive for *what* a removal can coherently target.
- **Greenfield, but the pattern exists.** There is no `remove`/`gc`/`tombstone` today. The substrate
  already defines **retraction** as "a new event that withdraws an earlier event without mutating it"
  (`docs/substrate-language.md`), and ADR-0014's withdrawal family is the working precedent.

## Decision

### 1. Removal targets content (a `content_hash`), never a ReviewUnit

The removal fact is an `ArtifactRemoved { content_hash }` event, idempotency key
`artifact_removed:<content_hash>`. The payload carries **only** the `content_hash` — no free-text
`reason` field — so two peers removing the same content emit a byte-identical payload (the convergence
property §6 depends on; a justification/`reason` is deliberately deferred, see Deferred). This mirrors
the reason-free, identity-only ADR-0014 withdrawal payloads (`src/session/event/association.rs:80,103`),
the cited convergent-retraction precedent. It applies uniformly to snapshot **and** note-body artifacts
(both are sha256-addressed blobs). Content-targeting is **forced** by the #146 dedup: identical content
is one shared blob, so "remove unit U's content but keep unit V's identical content" is incoherent —
there is one set of bytes. It also dissolves the unit-targeting circular dependency ("delete X to delete
Y" vs "delete Y to delete X"): retracting *content* marks every referencing unit removed at once, with
no cross-unit ordering.

### 2. The removal event is session-anchored and content-addressed (not review-unit-scoped)

An `ArtifactRemoved` event is **not** a review-unit event, and that is deliberate: #146 dedup makes one
blob shared by **many** ReviewUnits (often across different sessions), so content has no single
review-unit home. Its `EventTarget` (`src/session/event/target.rs:10`) therefore carries **only
`session_id`** (the removing actor's session), and the `content_hash` rides in the **payload**,
addressing the target by content identity. Note that `target.session_id` is itself
**first-stored-local provenance, not a convergent value** — like `writer`/`occurredAt` it lives in
`eventRecordHash` but **not** `payloadHash` (`record_hash.rs:33`, `mod.rs:135`), so two peers removing
the same content from different sessions converge on the same `{ content_hash }` fact while keeping
different first-stored session envelopes (§3/§6). This mirrors the detached co-signature carrier
(`EventTarget::for_event_signature`, `target.rs:98`), which already addresses its target by payload
content-identity with a session-only envelope. The model already admits non-review-unit events —
initialization and imported notes use `EventTarget::new(session, work_unit)` (`target.rs:36`),
task-attempt events use `for_work_object` (`target.rs:75`) — so a content-addressed removal is a new
*shape*, not a new *category*. The store is **flat, keyed only by idempotency key**
(`src/session/store/event_store.rs:27`), not partitioned by unit or session, so the single
content-keyed removal is **globally visible** to every projection that reads the store — which is what
lets §3 render the removal for *every* referencing unit, in any session. (Whether `content`/`artifact`
should be promoted to a first-class substrate **work object** — `WorkObjectType` is today only
`{ReviewUnit, TaskAttempt}`, `src/model/work_object.rs` — is a deferred future investigation; see
Deferred.)

### 3. The event log stays immutable; the projection joins capture + removal

Appending the removal event never rewrites or tombstones the capture event. The ReviewUnit projection
joins the capture event with any `ArtifactRemoved` over its `content_hash` and renders **"content
removed"** for every referencing unit. That *the content is removed* is the **convergent shared fact** —
every peer holds an `ArtifactRemoved` for that `content_hash`, so every peer agrees. Any **actor/
`occurredAt`** shown alongside is **first-stored-local provenance, not a convergent value**: those
fields live in the `payloadHash`-exclusive envelope, so two peers that independently remove identical
content each keep their own first-stored record (`src/session/store/event_store.rs:62-77`) and their
`eventRecordHash`es differ (`writer`/`occurredAt` are part of `eventRecordHash`,
`src/session/event/record_hash.rs:34-35`) — the *who/when* can disagree across peers while the
*that-it-is-removed* cannot. (In the common single-remover workflow there is one removal event, so
attribution is unambiguous; the divergence is specific to concurrent independent removals of
byte-identical content.) This upgrades today's hard `missing artifact for snapshot …; import referenced
artifacts before reading` error (`src/session/store/snapshot_artifact.rs:170`) into a graceful,
*explained* absence, and disambiguates *removed* from *not-yet-synced/corrupt*. No per-ReviewUnit
removal event (redundant and racy — new units can reference the content later); no silent
dangling-pointer (indistinguishable from a missing import). Structural metadata
(supersedes/superseded_by, lineage, endpoints) survives for free, and signatures / idempotency /
convergence are untouched (the breakage class the snapshot-scoped artifact work spent four review rounds
avoiding).

### 4. A multi-selector command resolves to a content set

`shore store remove` accepts ergonomic selectors that **all resolve to a set of `content_hash`es**:
`--snapshot <id>`, `--review-unit <id>`, `--ref <branch>` / `--range A..B` (via the
reachability/context projection), `--orphans`. `--review-unit` reports co-referencing units before
acting ("snapshot S is also referenced by V, W; removing it removes the content for all of them") —
consent/awareness, not a block.

### 5. Two-phase: `remove` (event) then `compact` (local sweep); remove-only

`remove` appends the event (cheap, convergent, auditable); the bytes it marks survive on disk until
`compact`, so **compaction — not removal — is the point of no return** for the data. Removal is
**one-way (remove-only)**: Tier-1 defines no append-only un-remove event. This is deliberate and matches
the established retraction precedent — ADR-0014's withdrawal family is **monotonic**: deterministic
content ids (`src/session/event/association.rs:132-167`), an `associated − withdrawn` set-subtraction
fold (`src/session/projection/commit_range.rs:5,229`), and **terminal** withdrawal — a later identical
`Associated` returns `Existing` and does **not** revive a withdrawn edge
(`docs/adr/adr-0014-reviewunit-commit-range-lifecycle.md:87-91`). And ADR-0008's no-timestamp-arbitration
rule means a reversible remove/restore *toggle* would have no convergent way to order
"removed-then-restored" against the reverse. Moreover, content-keying makes a removed `content_hash` permanently absent for any future
identical capture — a privacy *feature*, not a defect. Recovering content before `compact` is therefore
**re-capture / re-import** (the bytes are content-addressed; any uncompacted peer can re-materialize
them), **not** an un-remove; a true `ArtifactRestored` is a separate deferred decision (see Deferred).
`shore store gc` / `shore store compact` is a **local, non-event** maintenance sweep that physically
deletes the removed blobs. GC is deliberately **not** an event — "I deleted my local bytes" is a
sync-process/local fact, not a shared review fact (ADR-0008's allocation rule). Because removal is
content-targeted, a removed `content_hash` has no live referrer by construction, so GC needs **no
reference-count wait**: removed content is immediately collectable, and the sweep is re-derivable from
the log. Operator advice: **to remove sensitive data, `remove` then `compact`.**

### 6. Convergence and safety

The content-addressed idempotency key **plus a `content_hash`-only payload** means two peers removing the
same content **converge to one fact**. The mechanism is exact: the `eventId` derives from the
idempotency key alone (`src/session/event/mod.rs:136`) and `payloadHash` covers the whole payload
(`mod.rs:135`), so a byte-identical `{ content_hash }` payload yields an identical `payloadHash`; the
second writer then dedups rather than conflicting (`src/session/store/event_store.rs:58`). Differing
`writer`/`occurredAt`/signature **and `target` (including `session_id`)** live in the
**payload-hash-exclusive** envelope and fall to the keep-first-stored-record path
(`event_store.rs:62-77`), not a conflict — so there is **no timestamp arbitration** (ADR-0008). This is precisely why §1 keeps `reason` out of the payload: a free-text
`reason` would mint a divergent `payloadHash` for the same content and `record_event_once` would
**hard-conflict** (`event_store.rs:78-83`) — the breakage class the snapshot-scoped artifact work spent
four rounds avoiding.
A removal whose target content has not backfilled is a `retraction_target_missing`-style projection
diagnostic that self-heals on backfill (the ADR-0014 precedent). The surviving removal event retains
**no content bytes** — `content_hash` is a sha256, not the content — but the hash is still a stable
fingerprint: it discloses content **equality/existence** and is **dictionary-testable** for low-entropy
or guessable content (so it is not zero-leak, only byte-free). The removal event is actor-attributed and
signable (accountability), with actor identity carried in the envelope, not the payload — but precisely
because attribution is envelope-borne (and thus `payloadHash`-exclusive), the rendered actor/`occurredAt`
is **first-stored-local provenance, not a convergent shared value** (§3): convergence holds on *that the
content is removed*, not on *who/when* removed it. Making attribution itself convergent would need a
separate unionable mechanism, out of Tier-1 scope (see Deferred).

### 7. Mirror reality (the scope boundary)

The removal **event** converges to peers (they learn the content is removed and *may* GC their own copy),
but **bytes cannot be un-sent.** So remove+compact is *complete* only **before** the artifact is
pushed/mirrored — aligning with "privacy = don't push." The ephemeral / never-pushed case is fully
solvable locally; the already-mirrored case is a **documented limitation**, and cross-store/federation
redaction is out of scope here.

## Consequences

### Accepted

- The collapse's day-one privacy regression is answerable with a small, on-model mechanism: one
  content-addressed retraction event + a local blob sweep, with **no event mutation**.
- Removed content renders as an *explained* absence everywhere it is referenced, derived from a single
  event.
- The event slots into the existing model as a session-anchored, content-addressed event (§2) — no new
  work-object type and no new target category are required for Tier-1.
- GC is trivially safe (no refcount race) and re-derivable from the log.
- **Cost:** a pre-push window is required for a *complete* privacy guarantee (documented; ties to "don't
  push").
- **Cost:** an `ArtifactRemoved` event type, a `shore store remove` + `shore store gc`/`compact` CLI
  surface, a projection join, and the graceful-render path are net-new.

### Rejected

- **Unit-targeted removal (a `ReviewUnitRemoved` that deletes a unit's blob).** With #146 dedup a blob is
  shared, so unit-targeting creates the circular-dependency/refcount deadlock the owner flagged and
  cannot coherently express "delete these bytes." (*Forgetting a unit's existence/metadata* while
  keeping shared content is a different, deferred operation — see below.)
- **A review-unit-targeted removal event (envelope `review_unit_id`).** Content is shared across units
  and sessions (#146), so no single unit is the right envelope target; the event is session-anchored +
  content-addressed in the payload instead (§2).
- **A reversible remove/restore toggle in Tier-1.** A symmetric un-remove fights ADR-0008's
  no-timestamp-arbitration rule (a toggle needs ordering the model refuses to provide) and the
  monotonic ADR-0014 precedent; deferred (see below).
- **A snapshot-id selector with silent graceful failure on dangling pointers.** A missing blob is then
  indistinguishable from not-yet-synced/corrupt; removal must be a *recorded fact*, not an absence.
- **A per-referencing-unit removal event.** Redundant and racy; the projection join is the single source.
- **Rewriting / tombstoning the capture event on compaction.** Breaks `eventRecordHash` / signatures /
  idempotency / convergence — the exact failure mode #146 / the snapshot-scoped artifact work avoided.
  Events are immutable; only blobs are collected.
- **GC as an event.** Local byte-removal is not a shared review fact (ADR-0008); converging it would
  falsely assert that peers' bytes are gone.

### Deferred (explicitly NOT decided here)

- **Tier-2 metadata redaction** — forgetting data that lives *in events*: small **inline** note bodies
  (not externalized below the inline limit), file paths / OIDs / `worktreeRoot` on the capture event.
  Removing those means altering signed / content-addressed / possibly mirrored events — the genuinely
  hard append-only problem; warrants its own ADR.
- **Reversal / un-remove (`ArtifactRestored`).** Tier-1 is remove-only (§5). A symmetric restore would
  have to solve convergent toggle ordering against ADR-0008's no-arbitration rule **and** the fact that
  post-`compact` bytes cannot be recovered by an event at all (only re-imported); it is its own decision
  if a genuine un-remove (as distinct from re-capture/re-import) is ever needed.
- **Artifact as a first-class substrate work object.** `WorkObjectType` is today `{ReviewUnit,
  TaskAttempt}` (`src/model/work_object.rs`); the removal event is session-anchored + content-addressed
  (§2) rather than targeting an `Artifact` work object. Promoting `content`/`artifact` to its own work
  object — with its own lifecycle — is the more substrate-principled long-term shape and is flagged here
  as a **future investigation**, explicitly not decided now.
- **Full mirror-aware retention policy** — retention windows, tombstone-vs-hard-delete across mirrors,
  federation redaction.
- **Integration-ref definition and gc'd-commit (object-gone) degradation** — these belong to the
  merge-status / orphan projection, not the removal mechanism.
- **The ephemeral worktree-local opt-out** — the sibling day-one privacy constraint, decided in ADR-0015;
  referenced here, decided there.
- **A removal `reason`/justification.** Deliberately **not** carried in the Tier-1 `ArtifactRemoved`
  payload — a free-text payload field defeats content-addressed convergence (§1/§6). If a justification
  is wanted later it must be carried convergence-safely: externalized as its own (removable)
  content-addressed note-body artifact referenced by hash, or otherwise kept out of the identity-bearing
  payload (Tier-2-adjacent).
- **Convergent (unionable) removal attribution.** Tier-1 renders *who/when* removed as
  first-stored-local provenance, not a convergent shared value (§3/§6); the convergent fact is only
  *that the content is removed*. Making attribution itself converge across concurrent independent
  removals would need a separate unionable attribution mechanism (not the removal-event envelope) —
  deferred.

## Revisit Triggers

- Cross-store / federation redaction becomes a requirement (the "can't un-send" boundary must move).
- Tier-2 metadata redaction is needed in practice (inline-body or capture-metadata sensitivity
  surfaces).
- A genuine un-remove (not re-capture/re-import) is needed — revisit the remove-only decision (§5) and
  its toggle-ordering / post-compaction byte-recovery constraints.
- The Artifact-as-work-object investigation (Deferred) concludes that content should be promoted to a
  first-class substrate work object.
- A retention policy beyond manual removal is required (auto-removal windows, quota-driven GC).
- The dedup assumption changes — if snapshot artifacts ever become unit-scoped again (reversing #146),
  the content-vs-unit granularity must be reconsidered.
