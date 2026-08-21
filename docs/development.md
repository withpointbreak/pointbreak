# Development and validation guide

Pointbreak uses `just` as the maintainer entrypoint for development, verification, packaging, and
product-evidence commands. Run `just` or `just --list` to see recipes, and use
`just --list --group <name>` to focus on one surface.

The `Justfile` owns executable composition. This guide owns the decision about which gate applies to
a change and how to interpret its result. Script-level ownership and side effects live in
`scripts/README.md`.

## Start with the changed surface

| Change surface | Minimum local gate | Add when applicable | Expected outcome |
| --- | --- | --- | --- |
| Rust library, CLI, or headless behavior | `just check` | Focused `just test-file <name>` or `just test -E '<filter>'` while iterating; `just test-full` when the change touches `src/bench_support/**`, `src/session/benchmark.rs`, `benches/store_foundation.rs`, or any `#[cfg(feature = "longitudinal-counting")]` counting site | Commit range, build, format, Clippy, and nextest suite pass |
| Platform-conditional Rust code | `just check` | `just check-types` on relevant non-Linux hosts/CI | All workspace targets and feature-gated arms type-check |
| Longitudinal evidence façade or contract | Focused longitudinal nextest filters, `just test-full`, `just longitudinal-contract`, and `just longitudinal-smoke` | Verify each completed native package with `just longitudinal-verify-package <root>`; native collection remains an explicit operator ledger | Contract hashes remain stable; disposable non-timing construction/pair/preflight/package mechanics pass; package verification is read-only and recursively hash-complete |
| Derived-access product core or bundled SQLite closure | Focused service and SQLite cursor/locator/semantic tests, default and `--no-default-features` builds, `just check`, `just test-full`, and `just derived-access-tests` | `just store-foundation-qualification-smoke` when changing default routing, derived-store filesystem scope, or qualification integration; run `just derived-change-read <request>` on both native package hosts when changing Profile/Changes/Attention or `/api/v2/history` routing or its evidence contract, binding separate clean harness/product identities, a distinct explicit `POINTBREAK_QUALIFICATION_HOST_IDENTITY` per host lane, the exact control-test binary, and Change plus Timeline storage probes; `just package-archive-selftest`; native Windows focused open/close test when changing SQLite ownership or linkage | One product SQLite implementation serves qualification; strict and active Change and Timeline documents remain wire-equivalent under exact public fixture authority; V2 exact-source evidence embeds the frozen V1/v3 Change matrix and adds evaluator-v4 Timeline typed-error, request-bounded service-child counter, lifecycle/concurrent-trust, invalid-signature recovery, and bodyless-storage witnesses; bundled SQLite remains in the normal package closure; explicit host-lane authority is stable across network changes; explicit `off` records zero physical actions and creates no derived path |
| Developer-only LMDB proof surface | Focused closure/core/lifecycle tests plus `cargo bench --locked --features bench,lmdb-proof --bench store_foundation -- --lmdb-proof-open-close`, `--lmdb-smoke`, and `--lmdb-lifecycle-smoke` | Compile every `.github/binary-targets.json` target; run plain open/close and native dependency inspection on representative macOS, Linux glibc, Linux musl, and Windows hosts; run the lifecycle smoke natively on Windows for open-handle replacement, interrupted-copy cleanup, and reopen evidence | Exact reviewed sources compile and link statically; semantic and lifecycle smoke are non-timing, public-input-only, and disposable; online copy/restore/repair receipts and native allocation inventory are exact; no encryption, production routing, performance evaluation, or default-package/release inclusion |
| Inspector `web/src` | `just check`, `just web-check`, `just web-verify` | `just web-test` while iterating; `just web-build` when intentionally refreshing the bundle | Rust gate passes, front-end lint/types/tests pass, and committed `assets/app.js` matches source |
| VS Code extension | `just check`, `just extension-check` | `just extension-package` when packaging, binary selection, or extension delivery changes | Rust and extension checks pass; optional host VSIX contains the intended binary |
| GitHub Actions, binary targets, packaging, or release identity | `just workflow-lint`, `just package-archive-selftest` | `just release-bump-selftest` for Cocogitto/tag changes; `just installer-selftest` for acquisition changes | Workflow syntax and shell contracts pass without publication |
| Unix or Windows installer | `just installer-selftest` on the current host | Opposite-platform CI/live evidence required by `docs/releasing.md` | Hermetic acquisition, identity, upgrade, and rollback cases pass |
| Canonical Review example | `just review-example-verify` | Materialize into an empty repository when changing export/import behavior | Manifest, documents, projection identity, and source test agree |
| Review decision continuity | `just review-decision-browser-verify <empty-root>` | Inject the exact binary with `POINTBREAK_BINARY` for release evidence | Disposable canonical/synthetic stores pass browser behavior and viewport checks |
| Change-first Inspector product cut | `just change-inspector-browser-selftest`, then `POINTBREAK_BINARY=<absolute-exact-binary> just change-inspector-browser-verify <empty-root>` | Run the fast harness policy test while changing browser diagnostics; run the real gate after the exact source checkpoint and preserve its completion-last manifest, aggregate report, logs, recoverable missing-resource copy, and screenshots for human review | A disposable public L2 matrix proves 363+ Changes with at most 100 live cards, exact final topology and wide/narrow shared-membership fixtures, distinct removed and recoverably missing exact resources with no substituted diff, resource filters, wide/narrow, light/dark, compact/comfortable, keyboard and modal focus behavior, reading return path, URL-preserving clear reset, unchanged-generation DOM retention, and reduced motion without opening an owner store; recoverable product failures are reported together without publishing a passing manifest |
| Product screenshots or marketing capture | Appropriate capture recipe plus asset tests | Marketing synchronization/check workflow and real visual review | Captures, manifest, canonical example, and visible product state agree |
| Timeline compatibility fixtures (`tests/support/assets/inspector-timeline-compat-v1/`) | `just test-full` | Commit any regenerated content-addressed events | The generator and its exclusion-matrix verifier run only under the `bench` feature; checked-in bytes must match the current generator |
| Agent Skills | `just skills-validate` | `just skills-link` for a local installation check | Each skill validates against the pinned validator and links remain controlled |

