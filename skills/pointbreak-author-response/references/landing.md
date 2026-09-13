# Author landing and rewrite evidence

Read this only when recording an authorized landing or evaluating a single-commit rewrite.
Acceptance and proof do not authorize commit, push, PR creation, or merge by themselves; use the authorization already established for the task.

## Record the landing commit

After the reviewer accepts the exact current Revision, use proof-first landing:

```bash
landed_commit=<commit>
accepted_revision_id=<accepted-revision-id>
landing_cursor=$(pointbreak change select "$change_id" \
  --revision "$accepted_revision_id" --source "commit:$landed_commit" \
  --format json | jq -r '.token')
pointbreak association land \
  --review-cursor "$landing_cursor" --track "$author_track" --commit "$landed_commit"
```

Re-select after committing. A capture or fact-writing cursor may be worktree-bound, and its refusal
after the commit is the intended source-race protection. Landing uses a fresh commit-bound cursor
for the accepted exact Revision.

If the proof is refuted, capture and review a new Revision. If it is indeterminate and only provenance
is needed, use `--provenance-only` or the low-level structural `association record` command and avoid
content-qualified wording. An allowed extension must be explicit and remains partly unreviewed.

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
