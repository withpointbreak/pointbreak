# Pointbreak scripts

This directory contains Pointbreak's public installers and the repository automation that supports
development, release, and product-evidence workflows. It is an operational boundary, not a general
utility bucket.

Prefer a documented `just` recipe when one exists. Recipes provide the stable maintainer entrypoint
and compose prerequisites consistently. Invoke a script directly only when this guide, the script's
help, or an owning workflow says to do so.

## Operating rules

- `install.sh` and `install.ps1` are public acquisition contracts at stable paths. Do not move them.
- Release mutation is owner-gated. Follow `docs/releasing.md`; do not infer a release procedure from
  script names.
- Use a worktree-local or explicitly injected `POINTBREAK_BINARY` and disposable
  `POINTBREAK_HOME` for generated evidence. Do not let a test or capture inherit an owner store.
- Treat tags, checksums, manifests, protected examples, screenshots, and provenance digests as
  identity-bearing artifacts. Do not edit them merely to make a check pass.
- A self-test proves local mechanics. It does not prove that a public release or remote endpoint
  exists.

## Acquisition and installer contracts

| Script | Preferred entrypoint | Mutates | Expected result | Failure usually means |
| --- | --- | --- | --- | --- |
| `install.sh` | Public install command in `README.md` or `docs/installation.md` | Installs or replaces `pointbreak` in the requested prefix | The installed binary reports the requested clean release identity | Unsupported platform, missing/checksum-invalid asset, identity mismatch, or failed atomic replacement |
| `install.ps1` | Public PowerShell install command in `README.md` or `docs/installation.md` | Installs or replaces `pointbreak.exe`; may update user `PATH` | The installed binary reports the requested clean release identity | Unsupported platform, missing/checksum-invalid asset, identity mismatch, replacement failure, or `PATH` update failure |
| `install-selftest.sh` | `just installer-selftest` on macOS/Linux | Temporary fixture directories only | Hermetic fresh-install, upgrade, rollback, collision, and identity cases pass | Unix installer contract drift or a missing host prerequisite |
| `install-selftest.ps1` | `just installer-selftest` on Windows | Temporary fixture directories and temporary environment values only | The PowerShell installer contract matrix passes and cleanup restores the environment | Windows installer contract drift or missing PowerShell/archive support |

The installers are release-agnostic. Do not change them for an ordinary version bump when the
asset, checksum, identity, platform, installation, and rollback contracts are unchanged.

## Release construction and identity

| Script | Preferred entrypoint | Mutates | Expected result | Failure usually means |
| --- | --- | --- | --- | --- |
| `package-release-archive.sh` | Release workflow; exercise through `just package-archive-selftest` | Writes one archive in the working directory | Archive name, executable, license, and notice match `.github/binary-targets.json` | Wrong target row, missing build output, unsafe archive input, or layout drift |
| `package-release-selftest.sh` | `just package-archive-selftest` | Temporary package/archive fixtures only | Cargo package and every release archive layout validate without publishing | Package contents, metadata, target table, archive layout, or verification contract drifted |
| `verify-release-archives.sh` | Release/verification workflows | Read-only unless `--write-checksums` is supplied | Exact archive set validates; optional checksum file is complete and deterministic | Missing/extra archive, unsafe entry, wrong executable/layout, or checksum disagreement |
| `assert-release-identity.sh` | Release/verification workflows | Read-only | A runnable binary reports the exact version, tag, full commit, and clean Git build | The wrong binary or build entered the release path |
| `assert-release-identity-selftest.sh` | `just workflow-lint` | Temporary fixture binaries only | All accepted and rejected build-identity cases classify correctly | Release identity assertions became too weak, too strict, or incompatible with the version document |
| `finalize-cocogitto-release-tag.sh` | Cocogitto release hook only | Guardedly replaces one verified local lightweight tag with a signed annotated tag | The signed release commit is the approved child and the annotated tag peels to it | Parent/tree/commit signature mismatch, remote collision, unexpected local tag type, or signing failure |
| `finalize-cocogitto-release-tag-selftest.sh` | `just release-bump-selftest` | Temporary Git repositories and temporary GPG home only | Native Cocogitto tag lifecycle and collision guards pass | Cocogitto behavior, signing assumptions, or the finalizer contract changed |
| `run-release-plan.sh` | Commands in `docs/releasing.md` | Dispatches a GitHub workflow; `release` mode may publish after the owner gate | The exact-parent plan or release run succeeds and returns its report | Source parent moved, target already exists, workflow failed, authentication is missing, or release authorization is stale |
| `run-release-verification.sh` | Command in `docs/releasing.md` | Dispatches the published-release verification workflow; optionally retains reports | Live platform acquisition rows and immutable release identity verify | Missing/incorrect public artifact, installer failure, identity mismatch, unsupported live runner, or GitHub authentication failure |

