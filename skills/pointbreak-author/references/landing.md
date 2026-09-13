# Author landing and rewrite evidence

Read this only when recording an authorized landing or evaluating a single-commit rewrite.
Acceptance and proof do not authorize commit, push, PR creation, or merge by themselves; use the authorization already established for the task.

## Landing boundary

A commit that materializes already-reviewed content does not create a new Revision. Use the
proof-first wrapper after the reviewer accepts the exact Revision:

```bash
landed_commit=<commit>
landing_cursor=$(pointbreak change select "$change_id" \
  --revision "$revision_id" --source "commit:$landed_commit" \
  --format json | jq -r '.token')
pointbreak association land \
  --review-cursor "$landing_cursor" --track "$track" --commit "$landed_commit"
```

Re-select after committing: the authoring cursor is worktree-bound and must fail once the commit
changes that source state. The commit-bound cursor proves that the named commit materializes the
accepted exact Revision before `association land` records anything.

Strong language such as exact, equivalent, contained, or landed unchanged is permitted only when
this command returns a verified relation. `pointbreak association record` is the low-level structural
provenance escape; it does not prove content equivalence. `--provenance-only` records the same honest
limited claim when proof inputs are unavailable.

## Bounded single-commit rewrite evidence

For an eligible single-commit replay onto a descendant base, follow the
[rewrite decision table and command sequence](single-commit-rewrites.md):
select `--source captured`, preview `association land --candidate-parent --dry-run`, then record the
exact candidate with `--candidate-parent --expect-proof` using the actual preview hash. Preview writes
nothing and has no write acknowledgement. The independent canonical comparison is required; a captured
cursor alone does not authorize equivalence. Ordinary same-base materialization uses a fresh commit-source cursor.

Original validation and Accepted facts remain historical. Keep rewritten-candidate checks in external
receipts naming exact commit/tree/parent, proof hash, environment, command, result and attempt identity;
never attach them to the old Revision. Upstream context can still require fresh review. Changed reviewed
bytes or unsupported source/candidate shapes require the ordinary replacement-Revision workflow. A scoped
proof is not whole-tree readiness, and recording it does not authorize push, PR creation or merge.
