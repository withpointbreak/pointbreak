# Single-commit rewrites and read-only preview

Use this guide for a single-commit replay of reviewed content onto a descendant base.

| Situation | Route | Evidence boundary |
|---|---|---|
| Same reviewed state against its original base | Fresh `--source commit:<oid>` cursor; ordinary `association land` | Exact materialization |
| Identical scoped delta replayed as one commit on a descendant base | `--source captured`; parent-relative preview | Scoped equivalent rewrite; original facts stay historical |
| Changed blobs, modes, paths, status, content kinds or included scope | Capture and review a replacement Revision in the same Change | Fresh validation and assessment |
| Unsupported source or candidate shape, unrelated or unavailable history | Applicable ordinary route or replacement Revision | Parent-relative mode refuses |
| Structural provenance only | `association record` or ordinary `land --provenance-only` | No content-equivalence claim |

These routes describe the author's workflow; a reviewer only verifies the claim.

## Admission and proof scope

`--candidate-parent` admits a combined worktree capture or a committed range whose source commit
has exactly one parent equal to captured base A. Candidate C must have exactly one actual
commit-object parent B, and A must be an ancestor of B (equality is allowed). Root/merge candidates,
multi-commit sources, staged/unstaged/root captures and non-commit bases are unsupported.

The comparison uses B..C with the original capture mode and path scope. Full old/new blob identities,
modes, paths, status and content kinds must match the frozen canonical entries, including untracked-add
normalization. Identical text hunks alone are insufficient. Changes outside captured paths are outside
the proof. Advancing the base proves scoped equivalent rewrite, not exact materialization.
This mode ignores Git replacement objects and cannot combine with `--allow-extension` or
`--provenance-only`. Ordinary capture and landing retain their configured Git behavior.

## Author preview and recording

Use the existing exact `change_id`, accepted `revision_id`, and your author `track`. Bind `candidate`
to the full OID of the intended committed candidate; use HEAD only when it is that candidate.

```bash
candidate=$(git rev-parse --verify 'HEAD^{commit}')
rewrite_cursor=$(pointbreak change select "$change_id" \
  --revision "$revision_id" --source captured --format json | jq -r '.token')
pointbreak association land --review-cursor "$rewrite_cursor" --track "$track" \
  --commit "$candidate" --candidate-parent --dry-run --format json > rewrite-preview.json
proof_hash=$(jq -er '.proof.evidenceSha256' rewrite-preview.json)
```

Proceed only after preview succeeds and its exact candidate and proof have been inspected. When
recording is authorized, use the hash actually returned by that preview:

```bash
pointbreak association land --review-cursor "$rewrite_cursor" --track "$track" \
  --commit "$candidate" --candidate-parent --expect-proof "$proof_hash"
```

Recording recomputes the proof and revalidates the inputs immediately before publication. Drift
refuses before new proof, association or attestation publication. Successful recording preserves
ordered, idempotent proof → association → attestation writes, truthful partial failures, write
acknowledgements and advisory post-truth refresh diagnostics. Git and the Journal are separate
authorities; this is not a transaction excluding arbitrary external mutation.

## Preview and validation boundaries

Preview returns `pointbreak.association-land-preview.v1` with exact identities and the full canonical
proof. It performs no write-store preparation, signing-key load, artifact/event publication,
`state.json` refresh, migration, rebuild or activation. It has no write acknowledgement or
created/landed claim. Redirecting its JSON only saves a local inspection receipt.

Preview does not require `--expect-proof`, but checks it when supplied on either route. Parent-mode
recording requires the current preview hash. Preview JSON is inspection evidence, never trusted
recording input. A captured cursor alone does not authorize equivalence.

Original validation and Accepted facts remain attached to the original Revision. Keep rewritten
candidate checks in external receipts naming exact commit, tree, parent, proof hash, environment/tool
identities, commands, result and attempt identity. Never attach candidate tests or assessments to the
old Revision. Upstream context can require fresh review even when scoped equivalence succeeds.
Changed reviewed bytes or unsupported shapes require the ordinary replacement-Revision workflow.

Existing associations remain historical; each proof qualifies one association, and multiple live
associations may be ambiguous. Scoped proof is not whole-tree readiness. Recording does not authorize
push, PR creation or merge. There is no candidate-validation storage or automatic readiness decision.
