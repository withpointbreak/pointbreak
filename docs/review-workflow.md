# Review Workflow

This document explains how to interpret a Pointbreak review: the five stages every review moves
through, the roles that write into it, and when to reach for each command family. The canonical
executable journey — install to first Review to the complete paired author/reviewer loop — lives in
[getting-started.md](getting-started.md); command reference details live in
[cli-reference.md](cli-reference.md). If the change was authored by a coding agent, start with
[Agent authoring handoffs](agent-authoring.md) for the capture-at-end-of-work loop.

## The five stages

Every review answers five questions, in this order:
`Work -> Claims -> Evidence -> Questions -> Call`. Each stage is owned by one existing flattened
command family:

| Stage | The question it answers | Command family |
| --- | --- | --- |
| Work | What changed? | `capture`, `change`, `revision`, `inspect` |
| Claims | What does an author or reviewer assert? | `observation` |
| Evidence | What was checked? | `validation` |
| Questions | What still needs judgment? | `input-request` |
| Call | What is the current assessment? | `assessment` |

Two supporting nouns complete the picture: `attention` lists the outstanding judgment across
stages, and `association` records where the reviewed work landed. Review — the local web surface
opened by `pointbreak inspect --open` — is read-only and advisory: it renders the durable record
and never executes commands or writes to the store.

### Roles

Two review lanes do the work, and a human owns the outcome:

- **Author handoff** — the actor that made the change captures it and records claims, real
  validation evidence, and genuine open questions. Packaged for coding agents as the
  `pointbreak-author` skill.
- **Reviewer pass** — a second actor reads before writing, records its own findings and checks on
  its own track, asks questions, and makes exactly one current call. Packaged as
  `pointbreak-reviewer`.
- **Author response** — the author answers reviewer questions where they were asked, fixes what an
  actionable call requires, and records response context. It never assesses its own work. Packaged
  as `pointbreak-author-response`.

The roles describe review lanes, not headcount: one human can play all three, or two agents can
pair while a human reads the record and owns the decision.

## What Pointbreak reviews

Pointbreak coordinates multi-round review through a stable **Change**. Each state of that work is an exact
**Revision**: the base endpoint, target endpoint, and captured diff snapshot taken at one moment. Facts,
validation, requests, assessments, and associations stay attached to that immutable Revision; Change owns
only attributed membership and contextual relations between exact Revisions.

Capturing a Revision is the generative move in the workflow, while "review" stays the surface verb. V1
captures several Git-backed shapes: the local Git worktree from `HEAD` to
the working tree (the default, excluding untracked files unless `--include-untracked` is passed); the
committed range between two resolved commits with `pointbreak capture --base <rev>`
(`<rev>..--target`, target defaulting to `HEAD`), read as a tree diff with no working-tree
involvement; or explicit index-boundary captures with `pointbreak capture --staged` and
`pointbreak capture --unstaged`.

Each revision binds an immutable **object artifact** by content hash. The artifact body is
content-only, so two revisions capturing the same change in different worktrees share one
byte-identical artifact — they converge on a single **object** — rather than each owning a distinct
copy. The revision is the captured unit's identity; the object is a hash of its content alone.
Anything you record afterwards — observations, input requests, assessments — attaches to that
revision and lives in the durable `events/` log.

A later capture advances an exact Change cursor as either a replacement or intentional parallel Revision.
Replacement is an attributed Change-scoped relation between exact Revision/artifact pairs, not a field on
the new Revision proposal. Successive rounds never mutate captured snapshots. A Change may have several
valid current Revisions; successors that replace intersecting predecessor ancestry are surfaced as
`replacement_divergent` rather than silently reduced to one winner.

## The workflow at a glance

1. Start from a Git worktree containing the change you want to review.
2. Capture a revision with `pointbreak capture --summary`.
3. Open Review with `pointbreak inspect --open`, or use the scriptable reads
   (`pointbreak change show`, `pointbreak change revision`, `pointbreak history`).
4. Record review facts as the review moves through the stages:
   - **Observations** are the claims an author or reviewer wants preserved.
   - **Validation checks** are evidence that a command actually ran.
   - **Input requests** are durable pause/decision requests for someone else.
   - **Assessments** are the current review call for the revision (or for a
     file, range, or specific fact within it).
5. When unchanged reviewed content lands as a commit, select that commit against the exact Revision and use
   proof-first `pointbreak association land`.

The rest of this document walks through each step.

## 1. Start in a worktree with the change

