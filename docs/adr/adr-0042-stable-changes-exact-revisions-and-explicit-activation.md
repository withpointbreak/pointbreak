# ADR-0042: Stable Changes, Exact Revisions, and Explicit Store Activation

**Status:** Accepted (owner-approved 2026-08-04); as-built implementation and native qualification
landed 2026-08-06.
**Date:** 2026-08-06
**See also:** [ADR-0014](./adr-0014-reviewunit-commit-range-lifecycle.md) (commit/ref association),
[ADR-0017](./adr-0017-eventtarget-identity-layering-and-engagement-naming.md) (identity layers),
[ADR-0018](./adr-0018-event-borne-supersession-replaces-lineage.md) (Revision replacement),
[ADR-0026](./adr-0026-fact-to-fact-response-relationship.md) (fact continuity),
[ADR-0038](./adr-0038-relation-proof-and-auxiliary-document-resources.md) (independent evidence),
[ADR-0039](./adr-0039-exact-logical-bundles-and-import-receipts.md) (logical transfer), and
[ADR-0041](./adr-0041-bodyless-sqlite-derived-access.md) (derived access). This ADR supersedes
[ADR-0037](./adr-0037-immutable-review-generations-and-fact-continuity.md) in full.

## Context

Pointbreak originally treated each immutable Revision as the top-level review subject. Multi-round review
then depended on proposal-borne `supersedes` pointers, and agent guidance sometimes attached facts or a
landing commit to an older Revision after reviewable bytes had changed. That preserved individual captures
but did not provide a stable identity for the work spanning them. It also made a historical Revision id
insufficient wherever the stored artifact itself could conflict.

The implemented system now has three distinct identities (`src/model/change.rs`, `src/model/revision.rs`,
and `src/session/store/fingerprint.rs`):

```text
ChangeId                   stable multi-round review work
  -> RevisionRefV1         one immutable captured state plus its artifact hash
       -> ObjectId         content identity beneath that Revision
```

The transition adds event kinds that signed v0.9 readers do not understand. Those readers were lenient for
unknown event kinds, so merely appending Change events would allow a stale reader to return partial history.
The store therefore needs a durable minimum-reader boundary before the first Change claim. Because the owner
is the only known pre-1.0 user, a finite append-only migration is preferable to permanent legacy branching,
but it must preserve existing event/content bytes and a recoverable pre-activation copy.

The complete design is implemented and qualified on macOS/APFS and real Windows/NTFS. This ADR records that
as-built system; it does not authorize migrating an owner store or publishing a release.

## Decision

### D1. Change is stable; Revision is exact; Object is content identity

`ChangeId` identifies one stable review work session. Its canonical `ChangeIdentityDescriptorV1` uses an
opaque nonce by default. Root-Revision identity is an explicit rendezvous mode; Pointbreak never infers a
shared Change from Git ancestry, branch, PR, paths, labels, or identical bytes.

`RevisionRefV1 { revision_id, object_artifact_content_hash }` is the integrity-qualified address of one
immutable captured state. Facts, validation, requests, assessments, resources, and associations target that
exact Revision, not the Change. `ObjectId` remains the content-only layer beneath it. One Revision may be a
member of more than one Change, and reusing it reuses its exact facts; Change membership does not clone or
retarget those facts.

The generic event envelope keeps `Revision` as its captured review-domain subject. Calling Change the stable
review work object does not add a new `EventTarget` shape or move Revision facts upward.

### D2. Change structure is append-only claim algebra

Change declaration, membership, membership withdrawal, Change link, Revision-relation assertion, and
Revision-relation withdrawal are independently keyed attributed claims (`src/session/event/change.rs`).
Effective state is unordered claim union followed by exact-claim subtraction. There is no timestamp winner,
mutable parent field, stored current pointer, or implicit reparenting.

Revision replacement remains a directional `successor supersedes predecessor` relation, but authority is an
exact, Change-scoped `ChangeRevisionRelationAsserted` claim naming two `RevisionRefV1` values. A withdrawal
names one claim. Proposal-borne `supersedes` survives only as preserved historical migration input; new
writers leave it empty. The prospective `continues` relation is not part of the model.

These claims file against a journal-scoped `TargetRef::Journal` without a `track_id` or envelope
`subjectId`; interactive writers use `journal:default`, while bulk adoption preserves the legacy root's
journal id. Their attribution is the event actor. This reuses the journal-scoped target rather than adding a
Change variant to the generic event envelope.

