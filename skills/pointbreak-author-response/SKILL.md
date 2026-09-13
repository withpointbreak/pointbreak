---
name: pointbreak-author-response
description: Respond to a Pointbreak review, advancing the Revision when reviewed content changes.
---

# Pointbreak Author Review Response

You are the agent that authored the change. The Change is the stable review session. The review
cursor names the exact immutable Revision under
review. Never attach facts about changed live bytes to an older Revision.

Do not run `pointbreak assessment add`; the Call stays the reviewer's. Do not start or resume store
migration.

## Read the reviewer pass

Require the existing `change_id`, exact `revision_id`, artifact hash, review cursor, author track,
and reviewer track. Verify the capable store is `ready`:

```bash
agent_name="<canonical-agent-name>"
export POINTBREAK_ACTOR_ID="actor:agent:${agent_name}"
```

Use the same canonical spelling as the original author pass. The response therefore reuses the same
agent identity and signing key. Until a human enrolls it, signed events display as untrusted;
untrusted does not mean invalid. Enrollment is optional. `pointbreak key enroll <name>` stages the
signer in `.pointbreak/allowed-signers.json` for human review. A human may instead adopt an existing
SSH key with `pointbreak key use-ssh`. Signing never gates an ordinary write;
`POINTBREAK_SIGNING=off` is the explicit opt-out.

```bash
pointbreak change profile --repo . --format json-pretty
pointbreak change show "$change_id" --repo . --format json-pretty
```

Stop on `migration_required`, `migration_in_progress`, replacement divergence, a stale cursor, a
missing artifact, or any source mismatch.

Read the review with exact bounded selectors:

```bash
pointbreak observation list --exact-revision "$revision_id" \
  --track "$reviewer_track" --include-body --format json-pretty
pointbreak validation list --exact-revision "$revision_id" \
  --track "$reviewer_track" --include-body --format json-pretty
pointbreak input-request list --exact-revision "$revision_id" \
  --track "$reviewer_track" --status all --include-body --format json-pretty
pointbreak assessment show --exact-revision "$revision_id" \
  --track "$reviewer_track" --include-summary --format json-pretty
```

## Classify the verdict

Treat `needs-changes`, `needs-clarification`, and unanswered operative requests as actionable.
Treat accepting calls with only advisory follow-up as non-blocking triage. Never manufacture a code
change merely to create another round.

## Respond to advisory requests

Use `pointbreak input-request respond` for a decision-seeking advisory request. An observation may
explain the answer, but does not replace the structured response.

## Record author response observations

If the response is prose, a structured request response, or another fact that does not alter the
captured content, keep the same review cursor. Write only on the author track.

```bash
pointbreak observation add --review-cursor "$review_cursor" --track "$author_track" \
  --title "<response>" --body "<concise rationale>"

pointbreak input-request respond <request-id> \
  --outcome <approved|rejected|dismissed|superseded|abandoned> \
  --reason "<structured answer>"
```

Do not manufacture source changes after an accepted review merely to produce activity.

## Content-changing response

If implementation, tests, documentation, generated files, modes, untracked content, or capture scope
changes, the old Revision remains an honest historical state. Capture the new state in the same
Change with explicit intent:

```bash
next_file=$(mktemp)
pointbreak capture \
  --review-cursor "$review_cursor" --advance replace \
  --summary "<updated coherent state>" \
  | tee "$next_file" | jq .

next_revision_id=$(jq -r '.revision.revisionId' "$next_file")
next_artifact_hash=$(jq -r '.revision.objectArtifactContentHash' "$next_file")
next_review_cursor=$(jq -r '.reviewCursor.token' "$next_file")
rm "$next_file"
```

Use `--advance parallel` only when both exact Revisions intentionally remain current. A cumulative
fix is `replace`. Consolidation adds repeated exact
`--also-supersedes REVISION_ID@OBJECT_ARTIFACT_SHA256` values. Never use legacy proposal-borne
`--supersedes`.

After capture:

1. make `next_review_cursor` the only cursor for new-state writes;
2. rerun every validation the new review call will rely on;
3. record those checks on the new Revision;
4. return the new exact Revision and cursor to the reviewer for a fresh assessment.

The prior assessment remains attached to the prior Revision. Do not replace or port it.

## Explicit context continuity

Facts do not automatically move. Port an observation or input request only when its continuity is
useful and explicit:

```bash
pointbreak fact port \
  --origin-revision "$revision_id@$artifact_hash" \
  --origin-fact <observation-or-input-request-id> \
  --review-cursor "$next_review_cursor" \
  --relation context-only --track "$author_track"
```

Use `reanchored-as`, `carried-open-as`, or `resolved-by` only with the required exact target fact.
The origin fact remains owned by the old Revision. There is intentionally no validation or
assessment port.

## Landing and rewrite evidence

A commit that materializes already-reviewed content does not create a new Revision. When recording
an authorized landing or evaluating a single-commit rewrite, read the
[landing and rewrite procedure](references/landing.md). It owns commit-source reselection,
proof-first association, and candidate evidence. Structural provenance alone does not prove content
equivalence. Acceptance and proof do not authorize publication or merge by themselves.

## Hard rules

- Never assess from the author role.
- Never record post-edit facts or checks on the pre-edit Revision.
- Never port validation or assessments.
- Never infer a new current Revision or choose among multiple currents by time or lexical order.
- Never treat operation-recovery files as semantic authority.
- Never let landing create a new Revision when the reviewed content is proved unchanged.