`run-release-plan.sh release` is not a routine validation command. The required nonpublishing plan,
exact version and source commit, and explicit owner authorization are defined in
`docs/releasing.md`.

## Review examples and browser evidence

| Script | Preferred entrypoint | Mutates | Expected result | Failure usually means |
| --- | --- | --- | --- | --- |
| `capture-inspector-screenshots.sh` | `just capture-inspector-screenshots` or `just capture-marketing-review-screenshots`; set `POINTBREAK_REVIEW_EXAMPLE_PACK=<path-to-prebuilt-verifier>` to reuse an already-built pack verifier instead of compiling one, and `PLAYWRIGHT_CLI=<path>` to select a browser driver | Replaces selected PNGs and, when requested, writes the capture manifest last | Both themes match the running Inspector and optional canonical-example identity | Inspector unavailable, wrong revision/track, browser/setup failure, visual contract drift, or provenance mismatch |
| `materialize-inspector-decision-matrix.sh` | `just review-decision-matrix-materialize <empty-dir>` | Creates a disposable repository, public frozen L2 and historical-reader compatibility records, home, keys, and Pointbreak records beneath the destination | Canonical and synthetic decision-continuity fixtures are complete, every Timeline source/admission family is publicly represented, the trust pair is witnessed, and the fixture stays isolated from owner stores | Non-empty destination, missing/inexact binary, unsafe home placement, unavailable public fixture records, or record-construction drift |
| `verify-inspector-decision-continuity.sh` | `just review-decision-browser-verify <empty-root>` | Materializes disposable stores and writes browser evidence beneath the supplied root | Canonical and synthetic Review behavior passes across the supported viewport matrix | Fixture construction, Inspector startup, browser environment, console, layout, navigation, freshness, or product behavior failure |
| `verify-inspector-decision-continuity.mjs` | Internal template consumed by the shell verifier | Browser page state only | Injected browser assertions complete without errors | Review rendering or interaction contract failed; do not invoke this template directly |
| `change-inspector-browser-verify.sh` | `POINTBREAK_BINARY=<absolute-exact-binary> just change-inspector-browser-verify <empty-root>` for the one-shot full gate; invoke the script directly with one of the literal `--shakedown`, `--shakedown-timeline-boundary`, `--shakedown-exact-history-focus`, or `--shakedown-return-destinations` modes for focused readiness | Full mode creates only a public L2 Change matrix, disposable home, retained recovery copy for the intentionally missing artifact, one post-park public Timeline append, committed harness and activation-fixture snapshots, an injected-binary snapshot, browser screenshots, logs, and completion-last manifest below the supplied empty root. Every shakedown owns and cleans a separate temporary root and rejects a caller root. | The binary snapshot attests the clean source commit; the gate executes only source-bound harness, activation-fixture, and binary bytes and records their digests. Browser error observers precede the capability-bearing navigation. Default Change-aware Timeline over 300+ events and 363+ bounded Changes, signed chronology paging and typed filters, compact semantic Timeline rows with shortened native identity links, reason-bearing Attention, exact event/detail navigation, Change and fact relationship graphs with textual equivalents, a full-frame annotated diff with inline facts and `[`/`]` plus `p`/`n` navigation, keyboard/focus/modal behavior, follow/park/catch-up, exact Revision/resource states, reload/Back/Forward restoration, wide/narrow light/dark density states, unchanged-generation DOM retention, and reduced motion pass in a real browser. The manifest is written only after its browser report passes and a sorted SHA-256 inventory covers every retained browser output. | Non-empty/unsafe root, binary/source mismatch, fixture construction or missing-artifact containment, Timeline append or Inspector startup, browser environment, interaction/focus/route/layout assertion, console, request failure, or product behavior failure |
| `change-inspector-browser-verify.mjs` | Internal template consumed by the Change-first browser verifier | Browser page state and configured screenshot directory only | Independently recoverable product sections collect contextual failures, stop only invalid sections, and return one structured report after exercising Timeline and Change routes, semantic presentation, relationship graphs and text alternatives, full-frame exact diff, viewport, preference, keyboard, modal focus, reading, chronology, and motion | Change-aware Inspector rendering, accessibility, navigation, chronology, or responsive behavior failed; do not invoke this template directly |
| `change-inspector-browser-diagnostics.mjs` | Inlined into the browser template and exercised by `just change-inspector-browser-selftest` | In-memory diagnostics only | Soft checks aggregate with section, route, viewport, screenshot, and log context; invalid transitions stop only their section; any recorded failure refuses passing completion | Section recovery or terminal aggregate policy drifted |
| `change-inspector-browser-manifest.mjs` | Final completion step inside the shell verifier and harness self-test | Verifies stable retained browser-output handles, then atomically publishes captured candidate bytes without replacing an existing marker | Exact passing browser report has zero failures and matching assertion/screenshot counts; a sorted SHA-256 inventory covers every retained PNG and browser log/program before `manifest.json` appears | Invalid/failed browser report, mismatched counts or evidence bytes, incomplete inventory, changed or symbolic paths, existing completion marker, or cross-directory publication attempt |
| `change-inspector-browser-diagnostics.selftest.mjs` | `just change-inspector-browser-selftest` | Temporary directories under the host temporary root only; no browser, fixture, or owner store | Multiple section failures aggregate, invalid setup skips only its body, and failed diagnostics cannot publish a passing manifest | Browser diagnostic or completion-publication policy drifted |

