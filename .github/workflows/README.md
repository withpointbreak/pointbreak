# Workflow strategy

Workflows are split so pull requests get fast, complete signal on every change, while broader or
slower verification runs on a schedule. The full decision table for which local gate applies to a
change lives in `docs/development.md`; this note only records where each workflow sits and where a
new check belongs.

## Per-push (`ci.yml`)

Runs on every pull request and every push to `main`: lint, type checks, the default test suite,
the qualification smoke, and the per-surface checks (installer, skills, web, extension,
workflows, dependency audit). Jobs guarded by a condition render as `skipped` rows rather than disappearing, so a
reviewer can always see what did not run and why — the heavy jobs skip a `push` run only when the
`guard` job resolved the identical SHA as already green from its pull-request run.

## Scheduled (`nightly.yml`)

Runs once a day, and every job in it also carries `workflow_dispatch` so it can be triggered by
hand (`gh workflow run nightly.yml`) — a scheduled lane nobody can rerun on demand is a lane
nobody can debug. Hosts the full feature-on suite (`just test-full`), which no per-push lane
executes, the report-only subprocess-vs-gix parity harness (`just git-parity`), and the
cargo-deny advisory check — advisory failures are time-triggered (a new RUSTSEC entry, no code
change), so they belong here while the deterministic license/ban/source checks run per-push. Pushing a
branch whose name contains `full-ci` also runs the workflow, so the full suite can be exercised
on a risky change before it merges.

## Event-driven and scheduled (`project-sync.yml`)

Mirrors the planning labels (`priority:*`, `effort:*`, `status:*`, `research`, `tracking`) into the
org project board's Priority, Effort, and Workflow fields, and guards those labels: one per
namespace, changed only by people with write access. An `issues` event syncs that issue; the daily
run and `workflow_dispatch` reconcile every open issue, so a missed webhook heals within a day.
Needs the `PROJECT_SYNC_TOKEN` repository secret (a fine-grained token with the organization
permission Projects: read and write and repository Issues: read); without it the sync step fails
loudly and the guard still runs. Theme on the board is hand-set and never touched. The label
vocabulary itself is documented in `CONTRIBUTING.md`.

## Release (`release-plan.yml`, `release.yml`, `release-binaries.yml`, `verify-release.yml`)

Triggered by the release process, not by pushes; see `docs/releasing.md`.

## Where a new check belongs

- **Per-push** if it guards the correctness of every change.
- **Per-push behind a change-set condition** if it guards one surface; fail open — when the
  change set cannot be computed, run the job.
- **Scheduled** if it is broad, slow, or report-only; give it `workflow_dispatch` and a
  `timeout-minutes` bound like every other job.
- **Event-driven** if it reacts to repository metadata (issues, labels) rather than to code; keep
  it metadata-only, give it a daily reconcile so a missed event heals, and use only the tools on
  the runner image.

Text guards in `tests/github_actions.rs` pin this topology (job tiers, timeouts, the nextest
version, trigger shapes). Changing where or when a job runs means updating those guards in the
same change — that is deliberate.