Pointbreak runs inside a Git worktree. For the default capture the working tree
must differ from `HEAD`; the change can come from anywhere — a coding agent, a
teammate's WIP branch, your own edits — but it must be present in the working
tree before capture. Pointbreak reads the diff from `git`; it does not summarize
prior commits on its own. If the change is already committed, capture the
committed range directly with `pointbreak capture --base <rev>` (see
[section 2](#2-capture-a-revision)) instead of recreating it in the working
tree.

```bash
cd path/to/worktree
git status        # confirm the changes you expect are present
```

By default the store is the shared common-dir store at `<git-common-dir>/pointbreak`, under the clone's Git common
directory — every worktree of the clone resolves the same store, and because it lives inside `.git`
it never appears in `git status`, so ordinary captures never touch the working tree at all. Opting
into an ephemeral worktree (`pointbreak store mode ephemeral`) or writing a `--local` identity override
generates a committed `.pointbreak/.gitignore` (two lines: `data/` + `*.local.json`) that keeps the
worktree-local store and the private overrides out of `git status`; the file is visible, meant to
be committed, and survives clone. If the paths are already ignored — for example by a project
`.gitignore` entry — Pointbreak generates nothing and leaves your ignore files untouched. Nothing
writes the hidden `.git/info/exclude` anymore.

## 2. Capture a revision

```bash
pointbreak capture --summary "Explain the fallback behavior"
pointbreak capture --include-untracked --summary "Add the initial untracked files"
```

`pointbreak capture` records the exact Revision plus its initial Change claims and writes the captured
snapshot as an immutable Pointbreak-owned object artifact. The
`pointbreak.change-capture-receipt.v1` output includes:

- the stable Change ID
- the revision ID
- the optional human-readable summary used by discovery surfaces
- the object ID (the content-only identity)
- the object artifact's canonical content hash
- a self-hashed `ReviewCursorV1` token for subsequent exact writes or capture advancement

Pass the receipt's cursor to high-level fact writers with `--review-cursor`. Re-select it with
`pointbreak change select <change-id> --revision <revision-id>` when you need a fresh cursor; if several
Revisions are current, the explicit Revision is required.

The snapshot and capture summary are now frozen. Capturing changed content later creates a new
revision; it does not mutate the previous one. Rerunning identical content with a different summary
is rejected because immutable capture metadata cannot be edited in place.

Default worktree capture is a combined `HEAD` to working-tree capture: staged
and unstaged tracked changes are both included because both differ from `HEAD`.
It follows `git diff HEAD` for untracked files, so untracked paths are ignored
unless you opt in with `--include-untracked`. In a repository with no commits
yet, `pointbreak capture --include-untracked` captures untracked initial files from
Git's empty tree to the working tree; `--root` is for capturing a committed
target from the empty tree.

If the selected source has no changed files, capture returns an error instead
of recording an accidental empty revision. The message suggests relevant flags
such as `--include-untracked`, `--staged`, or `--unstaged`. Pass
`--allow-empty` only when an empty revision is the intended review object.

When the index boundary matters, capture it explicitly:

```bash
pointbreak capture --staged
pointbreak capture --unstaged
pointbreak capture --unstaged --include-untracked
```

`--staged` records the current commit (or Git's empty tree in a repository with
no commits) to the captured index tree. `--unstaged` records the captured index
tree to the working tree, matching plain `git diff`: staged changes are in the
base endpoint, and untracked files are excluded unless `--include-untracked` is
present.

### Capturing a committed range

When the change is already committed and the working tree is clean, capture the
landed range instead of recreating a working-tree diff:

```bash
pointbreak capture --base <commit-before-the-change>   # target defaults to HEAD
pointbreak capture --base <rev> --target <rev>         # explicit range
```

`--base`/`--target` resolve any rev (a branch, tag, `HEAD~N`, or commit OID) to a
commit; annotated tags peel, and a non-commit or unknown rev is rejected with an
honest error. The capture is the `base..target` tree diff — both endpoints are
`git_commit`, no working-tree or untracked state is read, and no worktree path
appears in the output. This is the supported way to review after landing: never
rewrite history (for example `git reset --soft`) to manufacture a worktree diff.

A post-landing range capture is a second current revision alongside any
worktree capture, so disambiguate later reads and writes with `--revision
<id>`, exactly as for any multi-capture store. Recording the
landed commit, choosing a canonical capture, and revision lifecycle remain open
follow-ups.

### Scoping a capture to a subtree

In a monorepo a review is usually about one subtree, but the worktree and the
commits interleave changes across many. Scope the capture with `--path` so
unrelated changes stay out of the reviewer's diff and out of the revision's
identity:

```bash
# review only what changed under packages/foo, in the current worktree
pointbreak capture --path packages/foo

# same, over an explicit range
pointbreak capture --base v1.2.0 --target HEAD --path docs/spec
```

`--path` takes a native git pathspec, is repeatable, and scopes tracked files and
any enabled untracked-file synthesis alike; a scope that matches no changed
files is an error unless `--allow-empty` is passed. See
[`pointbreak capture`](./cli-reference.md#pointbreak-capture) for the full
semantics.

Advance the exact cursor returned by capture or `pointbreak change select`:

```bash
pointbreak capture --review-cursor <cursor-token> --advance replace
pointbreak capture --review-cursor <cursor-token> --advance parallel
```

`replace` relates the new exact Revision to the cursor's selected predecessor. `parallel` adds an
intentional current Revision without replacement. Consolidation can name additional exact current
predecessors with repeated `--also-supersedes <revision-id>@<artifact-sha256>` arguments. The legacy
proposal-borne `--supersedes` option is rejected.

Write commands such as `pointbreak observation add`,
`pointbreak input-request open`, and `pointbreak assessment add` accept
`--revision <id>`. High-level Change writers use a `ReviewCursorV1` so the Change graph, exact
Revision/artifact, and source state are revalidated immediately before append. When several Revisions are
current, select one explicitly; replacement divergence, cycles, incomplete authority, or stale source state
fail closed. Exact-Revision reads remain valid for historical members.

## 3. Inspect what was captured

Four read surfaces answer different questions:

```bash
pointbreak change list
pointbreak change show <change-id>
pointbreak change revision <change-id> <revision-id> --artifact-hash <sha256>
pointbreak history
```

For a visual, cross-linked view of the whole store — an event timeline, stable Change pages, exact Revision
resources, and captured diffs annotated with their review facts — run
`pointbreak inspect` to open a local web UI (see the [CLI reference](cli-reference.md)). The commands
below remain the scriptable surface.

`change list` discovers stable work. `change show` reports attributed membership, current-set topology,
lifecycle, diagnostics, and the contextual relations that produced it. `change revision` is the exact
Revision/artifact read and never follows a floating head. Use `change interdiff` for a pair of exact members.
The legacy aggregate `revision list` and `revision show` documents are not Change-capable readers; an L2
store may still serve those CLI views, but they report neither Change membership nor contextual current-set
topology. The Inspector's legacy aggregate HTTP routes return typed `426 Upgrade Required` rather than
partial Change semantics.

### `pointbreak history`

`pointbreak history` is the chronological raw-event listing across the
entire `events/` log — across revisions if there is more than one.
It is the place to answer "what happened, in what order?" rather than
"what does this revision look like right now?".

```bash
pointbreak history --format json-pretty
pointbreak history --event-type review-observation-recorded
pointbreak history --revision <id> --include-body
```

`eventCount` describes the full validated event set used to build the document,
even when filters return only a subset of entries. An eligible bounded page
served by the current derived generation carries `projectionStamp`; an
authoritative read carries `eventSetHash`. Both are opaque snapshot identities.
History preserves duplicate semantic events as separate entries; it does not
collapse them or pick "winners".

## 4. Record review facts

The three event families below are append-only. Each writes one durable event
per call. Read surfaces collapse same-semantic-ID writes to one logical row
and surface a duplicate diagnostic.

### Observations

An observation is a durable note for a revision, a file, or a line range.
Observations are append-only; corrections are new observations that name the
older observation through `--supersedes`.

```bash
# Review-wide observation
pointbreak observation add \
  --track agent:codex \
  --title "Check error handling near IO boundary"

# File-targeted observation
pointbreak observation add \
  --track agent:codex \
  --title "Untrusted input flows here" \
  --file src/lib.rs

# Range-targeted observation, with a body from a file
pointbreak observation add \
  --track human:kevin \
  --title "Worth a unit test" \
  --file src/lib.rs --start-line 42 --end-line 58 \
  --body-file notes/lib-42.md

# Replay observations for one track
pointbreak observation list --track agent:codex --format json-pretty

# Include bodies on read
pointbreak observation list --include-body
```

Bodies may come from `--body`, `--body-file`, or `--body-stdin`. Large bodies
are stored as Pointbreak-owned content-addressed artifacts; command output never
exposes those paths.

### Validation evidence

A validation check records that a command actually ran against the captured content and what it
reported. It is evidence, not a verdict: it never accepts, rejects, merges, blocks, or replaces the
reviewer's assessment, and a green check does not close a review.

```bash
git diff --check
pointbreak validation add \
  --track agent:codex \
  --check-name "git diff --check" \
  --status passed \
  --command "git diff --check" \
  --exit-code 0 \
  --summary "The captured tracked change has no whitespace errors."

pointbreak validation list --include-body
```

Record only checks that actually ran; a deliberately skipped check is recorded as `skipped` with a
summary that says why. Validation checks target the whole revision — when the reasoning around a
check belongs to a specific file or range, record that as a separate anchored observation.

### Input requests

An input request is a durable pause/decision request. Use it when a reviewer
or tool needs an explicit answer before proceeding. `--mode` defaults to
`operative`; `advisory` requests are still durable and visible but do not
imply that a cooperative client must pause.

```bash
pointbreak input-request open \
  --track human:kevin \
  --title "Need approval to land schema change" \
  --reason manual-decision-required

pointbreak input-request list                 # defaults to open
pointbreak input-request list --status all
pointbreak input-request show <input-request-id> --include-body

pointbreak input-request respond <input-request-id> \
  --outcome approved \
  --reason "discussed in chat, ok to land"
```

`--reason` on the request is the classification axis (`manual-decision-required`,
`ambiguous-state`, `unsafe-action`, `insufficient-evidence`, etc.). `--outcome` on
the response is a separate axis (`approved`, `rejected`, `dismissed`, `superseded`,
`abandoned`).

Multiple different response events are preserved as append-only facts and
make the input request `ambiguous` rather than picking a timestamp winner.

### Assessments

An assessment is the current review call for a revision, a file, a range,
or a specific native observation/input request/assessment in the same
revision. CLI input and human-facing display use `accepted`,
`accepted-with-follow-up`, `needs-changes`, and `needs-clarification`; command
JSON output uses the matching `snake_case` values.

```bash
pointbreak assessment add \
  --track human:kevin \
  --assessment accepted \
  --summary "looks good, ship it"

# Assessment that replaces an older one
pointbreak assessment add \
  --track human:kevin \
  --assessment accepted-with-follow-up \
  --summary "supersedes earlier needs-changes after offline discussion" \
  --replaces <older-assessment-id>

pointbreak assessment show --format json-pretty
pointbreak assessment show --all --include-summary
```

`--replaces` is the only V1 relationship that removes an older assessment
from the current set. Replacement is never implicit — even for the same actor,
a revised call must name the assessment it retires, and two unreplaced
assessments read as competing candidates that surface as an
`ambiguous_assessment` attention item. An `accepted-with-follow-up` recorded
while no input request is open on the revision carries an advisory
`assessment_unlinked_follow_up` diagnostic: the label alone creates no durable
actionable state, so open the follow-up as an advisory input request unless
the label is deliberately prose-only.
`--related-observation` and `--related-input-request` record evidence links;
they do not mutate observations or close input requests (use
`pointbreak input-request respond` for the input-request lifecycle).

State-change outcomes such as deferred, split-out, overridden, and superseded
are recorded as observations tagged with `state-change:*`. Use
`pointbreak assessment` for review calls and `pointbreak observation add`
with a concrete tag such as `--tag state-change:deferred` for state-change
evidence.

### Attention

`pointbreak change attention` surfaces what still needs an actor's judgment across the store's stable
Changes: open asks, ambiguous assessments, unresolved current members, failed checks, and outstanding
follow-ups. It is a projection over the same durable facts the commands above record — nothing new is
written by reading it.

```bash
pointbreak change attention
```

Attention *guides, never gates* (ADR-0019): the list is derived attention
state, never a write precondition; a cooperative actor uses it to decide where
to look next, and Pointbreak never blocks a write on it. "Attention" is
promoted here from internal substrate vocabulary (ADR-0019's notification
seam) to a product-level term for this surface.

How items clear — every clearing action is a durable fact about the work,
never a fact about the queue (ADR-0019's judgment-subsumption amendment):

- `open_input_request` / `follow_up_outstanding` — respond with
  `pointbreak input-request respond` (the `dismissed` outcome closes a moot ask).
- `ambiguous_assessment` — record an assessment that `--replaces` the competing records on that exact
  Revision.
- stale or unresolved current state — capture a replacement or parallel Revision through the exact Change
  cursor, then record new-state validation and assessment on the new exact Revision.
- `failed_validation` — record a strictly-later passing run of the same
  check (`skipped` never clears), replace the Revision, or record a later,
  unanimously accepting judgment on it: a reviewer who accepts a revision
  with the failure in evidence has rendered the judgment the item was
  waiting for.
- replacement divergence — use explicit Change relation claims to reconcile or consolidate the conflicting
  current set; Pointbreak never chooses a winner by time.

### Landing: commit association

When unchanged reviewed content lands as a commit, select a commit-bound cursor and prove the landing before
recording its structural association:

```bash
landing_cursor=$(pointbreak change select <change-id> \
  --revision <revision-id> --source commit:<oid> \
  --format json | jq -r '.token')
pointbreak association land --review-cursor "$landing_cursor" \
  --track agent:codex --commit <oid>
pointbreak association list --axis commit --current
```

A proved landed commit is an association on the same exact Revision — unchanged reviewed content is
never a recapture or replacement, even when the commit lands after assessment. If reviewable bytes or
capture scope changed, capture a new Revision in the same Change before recording new-state facts or an
assessment. The low-level `association record` command remains available for explicitly unverified
structural provenance; it cannot authorize exact/equivalent/extension wording.

## 5. Concepts you need to know

### Durable event facts vs. rebuildable projections

Pointbreak separates **authoritative facts** from **derived views**. The paths below are relative to
the resolved store directory — `<git-common-dir>/pointbreak` by default, or a worktree-local `.pointbreak/data/` when the
worktree is ephemeral:

- `events/` is the authoritative append-only log. Each file is one
  immutable durable fact. Events are never moved, retried in place, or
  rewritten on read.
- `artifacts/` holds the immutable support records that events bind to:
  captured revision object artifacts, and the optional content-addressed bodies
  for large observation, input request, and assessment payloads.
- `state.json` is a **rebuildable projection**, not the authority. It
  may be deleted and regenerated; freshness against the current event set is
  verified through `eventSetHash`.

If `state.json` looks stale or inconsistent, Pointbreak rebuilds it from
the event log. Do not write to `state.json` yourself, and do not depend on
its internal shape.

### Command-output JSON is the integration surface

The stable surface for automation is **command-output JSON documents**:
`pointbreak.review-capture`, `pointbreak.review-history`, `pointbreak.review-revision`,
`pointbreak.review-observation-add` / `-list`,
`pointbreak.review-input-request-open` / `-list` / `-show` / `-respond`,
and `pointbreak.review-assessment-add` / `-show`.

These documents expose semantic IDs, content hashes, and freshness metadata.
Raw event files, event filenames, artifact paths, and `state.json` are
Pointbreak-owned storage details. They can change without a deprecation cycle.

### Tracks

Every observation, input request, and assessment belongs to a required
`--track`. Tracks are **review lanes**, such as `agent:codex` or
`human:kevin`. They are not actor identity. Writer provenance — who actually
ran the command — is recorded separately in the event envelope: the writer
`actorId` (from local Git config, or an explicit `actor:agent:<name>` set via
`POINTBREAK_ACTOR_ID`) and the `producer` that wrote the event. The human a resolved
agent acts on behalf of comes from the checked-in `.pointbreak/delegates.json` map at
read time. Pick track names that group facts the way you want to read them back,
then let provenance take care of itself.

### Bodies

Observation bodies, input request bodies, input request response
reasons, and assessment summaries all share the same input mechanics:
`--body` / `--body-file` / `--body-stdin` (or `--summary*` /
`--reason*`). Read commands omit body-like text by default and hydrate it
only when `--include-body` is passed. Small bodies stay inline in the event
payload; larger bodies move to content-addressed artifacts. From a user
perspective, the difference is invisible — read commands return the same
shape either way.

### IDs are opaque

Pointbreak exposes several kinds of IDs in its output: revision IDs, object
IDs, observation IDs, input request IDs, input request response
IDs, assessment IDs, event IDs, and review-stream row IDs. **Treat them all
as opaque strings.** They are stable and safe to use as keys or to pass back
into other commands, but their internal format is an implementation detail.
This opacity is load-bearing: a content id is derived from content, so two clones
capturing identical content converge on the same revision and object IDs without
coordinating, and a store migration may rename the on-disk files that hold them
while the IDs themselves stay valid and unchanged. Read the IDs; never parse them.
In particular:

- Do not parse review-stream `row.id` values, derive ordering from them
  lexically, or assume any particular width or prefix. Use the sibling
  `ordinal` field if you need a numeric position.
- Do not parse storage filenames. Event filenames, object artifact
  filenames, and note-body artifact filenames are derived from internal
  hashes and may change without a deprecation cycle.
- Do not depend on artifact paths or the internal shape of
  `state.json`.

## 6. The canonical walkthrough

The end-to-end transcript — install to capture to Review, the complete paired author/reviewer
loop, and the same-revision landing — lives in [getting-started.md](getting-started.md). This
document stays the interpretation layer; when the two disagree, the walkthrough and its executable
contract are authoritative.

Anything beyond this workflow — notifications, daemons, multi-writer coordination, automatic
delivery — is intentionally out of scope and will be addressed by future, separately-designed
subsystems.