The four literal shakedown modes are bounded readiness tiers for the full Change Inspector browser gate. Each accepts no caller evidence root: the harness creates one temporary root and uses the same public fixture materializer, primary Inspector server, injected browser program, and launch path as the full gate. The first three modes exit after one hard-coded journey. `--shakedown` observes two completion-anchored quiet polls, permits only serialized `/api/v2/profile` probes, and verifies unchanged-generation disclosure and scroll retention. `--shakedown-timeline-boundary` holds one Timeline continuation while a due tick passes, rejects background API fan-out during that foreground traversal, then verifies the released global `G` traversal reaches the terminal selected event. `--shakedown-exact-history-focus` verifies Changes `G`, accepted exact Revision and resource hydration, browser Back restoration, and narrow detail Back route/closure/focus as one history journey. `--shakedown-return-destinations` instead runs four independent diagnostics sections so an earlier non-fatal failure cannot hide a later journey. It verifies narrow Timeline detail Back reaches the fully restored retained master; performs an authenticated typed `limit=1` Changes preflight and binds its exact projection stamp and terminal capability to the rendered list before any detail/keyboard input; verifies a Changes exact-detail return completes before View input and the shared terminal-`G`/first-page-`g` traversal; preserves the parallel-current filtered `limit=100` Revision/resource/Back journey through accepted route-bound bodies with retained detail focus; and uses at most three consecutive natural poll opportunities to prove one exact pre-navigation primary-profile request was superseded by an explicitly armed Changes-to-Timeline transition whose destination completed successfully. Request tracking is by Playwright Request-object identity; only `GET`/`fetch`, the primary `/api/v2/profile`, exact `net::ERR_ABORTED`, one snapshotted request, and at most one admitted failure per invocation qualify. Every unarmed, new, different, multiple, or unsuccessful-destination failure remains fatal. The exact ordered sections write distinct temporary screenshots named `shakedown-retained-timeline-return.png`, `shakedown-changes-terminal-return.png`, `shakedown-parallel-current-exact-history.png`, and `shakedown-poll-supersession-request-accounting.png`; the report and independent filesystem checks must both count all four. A passing shakedown closes the browser, stops its servers, removes its root, prints a compact cleanup receipt, and retains no manifest or browser evidence. Setup, browser, product, or cleanup failures are non-full-attempt failures and may leave diagnostics only when cleanup itself cannot complete; do not supply or touch any reserved full-gate evidence root. Use at most two failed focused shakedown cycles for one failure class. After the plan-required shakedown passes, run the full browser verifier once against its fresh reserved empty caller root.

