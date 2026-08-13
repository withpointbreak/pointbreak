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
| `capture-inspector-screenshots.sh` | `just capture-inspector-screenshots` or `just capture-marketing-review-screenshots` | Replaces selected PNGs and, when requested, writes the capture manifest last | Both themes match the running Inspector and optional canonical-example identity | Inspector unavailable, wrong revision/track, browser/setup failure, visual contract drift, or provenance mismatch |
| `materialize-inspector-decision-matrix.sh` | `just review-decision-matrix-materialize <empty-dir>` | Creates a disposable repository, public frozen L2 activation records, home, keys, and Pointbreak records beneath the destination | Canonical and synthetic decision-continuity fixtures are complete, isolated without relying on an owner store, and publish hash-only bodyless-probe authority for the fixture summary, prose, and activation payload | Non-empty destination, missing/inexact binary, unsafe home placement, unavailable public L2 fixture, or record-construction drift |
| `verify-inspector-decision-continuity.sh` | `just review-decision-browser-verify <empty-root>` | Materializes disposable stores and writes browser evidence beneath the supplied root | Canonical and synthetic Review behavior passes across the supported viewport matrix | Fixture construction, Inspector startup, browser environment, console, layout, navigation, freshness, or product behavior failure |
| `verify-inspector-decision-continuity.mjs` | Internal template consumed by the shell verifier | Browser page state only | Injected browser assertions complete without errors | Review rendering or interaction contract failed; do not invoke this template directly |
| `change-inspector-browser-verify.sh` | `POINTBREAK_BINARY=<absolute-exact-binary> just change-inspector-browser-verify <empty-root>` | Creates only a public L2 Change matrix, disposable home, retained recovery copy for the intentionally missing artifact, one post-park public Timeline append, committed harness and activation-fixture snapshots, an injected-binary snapshot, browser screenshots, logs, and completion-last manifest below the supplied empty root | The binary snapshot attests the clean source commit; the gate executes only source-bound harness, activation-fixture, and binary bytes and records their digests. Browser error observers precede the capability-bearing navigation. Default Change-aware Timeline over 300+ events and 363+ bounded Changes, signed chronology paging and typed filters, compact semantic Timeline rows with shortened native identity links, reason-bearing Attention, exact event/detail navigation, Change and fact relationship graphs with textual equivalents, a full-frame annotated diff with inline facts and `[`/`]` plus `p`/`n` navigation, keyboard/focus/modal behavior, follow/park/catch-up, exact Revision/resource states, reload/Back/Forward restoration, wide/narrow light/dark density states, unchanged-generation DOM retention, and reduced motion pass in a real browser. The manifest is written only after its browser report passes and a sorted SHA-256 inventory covers every retained browser output. | Non-empty/unsafe root, binary/source mismatch, fixture construction or missing-artifact containment, Timeline append or Inspector startup, browser environment, interaction/focus/route/layout assertion, console, request failure, or product behavior failure |
| `change-inspector-browser-verify.mjs` | Internal template consumed by the Change-first browser verifier | Browser page state and configured screenshot directory only | Independently recoverable product sections collect contextual failures, stop only invalid sections, and return one structured report after exercising Timeline and Change routes, semantic presentation, relationship graphs and text alternatives, full-frame exact diff, viewport, preference, keyboard, modal focus, reading, chronology, and motion | Change-aware Inspector rendering, accessibility, navigation, chronology, or responsive behavior failed; do not invoke this template directly |
| `change-inspector-browser-diagnostics.mjs` | Inlined into the browser template and exercised by `just change-inspector-browser-selftest` | In-memory diagnostics only | Soft checks aggregate with section, route, viewport, screenshot, and log context; invalid transitions stop only their section; any recorded failure refuses passing completion | Section recovery or terminal aggregate policy drifted |
| `change-inspector-browser-manifest.mjs` | Final completion step inside the shell verifier and harness self-test | Verifies stable retained browser-output handles, then atomically publishes captured candidate bytes without replacing an existing marker | Exact passing browser report has zero failures and matching assertion/screenshot counts; a sorted SHA-256 inventory covers every retained PNG and browser log/program before `manifest.json` appears | Invalid/failed browser report, mismatched counts or evidence bytes, incomplete inventory, changed or symbolic paths, existing completion marker, or cross-directory publication attempt |
| `change-inspector-browser-diagnostics.selftest.mjs` | `just change-inspector-browser-selftest` | Temporary directories under the host temporary root only; no browser, fixture, or owner store | Multiple section failures aggregate, invalid setup skips only its body, and failed diagnostics cannot publish a passing manifest | Browser diagnostic or completion-publication policy drifted |
| `derived-change-diagnostic-campaign.mjs` | `just derived-change-diagnostic <create-authority\|run-host\|unavailable-host\|merge> <absolute-config>` | Creates caller-selected, initially empty external evidence and scratch roots; host runs isolate each mutable case below scratch while the merger writes `derived-change-diagnostic-report.json` once | One exact campaign binds clean signed source, product/harness/control binaries, public fixture authority, APFS/NTFS hosts, the complete closed platform program inventory by absolute path and SHA-256, required JavaScript/browser dependency trees, explicit dependencies, all recoverable case results, and a final `admissible: false` report. Preflight proves the bound Cargo tools and the direct Node/Vitest/Playwright/browser launch chain are loadable before expensive lanes; postflight rehashes every program and bound tree. | Global identity or root invalidity, substituted or unloadable tool, dependency-tree drift, lane prerequisite failure, case failure, required host unavailability, incomplete inventory, changed retained bytes, or a Red aggregate verdict |
| `derived-change-diagnostic.mjs`, `derived-change-diagnostic-fixture.mjs`, and `derived-change-diagnostic-report.mjs` | Internal modules exercised by `just derived-change-diagnostic-selftest` | External per-case scratch plus separate retained diagnostic roots only; no owner store and no terminal receipt/package/manifest output. Each command receives fresh scratch-scoped `CARGO_HOME`, `RUSTUP_HOME`, and `CARGO_TARGET_DIR`; compile and policy cases receive only the configured actual `CARGO` and `RUSTC` programs. | Commands continue independent roots, fail closed on unknown diagnostic children, normalize the stable public topology checkpoint while retaining raw observations, rehash bound program files and safe in-root dependency trees, inventory logs/artifacts by SHA-256, finalize campaign-bound host fragments, and merge exactly one deterministic non-admissible report | Unsafe or non-empty roots, wrong source/binary/fixture/tool identity, invalid dependency graph, corrupt collection, artifact drift, topology checkpoint drift, or report recomputation mismatch |
| `derived-change-diagnostic-native.mjs` | Internal native collector called by the campaign | Fresh D0-128/L1/L7 and lifecycle roots below the case scratch root; requests and logs remain below the retained case root | Every independent native tier and all lifecycle vectors return one internal case collection; raw terminal receipts are never serialized or retained | Invalid exact-source request, native setup failure, unstable admitted root, lifecycle setup failure, or individual vector failure |
| `derived-change-diagnostic-change-read.mjs` | Internal public-fixture collector called by the campaign | Nine independent disposable Change fixture templates and workspaces below the case scratch root; requests, witnesses, and logs remain below the retained case root | Exact named read, control, concurrency, append/restart, fault, and storage cases return one internal collection while identity postflight remains independently observable | Fixture authority or tool drift, materialization failure, invalid collector output, failed read/control/storage case, or postflight identity change |
| `derived-change-diagnostic-browser.sh` and `.mjs` | Internal focused real-browser collector called by the campaign | One public L2 fixture below the case scratch root, plus an Inspector process, browser session, and retained logs/screenshots below the case evidence root | Bound Bash and utilities materialize the fixture; bound Node loads the exact Playwright package tree and launches the explicitly configured bound browser bundle without npm/npx or PATH fallback. Repeated narrow exact-detail to wide split-pane transitions retain the same event id and route, settle with exact-action focus and all five ordinary surfaces non-inert, and retain every iteration and runtime error channel. | Dirty/wrong source or binary, missing or drifted browser/tool prerequisite, fixture/startup failure, exact-event/route transition failure, runtime page/console error, or missing retained artifact |

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
