---
name: pointbreak-author
description: Create a durable author handoff for one exact Pointbreak Revision inside a stable Change. Capture the coherent content state, write author facts through its review cursor, record only validation that ran against that exact state, and never self-assess.
---

# Pointbreak Author Handoff

You are the coding agent that just authored the change. Create one content-truthful handoff. A
`ChangeId` is the stable multi-round work session; a
`RevisionRefV1` is one immutable content state. Observations, validation, requests, and assessments
belong to an exact Revision and never float to the Change.

Do not run `pointbreak assessment add`; the Call (`assessment`) belongs to the reviewer.

## Capability preflight

Run this before any normal review operation:

```bash
pointbreak change profile --repo . --format json-pretty
```

Proceed only when the profile is `ready`. On `migration_required` or `migration_in_progress`, stop
and report the typed guidance. This skill never plans, starts, resumes, or activates a migration.

## Capture The Right Revision

Inspect the intended source first. Capture an uncommitted worktree state, or use `--base` for an
already committed range. Include untracked files only when they are part of the coherent change.
The captured state is visible with `pointbreak inspect --open` before review facts are added.

```bash
git status --short --branch

capture_file=$(mktemp)
pointbreak capture --summary "<concise change summary>" \
  | tee "$capture_file" | jq .

change_id=$(jq -r '.changeId' "$capture_file")
revision_id=$(jq -r '.revision.revisionId' "$capture_file")
artifact_hash=$(jq -r '.revision.objectArtifactContentHash' "$capture_file")
review_cursor=$(jq -r '.reviewCursor.token' "$capture_file")
rm "$capture_file"
```

For committed work, add `--base <commit-before-work>` (and `--target <commit>` when necessary).
Never rewrite Git history to manufacture a worktree diff. Retain the complete receipt, especially
the operation ID and review cursor, until the handoff is finished.

The first capture creates an independent Change. Do not infer shared Change identity from branch,
PR, ancestry, paths, task label, or identical bytes.

## Choose your track and identity

Use one author track for the handoff. Tracks distinguish review lanes; the actor ID is the durable,
run-independent writer identity.

```bash
agent_name="<canonical-agent-name>"
run_id="<short-run-id>"
track="agent:${agent_name}-${run_id}"
export POINTBREAK_ACTOR_ID="actor:agent:${agent_name}"
```

Keep both identifiers lowercase and hyphenated. Signing is automatic and advisory; never disable or
bypass the handoff because a key is unavailable.

On the first write under an `actor:agent:*` identity, Pointbreak creates or reuses the agent key.
Until a human enrolls it, signed events display as untrusted; untrusted does not mean invalid.
Enrollment is optional. `pointbreak key enroll <name>` stages the signer in
`.pointbreak/allowed-signers.json` for human review. A human may instead adopt an existing SSH key
with `pointbreak key use-ssh`. Signing never gates an ordinary write; `POINTBREAK_SIGNING=off` is the
explicit opt-out.

## Record observations

Use the review cursor for every new write. It binds the Change graph and exact Revision artifact and
refuses stale or mismatched source state.

```bash
pointbreak observation add \
  --review-cursor "$review_cursor" --track "$track" \
  --title "<what changed or why>" \
  --body-content-type text/markdown \
  --body "<concise reviewer context>"
```

A pre-implementation Red result is context, not final-state validation: “That pre-change failure did not run against the captured revision” is the reason to record it as an observation.

## Record validation evidence

```bash
pointbreak validation add \
  --review-cursor "$review_cursor" --track "$track" \
  --check-name "<check>" --status passed \
  --command "<exact command>" --exit-code 0 \
  --summary "Passed after the final edit against this exact Revision."
```

## Open input requests

```bash
pointbreak input-request open \
  --review-cursor "$review_cursor" --track "$track" \
  --title "<decision needed>" --reason manual-decision-required \
  --mode advisory --body "<why another actor must decide>"
```

Use file/range anchors when a fact is local. Use Markdown content types when bodies name code. Do
not paste transcripts, record checks you did not run, or convert a pre-implementation Red result
into validation for the final Revision; record that Red result as an observation.

Validation is exact-state evidence. If any content changes after capture, do not write new validation
or observations through the old cursor. Advance the Change as described by the author-response skill.

## Read back and hand off

Use bounded, exact selectors. `--revision` is legacy navigation and may follow replacement; capable
agent workflows do not use it for fact readback.

```bash
pointbreak observation list --exact-revision "$revision_id" \
  --track "$track" --include-body --format json-pretty
pointbreak validation list --exact-revision "$revision_id" \
  --track "$track" --include-body --format json-pretty
pointbreak input-request list --exact-revision "$revision_id" \
  --track "$track" --status open --include-body --format json-pretty
```

Then report the Change ID, exact `REVISION_ID@OBJECT_ARTIFACT_SHA256`, review cursor, and author track.
State explicitly that you did not add an assessment.

## Landing boundary

A commit that materializes already-reviewed content does not create a new Revision. Use the
proof-first wrapper after the reviewer accepts the exact Revision:

```bash
pointbreak association land \
  --review-cursor "$review_cursor" --track "$track" --commit <commit>
```

Strong language such as exact, equivalent, contained, or landed unchanged is permitted only when
this command returns a verified relation. `pointbreak association record` is the low-level structural
provenance escape; it does not prove content equivalence. `--provenance-only` records the same honest
limited claim when proof inputs are unavailable.

## Hard rules

- Do not self-assess.
- Do not use a stable Change as a fact target.
- Do not keep writing to a Revision after source content changes.
- Do not use proposal-borne `--supersedes`; advancement is a separate Change-scoped relation.
- Do not port validation or assessments between Revisions.
- Do not migrate or activate a store from this skill.
- Do not recapture merely because an unchanged reviewed state was committed; prove and associate it.