For one Change, current Revisions are active members not named as predecessors by active valid relation
claims. The deterministic projection (`src/session/projection/change.rs`) distinguishes:

- `initial`: one current member and no replacement relation;
- `parallel_current`: multiple intentional current members and no active replacement relation;
- `replacement`: one successor replaces one predecessor;
- `consolidation`: one current successor replaces several predecessors;
- `mixed`: multiple current states plus non-divergent replacement structure;
- `replacement_divergent`: current successors have intersecting predecessor ancestry;
- `cycle_conflicted` or `incomplete`: invalid or missing authority.

Parallel current Revisions may collectively reach accepted lifecycle only when each current Revision has one
accepting assessment and no operative obligation remains. Replacement divergence, cycles, declaration or
artifact conflict, and incomplete authority fail closed; Pointbreak never chooses a winner.

### D3. Content changes advance Revision; context moves only explicitly

`ReviewCursorV1` is derived resumable selection state binding Change, exact Revision/artifact, current-set
and graph token, source state, and blocking diagnostics. It is not a journal event or mutable current
pointer. High-level writers compare the cursor immediately before append and refuse stale graph, ambiguous
or divergent topology, missing artifact, changed source, or mismatched scope.

When code, tests, documentation, generated output, file modes, untracked inclusion, or capture scope changes,
the author captures a replacement or intentional parallel Revision in the same Change. The prior Revision
and its assessment remain exact history. Relevant context may be ported by an attributed `ReviewFactPorted`
record. Validation and assessments are never ported, and new-state facts are never attached to old bytes.

An unchanged state becoming a commit is different: it remains the same Revision and gains a commit
association only after D4's proof-first landing succeeds.

### D4. Association is structural; qualified landing is evidence-backed

Commit/ref association records historical structure for one exact Revision. Strong statements such as exact
materialization, equivalent rewrite, or extension require independent exact-Revision-bound relation evidence
under ADR-0038. `pointbreak association land` selects a commit-bound cursor, proves the relation before any
association write, and records the proof/attestation plus structural association. A changed or mismatched
candidate refuses before recording facts.

`pointbreak association record` remains the explicit low-level provenance escape. It records an unverified
structural association and cannot authorize content-qualified wording. A landing commit may truthfully
associate with several Changes without merging their identities.

### D5. `review_change_revision_v1` is a hard minimum-reader store boundary

Capability authority is a typed non-event Journal record routed before event decoding
(`src/session/store/capabilities.rs`). `AuthorityCursorV2` accounts for Journal records, events, and the
capability set separately. The store has three states:

- **L0 / `migration_required`:** no activation record; signed v0.9 remains supported and capable Change
  product routes return typed migration guidance before partial semantics.
- **M1 / `migration_in_progress`:** one valid activation and manifest authority records
  `review_change_revision_v1` as the minimum reader, but verified completion is absent. Normal product
  reads/writes fail closed; only the explicit migration resume/inspection path proceeds.
- **L2 / `ready`:** completion verifies declaration, Revision-id-keyed membership, artifact-exact relation
  claims, and every admitted legacy relation. Projection then refuses any member whose Revision id resolves
  to conflicting artifact identity. Capable Change reads/writes and exact import are eligible.

Malformed, conflicting, partial, unknown, or non-monotonic capability state refuses before semantic output
or mutation. A pre-Change derived generation is never a fallback after activation.

The **reader capability profile** is durable architecture, not a one-off label for this bulk adoption.
It names the minimum reader and the coherent document/capability registry that a root requires; a later
breaking cohort must declare a successor profile and make its reader support explicit. The current
`review_change_revision_v1` activation and its retained migration procedure are only the first use of that
mechanism. The corresponding vocabulary is deliberately durable and relative to the required target cohort:
`migration_required` means that target profile has not been admitted, `migration_in_progress` means it has
been admitted but lacks verified completion, and `ready` means the declared target cohort has verified as
complete. `reader_upgrade_required` is the complementary reader result: a
root may be ready, while a particular client is still unable to consume its declared profile. These states
describe observed authority rather than optimism; partial or ambiguous authority never manufactures
readiness or a compatibility fallback.

`pointbreak change migrate-dry-run` is read-only and freezes exact root identity, authority cursor, cohort
manifest, allocations, overlap/anomaly decisions, and the owner-decision hash. `pointbreak change migrate`
requires the exact dry-run, cohort and minimum-reader acknowledgements, explicit acknowledgement that v0.9
is unsupported for that root after activation, a verified external pre-activation backup, and an available
signer for the initial retained execution plan.