These are minimum gates, not substitutes for a task-specific acceptance matrix. A change that crosses
surfaces inherits every affected row.

For exact-source Change-read evidence, build the library and CLI control executables from the same clean
commit as the qualification harness with these frozen commands:

```bash
cargo +stable test --locked --features longitudinal-counting --lib --no-run
cargo +stable test --locked --features longitudinal-counting --bin pointbreak --no-run
```

The request binds each executable's bytes and canonical build command. Before accepting any lifecycle or
call-graph row, the runner executes a source-attestation test inside each binary and requires libtest output
proving that exactly one frozen, fully qualified test ran and passed. An exit code of zero with no matching
test is not evidence.

Timeline qualification is carried only by `pointbreak.qualification-derived-change-read-receipt.v2`, which
embeds the complete V1 Change receipt and selects evaluator v4. Its six Timeline suites use the Inspector
service child for both semantics and request-local counters; direct library counters remain V1 Change
characterization and are not substituted for `/api/v2/history` process evidence. A typed-failure fixture may
mark only its unavailable continuation-token storage sentinel absent. Do not invent a replacement token or
compare request-signed token hashes between hosts.

Bodyless-storage evidence derives its summary, prose, and raw-payload probe hashes from the public fixture
witness, binds the fixture-private path to the disposable repository, and derives the selected-store path
probe internally. The SQLite table, column, and index catalog must also remain free of body/search names.
Checkpoint/catalog reads occur in one read transaction and are retained only when before/during/after
carrier inventories and the selected publication remain unchanged. Pre-cut receipts are diagnostic-only and
must name the complete non-empty set of failed matrix rows; they can never enter a qualification package.

Any new external longitudinal driver that captures process CPU on macOS must use
`capture_longitudinal_process_snapshot_v1`. The helper applies the live Mach timebase before returning
nanoseconds and retains that timebase in the snapshot. It fails closed on other platforms rather than
reporting unavailable CPU as zero. Historical receipts that did not normalize at capture remain immutable
and require a separately bound additive correction; do not rewrite them in place.

## What `just check` covers

`just check` runs, in order:

1. `commit-check` for the configured commit range;
2. a debug Rust build;
3. Rust formatting and Clippy across the workspace, all targets, and all features; and
4. the default Rust nextest suite (the qualification-evidence harness compiles and is
   clippy-linted in step 3 under `--all-features`, but its tests run only under `just test-full`).

It deliberately does **not** install or run Node, build the Inspector bundle, check the VS Code
extension, lint workflows, exercise installers, or run real-browser evidence. Those surfaces have
separate prerequisites and failure meanings, so the repository does not provide a misleading
`check-all` recipe.

### What `just test` does not run

The qualification-evidence harness is gated behind the `bench` feature at five module
declarations across three files: `derived_access` and `foundation` in `src/bench_support.rs`,
`builder` and `evidence` in `src/bench_support/longitudinal/mod.rs`, and `benchmark` in
`src/session/mod.rs`. A featureless run therefore compiles and executes neither those roughly
79k lines nor the roughly 348 tests inside them, which is what keeps the default wall clock
usable. The deterministic longitudinal contracts, the counting sites, and the control-registry
tests all stay in the default lane.

Run `just test-full` to run the default suite plus the harness. Do that before landing a change
to the harness itself, to `src/session/benchmark.rs`, to `benches/store_foundation.rs`, or to any
counting site — and whenever a generated fixture the harness owns may need refreshing. It selects
`longitudinal-counting` (which implies `bench`), deliberately not `--all-features`: the
`gix-parity` and `lmdb-proof` lanes stay separate and keep their own recipes.

During an uncommitted first edit, `commit-check` may have no task commit to inspect. Run focused tests
while iterating, then run `just check` after creating the reviewable commit range or pass the intended
range to `just commit-check` explicitly.

