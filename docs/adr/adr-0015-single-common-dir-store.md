# ADR-0015: Single Common-Dir Review Store; Filter on Read

**Status:** Accepted (owner-approved 2026-06-19); landed via the store-topology collapse implementation
work. The three load-bearing blockers below resolved before acceptance: capture context is recorded as
separate association events via the landed ADR-0014 association/withdrawal family; the cross-worktree
grouping projection's prerequisites (#146 and the ReviewUnit commit-range lifecycle) landed; and the
day-one privacy/content-removal mechanism is decided in the accepted ADR-0016. (See "load-bearing
preconditions" below.)
**Date:** 2026-06-19
**Amendment (2026-07-15, #539):** the unfiltered revision list is projection-complete. Git
reachability remains read-time status enrichment, but no longer default-hides recorded revisions;
only an explicit filter may narrow them. Content privacy and reclamation remain the responsibility
of ADR-0016's explicit remove/compact lifecycle, not record visibility.
**See also:** [ADR-0003](./adr-0003-agent-resource-claims-advisory-first.md) (advisory contract),
[ADR-0008](./adr-0008-cross-peer-conflict-policy.md) (cross-peer conflict policy — "ambiguity is a
selection error at unscoped-current boundaries"; idempotent union),
[ADR-0009](./adr-0009-resumption-binding-trust-source.md) (possession-as-trust-root, single-writer
context); [ADR-0005](./adr-0005-review-unit-lineage.md) (retired `ambiguous_current_review_unit`);
[ADR-0014](./adr-0014-reviewunit-commit-range-lifecycle.md) (ReviewUnit commit-range lifecycle — the
association/withdrawal event family that records capture context as separate events and feeds the
cross-worktree grouping projection);
[ADR-0016](./adr-0016-content-targeted-artifact-removal-and-compaction.md) (the day-one content-removal
mechanism). GitHub #153/#138/#139/#140/#146.

## Context

Before this decision, Shoreline resolved its review store per worktree to one of two locations:

- **Worktree-local** `<worktree>/.shore/data` — the zero-config default for an **unregistered**
  worktree.
- **Clone-local shared** `<git-common-dir>/shore` (`.git/shore`) — **opt-in**, registered by a
  `shore store link` command, shared across the clone's worktrees.

The earlier write-through landing first made both reads and writes follow the resolved mode — a
registered linked worktree read **and** wrote `.git/shore`, which closed the original #138 "linked main
worktree can't read its own capture" split. What still remained at that point was that the shared store
was **opt-in** — a `shore store link` registration plus a per-worktree resolution-mode/registration
lifecycle — which is the friction this ADR targets: the collapse turns that opt-in registration into the
**common-dir default** plus read-time scoping and an explicit ephemeral opt-out. (Historically, earlier
still, `link` moved only the *read* location — reads resolved the shared store when linked (#140), while
writes stayed worktree-local and were advanced by a **batch** copy; `link` was a one-shot sync, not a
persistent relocation. The original store-topology intent was clone-local-shared; the batch shape was a
deliberate stepping stone to avoid live multi-writer that calcified into a "sync to a remote, only when
instructed" model.)

Underlying conceptual error: **git worktrees are views of one repository, not forks.** They share the
object store, refs, and config; only `HEAD`/index are per-worktree. Append-only, content-addressed
review **events are objects, not `HEAD`** — yet the two-mode model isolates them per-worktree and makes
sharing opt-in, treating each worktree like an independent clone.

## Decision

**A single review store per clone, in the Git common dir (`.git/shore`), is the DEFAULT for every
worktree (main included), and storage isolation is replaced with read-time scoping — while an explicit
opt-in ephemeral/worktree-local mode is kept for the sensitive-throwaway case.** This flips the prior
default (isolated-by-default + share-by-opt-in) to its inverse (shared-by-default + ephemeral-by-opt-in).

1. **Resolution defaults to the common dir.** The default store dir is the Git common dir's `shore`
   directory (`.git/shore`), resolved the same from the main worktree and every linked worktree. The
   prior opt-in-link resolution-mode enum and the store-registration file/schema are gone — the common
   store is the default, not a registered opt-in. Reads, writes, and write-validation all open the one
   store. The only surviving per-worktree bit is the **explicit opt-in ephemeral** mode (a
   deliberately-chosen worktree-local store for sensitive-throwaway review); absent it, every worktree
   uses the common store. (This is a far smaller distinction than the prior link/unlink lifecycle.)
   **Placement is a location-agnostic spectrum:** ephemeral worktree-local (opt-out) / clone-local
   `.git/shore` (this default) / **user-level `~/.shore/stores/<family>` (opt-in)**. The user-level tier
   is **a first-class opt-in tier** — but it is gated (precondition 4 below) and is not the default.
   **Placement and sync-strategy are two axes:** separate *where bytes live* (the tiers above, plus a
   possible **git-ref** store — the content-addressed log as a git tree on a ref) from *whether/where
   they are pushed* (**local-only / private-remote / shared-remote**). **Privacy is "don't push," a
   configurable sync-strategy** + redaction — **not** directory permissions and **not** hidden custom
   refs. A git-ref store may obsolete the user-level *directory* tier (no key co-mingling; git
   distribution — modulo a `refs/heads`-vs-custom-ref subdecision, *not* literally "for free") at the
   cost of ref-CAS concurrency the filesystem store avoids; the substrate choice (filesystem dir vs
   git-ref tree) and ref-namespace remain **open** future work.

2. **Concurrency is lock-free by content-addressing** (re-affirmed from the write-through analysis):
   events and snapshot artifacts via content-addressed `create_file_exclusive`; note bodies
   content-addressed; `state.json` a regenerable atomic-rename projection that reads never trust as
   authority (reads rebuild from events). Concurrent writers across worktrees cannot corrupt the durable
   layer. **No store mutex.** (Corruption-free is not conflict-free: the legacy snapshot artifact body
   embedded a worktree-namespaced `review_unit_id`, so same-`snapshotId` captures across worktrees
   collided *loudly* — the write-side twin of cross-worktree grouping — now resolved by the
   snapshot-scoped artifact body (#146); see precondition #2.) If a lock is ever required it MUST be
   **store-directory scoped, never "one clone = one writer"**, so cross-clone / user-level federation
   (deferred) inherits it.

3. **Reads default to a scoped view** — a **git context/reachability projection** over the one store:
   captures relevant to the current `(worktree, HEAD)` context, with explicit selectors (`--all`,
   `--review-unit`, `--branch`, `--worktree`) to widen across the family. Ambiguity at the default
   boundary remains a **selection-time error, never an ambient diagnostic** (ADR-0005/0008).

4. **`link`/`unlink` shed their worktree *link* role.** Cross-worktree sharing is the default, so
   `link`'s honest residual is cross-**clone** federation (cross-clone / user-level federation, deferred)
   and the earlier per-worktree `unlink`-as-link-detach (#139) is moot. A per-worktree **ephemeral
   opt-out** is instead provided as a `shore store mode` command plus committed/local config (to keep a
   worktree's data worktree-local + discardable). Migration folds existing `.shore/data` stores into
   `.git/shore` via an idempotent import, **but only with a consent / opt-out path — it does NOT
   auto-fan-in an ephemeral/sensitive worktree** (privacy precondition below). The opt-in-link
   registration is replaced by the smaller ephemeral-opt-out state, not removed outright.

5. **Lifecycle is explicit.** Orphan units (deleted-unmerged branches) are **kept** (append-only) but
   **hidden** from the default view (no live ref contains their commit) and removed only by an explicit,
   never-automatic **content removal** (ADR-0016: `shore store remove` then `compact`; its full
   mirror-aware retention semantics in an append-only model are deferred there to a follow-on ADR).
   Merge-status is a **structural reachability projection** (`merged|open|orphaned|unknown`) for
   commit-anchored captures, replacing the observation-only signal — except for uncommitted
   working-tree-snapshot captures, which remain observation-driven until committed.

## The load-bearing preconditions (now resolved)

Independent review surfaced **three** blockers; reachability is not the silver bullet the first draft
implied. All three are resolved:

1. **Capture context is a separate event, not a payload field — RESOLVED.** Capture keys on a
   `review_unit_id`-derived idempotency key, and the store rejects same-key + different-payload-hash, so
   a `head_oid`/`captured_on_ref` field *on* the capture payload would change the payload hash →
   recapture **conflicts**. **Resolved by the landed ADR-0014 association/withdrawal family:** capture
   context rides on **separate** events (`ReviewUnitRefAssociated` carries `ref_name` + `head_oid`;
   `ReviewUnitCommitAssociated` carries the commit endpoint), never on the capture payload — no
   payload-hash conflict. A worktree capture **auto-records** its capture-time branch ref as a
   best-effort `ReviewUnitRefAssociated`, so the capture context is populated. The topology side then
   **consumes** these events: the read-scoping/reachability projection (§3) reads the association events,
   feeding the cross-worktree grouping projection (precondition 2).
2. **Cross-worktree grouping of commit-anchored captures — RESOLVED.** The identity hash folds the
   normalized worktree root for *all* captures including commit-range, so two worktrees capturing the
   same range mint different ids; reachability scopes a *single* id but does not dedupe. The decision
   (review-confirmed against ADR-0014) is to build the **commit-OID grouping projection** — invert the
   commit-range projection's per-unit current commit set into `commit_oid → {review_unit_ids}` — and
   **reject** the identity-namespace change (no re-ID). That grouping projection consumes ADR-0014's
   `ReviewUnitCommitAssociated`/`…Withdrawn` events, which are landed. **Write-side twin (GitHub #146) —
   RESOLVED:** the grouping projection is read-side and presupposed the two ids coexist in the store —
   but they could not, because a snapshot artifact is path-keyed by `snapshotId` while its legacy **body
   embedded `review_unit_id`**, so two same-range captures from different worktrees collided on write
   with a hard `snapshot artifact conflict`. The **snapshot-scoped V2 artifact body** (#146) dropped
   `review_unit_id`/`worktreeRoot`, dedups identical-`snapshotId` artifacts, and binds identity via the
   event/projection — so the two ids coexist with one shared byte-identical artifact. (A dual-read path
   keeps V1 artifacts readable; the hard V2 break is deferred to #177.) This decoupled the artifact
   **body**, not the id, so the no-re-ID rejection stands.
3. **Working-tree-capture scope + day-one privacy — RESOLVED.** The collapse persists potentially
   sensitive snapshot bytes in `.git/shore` after `git worktree remove` (which previously discarded
   them), so an explicit day-one cleanup is non-negotiable. **Resolved by the accepted ADR-0016**
   (content-targeted `ArtifactRemoved` + `shore store remove` then `gc`/`compact`, remove-only) — the
   day-one removal mechanism. The original default-hide pairing was superseded by #539's invariant
   that recorded revisions stay discoverable; privacy requires explicit removal, not a hidden row.
   The working-tree-capture default scope
   (uncommitted target → no commit anchor → scoped by worktree-identity + recency, not structurally
   merge-status'd) and the explicit opt-in ephemeral / worktree-local opt-out command and config surface
   (§4) are part of the as-built collapse. (The projection-freshness rebuild was always **non-gating** —
   a net-zero-cost post-write `from_events` rebuild.)
4. **(Gates the user-level tier only)** Family-identity keying + a secrets-safe layout. The user-level
   tier needs a stable repository-family-id resolution (recommend **explicit-link** for V1; do **not**
   auto-key by remote URL). For the user-managed-git-repo story, the supported git root is
   **`~/.shore/stores/<family>/` (or `~/.shore/stores/`), never `~/.shore` itself** — so the private
   signing keys in the *sibling* `~/.shore/keys/` are **outside any review-store repo by construction**
   (not merely `.gitignore`d). `git init ~/.shore` is explicitly discouraged; `state.json` is gitignored
   within the stores tree. Without these the user-level tier ships unsafe; the **clone-local default
   does not need them**, so this gates the *tier*, not the whole decision.

**Blockers 1–3 are resolved, so the core decision holds; the cross-worktree grouping projection, the
ephemeral opt-out surface, and the working-tree-capture scope are part of the as-built store. Precondition
4 still gates only the user-level tier.**

## Consequences

### Accepted

- **Mostly deletion** of the *opt-in-link* machinery: the read/write-validation link-mode branching and
  (in steady state) the local∪linked union + the clone-local unsynced/batch-only diagnostics. The prior
  resolution-mode enum and store-registration file/schema are **replaced by an ephemeral opt-out state**
  (a smaller bit + resolver/config), *not* removed outright — that is the ephemeral opt-out surface (§4).
- #138 (lost-on-`worktree remove`) **dissolves by construction** — writes land under the surviving
  common dir, never in a removable worktree.
- The "single-writer per `.shore/data`" contract evolves to a **concurrent-writer-safe shared store**
  (the projection-freshness rebuild: command-returned projections rebuild from a post-write event scan,
  never from a pre-write single-writer batch).
- A structural merge-status / liveness projection replaces the observation-only merge signal for
  commit-anchored captures.
- The earlier write-through landing stands as the foundation; its write-store resolution's opt-in-link
  branch becomes the default common-dir resolver (mostly a deletion, plus the ephemeral opt-out branch).

### Rejected

- **Keeping the two-mode model + opt-in `link`.** It is the accidental complexity this ADR removes; the
  opt-in stepping stone is unnecessary once writes relocate.
- **Hidden custom refs as a privacy mechanism, and shadow branches for uncommitted state.** Prior art
  tried hidden custom refs and **removed them** — privacy is carried by sync-strategy ("don't push") +
  redaction, not ref-hiding (a custom `refs/shore/...` is fine only for branch-list *hygiene*, never as
  the privacy lever). And Shoreline does **not** adopt shadow branches for uncommitted state — its
  content-addressed snapshot artifacts already capture it.
- **Per-worktree storage isolation as the *default*** — replaced by read-time filtering. But isolation
  is **not** rejected as a *feature*: review found a real case (ephemeral worktree reviewing sensitive
  untracked files, where worktree-delete is an accidental privacy guarantee), so an **explicit opt-in
  ephemeral/worktree-local mode survives** (shared-by-default + ephemeral-by-opt-in, the inverse of the
  prior default). What is rejected is making isolation the *default*.
- **A store/clone-level write mutex** — unnecessary (content-addressing is lock-free) and would break
  cross-clone / user-level federation; any future lock is store-dir scoped only.
- **Automatic/silent deletion of orphans** — retention is keep-by-default; deletion is explicit only.
- **Teaching reads to union worktree-local into results** — re-forks authority; the relocation removes
  the need.

### Deferred (named, not built)

- **User-level placement — not fully deferred.** The single-user `~/.shore/stores/<family>` store is
  **a first-class opt-in tier** (gated by precondition 4), not deferred. `link`/`unlink` are repurposed
  as its promote/detach surface. **Still deferred** is the broader cross-clone / user-level federation
  **authority**: cross-*user* / remote mirror / federation (encrypted remote sync, multi-writer across
  machines). (A per-*worktree* ephemeral opt-out command/config is part of the as-built store.)
- **Full retention / mirror-aware retention policy** in an append-only model (can't un-send a mirrored
  event) — its own follow-on ADR (ADR-0016 decides the content-`remove`+`compact` mechanism and defers
  this broader policy). **Only this is deferred.** The *minimum* day-one cleanup — the ephemeral opt-out
  (keep a worktree's data worktree-local + discardable) and a local-only purge of an unmirrored
  worktree's sensitive snapshots — is part of the as-built store, per the load-bearing preconditions
  above. (Resolves the apparent day-one-vs-deferred tension: minimum cleanup now, full mirror/retention
  policy later.)
- The read-scoping/reachability projection's deeper **performance model** under large stores. (Its
  consumption of the already-recorded association events is built; capture context is **not** a
  capture-payload field — it is carried by `ReviewUnitRefAssociated`/`ReviewUnitCommitAssociated` per
  ADR-0014, precondition 1.)

## Revisit triggers

- Working-tree-capture scoping proves it cannot be made good enough → reconsider (the two-mode model may
  survive in reduced form).
- A concurrency hazard appears that content-addressing does not cover → add a **store-dir-scoped** lock
  (never clone-scoped).
- Cross-clone sharing becomes a real requirement → cross-clone / user-level federation, reusing
  `link`/the existing store-bundle import.
- The append-only **content-removal**/retention story (ADR-0016) proves intractable → revisit retention
  separately.