The store authority lock (`StoreAuthorityLock`, `authority.writer.lock`) spans revalidation and the
L0 -> M1 -> L2 transition. Activation and manifest authority append before cohort claims; exact
declarations, memberships, relations, and retained anomaly
decisions resume idempotently at every append boundary; completion appends last. A retry reuses the retained
signed plan rather than recomputing identity from a partial graph. Derived Change state publishes only after
L2.

### D6. Supported readers, transfer, and derived access consume one cohort

The capable CLI, Inspector Web UI, VS Code extension, and the three in-repo review skills all use stable
Change plus exact Revision. L0/M1 routes return typed migration status, legacy aggregate HTTP routes on L2
return typed reader-upgrade guidance, and supported clients negotiate the reader profile before presenting
Change semantics.

Exact bundle v2 has distinct exact-Revision and complete-Change scopes. It carries the capability activation,
completion, declarations, applicable membership/withdrawal/relation closure, selected exact proposal and
content records, and required fact/proof resources. Import preflights the whole package, requires an L2
destination, writes content before cohort events, and records the destination-local operational
receipt last. It never reconstructs omitted claims or mutates imported bytes.

Loose Journal and ContentStore bytes remain authority. The bodyless SQLite generation is disposable and
rebuildable under ADR-0041. Activation invalidates pre-Change generations; a fresh schema projects Change
membership, topology, lifecycle, fact origins, and exact resources only after verified L2 completion.

### D7. Rollback is forward repair; the migrator is temporary but retained

Activation is append-only and forward-only. There is no supported deletion of capability/manifest/completion
authority and no route back to L0 in place. Explicit derived-off and writer disable are operational controls,
not store rollback.

The verified pre-activation backup may restore only into an empty, separately identified L0 recovery fork.
It never overwrites or silently reopens the activated root. Exact logical transfer and copy-out repair remain
available for forward recovery.

The run-once migrator stays isolated from normal product services. It may be removed only by a later,
separately authorized cleanup after every known root has cut over or has a reconciled recovery decision. Git,
the signed release artifact, and retained external plans/backups preserve the recovery implementation and
evidence; this ADR does not claim that removal has occurred. Removing that procedure does not remove the
reader-profile boundary or its truthful transition states: future capability-cohort migrations must use their
own explicitly admitted procedure and may not infer a ready result from a partial predecessor.

## Consequences

### Accepted

- Multi-round work has one stable identity without weakening exact-state review or audit history.
- Parallel work and replacement conflict are distinguishable instead of collapsed into one scalar head.
- Late relation correction is append-only, attributed, and exact-claim-withdrawable.
- Agent loops cannot truthfully assess changed bytes under an older Revision cursor.
- Unknown Change events cannot be partially consumed by a lenient legacy reader after activation.
- Existing event/content bytes, signatures, hashes, and content identities survive bulk adoption unchanged.
- Migration carries explicit owner acknowledgement, recoverable backup, interruption resume, and native
  APFS/NTFS proof.

The costs are a larger claim/document surface, cursor discipline, a forward-only store boundary, coordinated
reader rollout, one-time migration ceremony, and temporary migrator maintenance.

### Rejected

- Treating every Revision as unrelated top-level work forever.
- Assigning implicit Change identity from Git, PR, paths, time, or content.
- Permanent `legacy_derived` coexistence or steady-state branching by old/new store semantics.
- Proposal-borne replacement authority, required `continues`, mutable parent fields, or timestamp winners.
- Moving facts, validation, or assessments to a new Revision implicitly.
- Associating a changed commit with an old Revision merely because it belongs to the same PR.
- Warning and returning partial Change semantics to v0.9 or any unsupported reader.
- In-place backup restore, capability rollback, implicit migration, or derived state as authority.

## Revisit Triggers

- A real workflow needs Change-to-Change absorption, split, dependency, or replacement semantics.
- Many-to-many Revision membership produces fact reuse that cannot be presented safely.
- Claim-specific withdrawal needs an authorization policy beyond the current trust-neutral convergence rule.
- A supported reader can bypass, misclassify, or partially consume M1/L2 authority.
- Native operation shows `StoreAuthorityLock`, the retained plan, backup, or completion-last transition is not
  resumable or repairable.
- Known-root cutover completes and a separately reviewed cleanup can remove the production migrator.
- A post-1.0 compatibility promise requires a versioned transition different from this explicit pre-1.0
  minimum-reader break.