The differential git-backend parity harness is a separate gate. Git access runs through a typed
backend seam (ADR-0040): the in-process `gix` backend ships in the default build (the `gix`
feature, on by default) and the qualified read/scalar classes route to it, while the capture diff
and write-tree stay on subprocess `git` permanently. `POINTBREAK_GIT_BACKEND=subprocess` is the
runtime escape hatch, and `--no-default-features` builds the subprocess-only backend. `just check`
covers the `gix` code (Clippy runs `--all-features`) but does not run the parity harness, which is
gated on `--features gix-parity` and exercised by `just git-parity` — and by a dedicated CI lane on
macOS and Windows. Run `just
git-parity` when you change the git seam or either backend; `just git-bench` prints the per-operation
subprocess-vs-gix win. See `docs/adr/adr-0040-git-backend-seam-and-hybrid.md`.

## Generated and protected artifacts

Some commands are intentionally mutating:

| Command | Writes | Rule |
| --- | --- | --- |
| `just fix` | Rust source formatting and Clippy fixes | Inspect every edit; it allows dirty/staged input |
| `just reader-profile-generate` | `src/documents/change_reader_profile_v1.json` | Run after intentionally changing the Rust Change reader registry; both bundled clients consume this checked-in derivative |
| `just web-build` | Committed Inspector `assets/app.js` | Run only after editing web source; finish with `just web-verify` |
| `just extension-package` | Local VSIX/package output | Treat as disposable dogfood output unless a task explicitly preserves it |
| `just longitudinal-smoke` | Disposable public roots under the host temporary directory | Non-timing only; output is never terminal or native qualification evidence |
| `just derived-change-read-diagnostic <request>` | One empty external workspace containing isolated public fixture clones | Emits only schema-less diagnostic case statuses for the wrapper; it is never a receipt, fragment, package, or terminal evidence input. |
| `just review-example-export …` | Canonical example output | Export from an explicit source repository, inspect the pack, then verify it |
| `just change-inspector-browser-verify <empty-root>` | A disposable public-L2 fixture repository, home, retained recovery copy for the intentionally missing artifact, logs, screenshots, and completion-last manifest beneath the caller-supplied root | The root must start empty and stay outside the worktree; require a clean worktree and exact injected binary. The manifest is evidence only when it exists last, names the same clean source commit and binary, matches the browser report, and binds every retained browser output by SHA-256. |
| Screenshot capture recipes | PNG files and optional provenance manifest | Capture committed UI from the intended record; visually inspect both themes |
| `just migrate-store-common-dir …` | Pointbreak store placement | Non-destructive and idempotent, but still durable state; inspect the target repo first |

Freshness commands such as `just web-verify`, `just review-example-verify`, and marketing lock checks
are not regeneration instructions. If one fails, establish whether the derivative is stale or the
claimed source identity is wrong before writing anything.

## Release and publication boundary

The release self-tests are nonpublishing mechanics checks. They do not authorize or perform a public
release. Current commands, credentials, exact-parent requirements, and the explicit owner gate live
in `docs/releasing.md`.

In particular:

- `just package-archive-selftest` validates Cargo/package/archive mechanics locally;
- `just release-bump-selftest` validates Cocogitto and signed-tag mechanics in temporary repos;
- `just installer-selftest` validates the current host installer against local fixtures; and
- `just workflow-lint` validates workflow and shell contracts.

Public truth is established only by the owner-authorized release workflow followed by published-
release verification and any required installed-product or browser evidence.

## Interpreting failures

| Class | Typical signal | First response |
| --- | --- | --- |
| Prerequisite/environment | Command or tool missing, dependency not installed, browser unavailable, credentials rejected | Repair the environment and rerun; do not weaken the check |
| Stale generated artifact | `git diff --exit-code` reports a generated file changed | Regenerate through the owning command and inspect why the bytes changed |
| Identity/provenance mismatch | Commit, tag, digest, manifest, archive, installer, or binary fields disagree | Stop and reconcile the exact source; never edit the identity field in isolation |
| Contract drift | Schema/layout/target/transaction assertion fails | Decide whether implementation or reviewed contract is wrong, then change the owner |
| Behavior regression | Valid fixture reaches the product but semantic/browser assertion fails | Preserve evidence and debug the affected behavior |

Do not turn a fail-closed identity or protected-proof check into a write path merely to make it green.

## Prerequisites and setup

- Provision the toolchain with `nix develop`, `mise install`, or a manual `rustup` setup; see
  `CONTRIBUTING.md` for the three environment options.
- Run `just setup-hooks` once per clone for Cocogitto commit and branch checks (the Nix shell and
  mise do this for you).
- Run `just web-install` before Inspector Node commands and `just extension-install` before VS Code
  extension commands.
- `just workflow-lint` requires `actionlint`, `shellcheck`, and `jq`.
- Browser evidence requires the repository's supported browser tooling and an empty disposable root.
- Commands that accept `POINTBREAK_BINARY` require an absolute executable path. Release evidence
  should inject the exact installed or exact-tag binary instead of rebuilding implicitly.

See `scripts/README.md` for script inventory, mutation boundaries, and script-specific failure
classes.
