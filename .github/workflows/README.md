# Workflow strategy

Workflows are split so pull requests get fast, complete signal on every change, while broader or
slower verification runs on a schedule. The full decision table for which local gate applies to a
change lives in `docs/development.md`; this note only records where each workflow sits and where a
new check belongs.

## Per-push (`ci.yml`)

Runs on every pull request and every push to `main`: lint, type checks, the default test suite,
the qualification smoke, the parity harness, and the per-surface checks (installer, skills, web,
extension, workflows). Jobs guarded by a condition render as `skipped` rows rather than
disappearing, so a reviewer can always see what did not run and why.

## Scheduled (`nightly.yml`)

Runs once a day, and every job in it also carries `workflow_dispatch` so it can be triggered by
hand (`gh workflow run nightly.yml`) — a scheduled lane nobody can rerun on demand is a lane
nobody can debug. Hosts the full feature-on suite (`just test-full`), which no per-push lane
executes.

## Release (`release-plan.yml`, `release.yml`, `release-binaries.yml`, `verify-release.yml`)

Triggered by the release process, not by pushes; see `docs/releasing.md`.

## Where a new check belongs

- **Per-push** if it guards the correctness of every change.
- **Per-push behind a change-set condition** if it guards one surface; fail open — when the
  change set cannot be computed, run the job.
- **Scheduled** if it is broad, slow, or report-only; give it `workflow_dispatch` and a
  `timeout-minutes` bound like every other job.

Text guards in `tests/github_actions.rs` pin this topology (job tiers, timeouts, the nextest
version, trigger shapes). Changing where or when a job runs means updating those guards in the
same change — that is deliberate.