The browser program now records request ownership by committed main-document generation,
an exact accepted Changes route visit, capability-redacted source hash, and safely resolved
main-frame initiator. Only one still-outstanding profile request with all of that provenance
can bind either explicit supersession arm. Its post-destination settlement join is bounded
at 30 seconds; timeout is a single non-admissible section failure, not a retry or a new
request-failure exemption.

The shell bounds only the `run-code` browser-program stage at 600 seconds. While that child
is active it writes `logs/browser-stage-start.json`, appends a redacted heartbeat no less
often than every 15 seconds to `logs/browser-stage-heartbeat.log`, and publishes exactly
one `logs/browser-stage-terminal.json`. The heartbeat contains only mode, elapsed time,
child liveness, screenshot count/latest basename, and gate-log byte count. The child leads
a verified process group. Heartbeats run against fixed 15-second deadlines; a missed
deadline or worker/render failure becomes a gate-failing internal terminal outcome. After
the first observed exit, timeout, `INT`, or `TERM`, the
shell stops its workers, spends at most 10 seconds terminating and verifying absence of
that process group, and records `runCodeGroupCleanup` as `complete` or `failed`; inability
to enumerate the group is failure, never proof of absence. The
terminal receipt deliberately records browser session cleanup as `pending` and is printed
before the separate browser-close attempt, so a slow close cannot hide the stage outcome.
Focused roots may then be removed; full-mode inventories hash all three receipts before
the completion-last manifest. A timeout or cleanup failure never fabricates
`browser-result.json` or `manifest.json`.

Screenshot and canonical-example changes have cross-repository consequences. Follow
`docs/manual-testing.md` and the marketing repository's documented synchronization workflow before
advancing protected captures or marketing locks.

## Maintainer utilities

| Script | Preferred entrypoint | Mutates | Expected result | Failure usually means |
| --- | --- | --- | --- | --- |
| `link-agent-skills.sh` | `just skills-link` or `just skills-unlink` | Creates or removes controlled skill symlinks | Requested agent installations point to the repository skills without replacing unrelated paths | Ambiguous target, non-symlink collision, unsupported agent, or unsafe user-level request |
| `worktree-to-fixture.sh` | Direct invocation after reading `--help` | Writes a standalone fixture outside the source repository | Fixture retains exact Git state and the resolved Pointbreak store without source-repository coupling | Missing binary/store, unsafe destination, unresolved Git base, copy failure, or fixture readback failure |

Fixtures may contain private review data. Keep them outside this repository and never commit them.

## Failure classes

Use the error output first, then classify the failure before changing anything:

1. **Prerequisite or environment** — a required executable, toolchain, browser, credential, network
   endpoint, or injected path is unavailable. Repair the environment and rerun; do not record this
   as product evidence.
2. **Stale generated artifact** — authored source and a committed derivative disagree. Regenerate
   through the owning command, inspect the diff, and rerun the freshness check.
3. **Identity or provenance mismatch** — a commit, tag, digest, manifest, archive, installer, or
   binary does not name the same work. Stop and reconcile the source; never hand-edit the identity.
4. **Contract drift** — implementation and an asserted schema, layout, platform, or transaction
   rule disagree. Fix the owning implementation or deliberately update the reviewed contract.
5. **Behavior regression** — a valid fixture and environment reached the product but an assertion
   failed. Preserve the evidence and investigate the product path.

## Adding or changing a script

- Give each human-invoked script a short header stating its purpose, preferred wrapper, side effects,
  and critical prerequisites or environment variables. Provide `--help` or a usage error.
- Add or update the appropriate `just` recipe when the script is a normal maintainer entrypoint.
- Classify the script in this README and state whether it mutates durable or protected artifacts.
- Update the owning workflow, tests, and public documentation together when the script implements a
  release, installer, or evidence contract.
- Keep public installer paths stable. Prefer documentation over directory churn until a capability
  has a proven independent boundary and all external callers can migrate safely.
