---
name: pointbreak-reviewer
description: Review one exact Pointbreak Revision within a stable Change. Read and write through exact selectors, inspect the frozen content state independently, record reviewer facts and validation on a separate track, and assess only that Revision.
---

# Pointbreak Reviewer Handoff Review

You are the reviewing agent. Review one immutable Revision, not an ambient checkout and not a stable
Change as a whole. The Change
keeps multi-round continuity; the review cursor binds this pass to one exact Revision and artifact.

## Inputs and capability

Require the Change ID, exact Revision ID and artifact hash, author track, and review cursor from the
author receipt. Run:

```bash
pointbreak change profile --repo . --format json-pretty
pointbreak change show "$change_id" --repo . --format json-pretty
```

Proceed only on `ready`. Stop on migration guidance, a stale cursor, missing content, replacement
divergence, or a source mismatch. Never migrate or activate a store from this skill.

## Read the author's handoff

Read only the exact author handoff:

```bash
pointbreak observation list --exact-revision "$revision_id" \
  --track "$author_track" --include-body --format json-pretty
pointbreak validation list --exact-revision "$revision_id" \
  --track "$author_track" --include-body --format json-pretty
pointbreak input-request list --exact-revision "$revision_id" \
  --track "$author_track" --status open --include-body --format json-pretty
```

`--revision` is a legacy head seed and may follow replacement. Do not use it for capable review
readback or writes.

## Choose your track and identity

Choose a separate reviewer track and stable actor identity:

```bash
agent_name="<canonical-agent-name>"
run_id="<short-run-id>"
reviewer_track="agent:${agent_name}-${run_id}"
export POINTBREAK_ACTOR_ID="actor:agent:${agent_name}"
```

On the first write under an `actor:agent:*` identity, Pointbreak creates or reuses the agent key.
Until a human enrolls it, signed events display as untrusted; untrusted does not mean invalid.
Enrollment is optional. `pointbreak key enroll <name>` stages the signer in
`.pointbreak/allowed-signers.json` for human review. A human may instead adopt an existing SSH key
with `pointbreak key use-ssh`. Signing never gates an ordinary write; `POINTBREAK_SIGNING=off` is the
explicit opt-out.

## Review independently

Inspect the immutable captured resource or a source state that the cursor proves matches it. Treat
the author's facts as navigation, not proof. Run targeted checks first and broaden validation in
proportion to risk.

If the live checkout differs from the captured Revision, do not assess the live bytes as though they
were the frozen Revision. Ask the author for a new replacement/parallel capture, or review the
captured resource directly. A commit-only landing does not require recapture when the proof-first
landing contract verifies the reviewed relation.

## Record reviewer findings

All reviewer writes use the review cursor and reviewer track:

```bash
pointbreak observation add \
  --review-cursor "$review_cursor" --track "$reviewer_track" \
  --title "<finding>" --body-content-type text/markdown \
  --body "<evidence and impact>"
```

## Record reviewer validation checks

```bash
pointbreak validation add \
  --review-cursor "$review_cursor" --track "$reviewer_track" \
  --check-name "<check>" --status passed \
  --command "<exact command>" --exit-code 0 \
  --summary "Ran against the exact reviewed state."
```

Use an advisory input request when the author must decide a non-blocking follow-up. Respond to an
operative author request only when it is genuinely answered.

## Respond to operative input requests

Use `pointbreak input-request respond` only when the operative request is genuinely answered.
Never write to the author's track.

## Add exactly one assessment

```bash
pointbreak assessment add \
  --review-cursor "$review_cursor" --track "$reviewer_track" \
  --assessment <accepted|accepted-with-follow-up|needs-changes|needs-clarification> \
  --summary "<exact-state review call>"
```

The assessment applies only to the cursor's Revision. If the author changes content, that assessment
remains historical and the replacement/parallel Revision needs fresh validation and a fresh
assessment. Do not port it. `--replaces` is only for revising a call on the same exact Revision, not
for carrying a call forward to new bytes.

Review loops may preserve the Change ID, author/reviewer tracks, and external session state across
rounds. They must replace the review cursor whenever content changes.

This reviewer role alone must make the Call for the exact Revision.

## Exact readback

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

Then report the Change ID, exact Revision reference, reviewer track, assessment, and open requests.
Stand down; implementation belongs to the author role.

## Hard rules

- Do not review or assess an ambient live checkout under a mismatched cursor.
- Do not reuse an old Revision after source content changes.
- Do not port validation or assessments between Revisions.
- Do not infer current state from timestamps, branches, tracks, or associations.
- Do not claim an exact/equivalent/contained landing from structural association alone.
- Do not add more than one current assessment per reviewer pass on the exact Revision.
