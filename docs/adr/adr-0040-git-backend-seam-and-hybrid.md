# ADR-0040: Git Backend Seam and the Permanent Subprocess/gix Hybrid

**Status:** Accepted (owner-approved 2026-07-19); landed via the git backend seam implementation work.
**Date:** 2026-07-19
**See also:** [ADR-0020](./adr-0020-durable-storage-backend-seam.md) (the ratified backend-seam
idiom this reuses: closed enum at one choke point, object-safe traits as insurance, env selector,
honesty-test second impl, zero-format-change staging), [ADR-0004](./adr-0004-event-signatures.md)
(signed event records the writer-identity input feeds), issue
[#238](https://github.com/withpointbreak/pointbreak/issues/238). Grounded in the git-library
integration design research (synthesized and cross-provider-reviewed 2026-07-19), which
delta-verified and superseded the June 2026 deferral. Amends the repo guidance "prefer shelling out
to `git` at first, and let a VCS abstraction come later if the review model earns it" (CLAUDE.md):
the abstraction is now earned at the seam; shelling out remains correct for capture-time diff and
honest fixture history.

## Context

Every production git access is already funneled through one typed seam: ~32 `git_*`/`capture_*`
functions in `src/git/command.rs` whose only spawn sites are `run_git_status`
(`command.rs:901`) and `run_git_with_stdin` (`command.rs:937`), plus the two-pass diff pipeline in
`src/git/ingest.rs` (`diff_files_for_args`, `ingest.rs:161-183`). Each call is a fresh subprocess:
~10–13 ms on macOS, **~38–45 ms on Windows** (measured). The test suite is
sys-bound on these spawns (329 s sys vs 223 s user across 2,764 tests), and interactive capture
pays ~130 ms (mac) / ~470 ms (win) of pure git-spawn overhead on the measured zero/one-untracked
11–12-spawn capture profile.

The design research established the three facts this decision rests on:

1. **The identity split.** Capture-time diff bytes are identity-bearing at the strongest level:
   `object_identity` hashes path + status + rename source + intrinsic flags + row {kind,text}
   (`fingerprint.rs:458-493`), feeding `revision_id` (`fingerprint.rs:360-375`), the signed
   `WorkObjectProposed` payload's `object_artifact_content_hash` (`capture.rs:451`,
   `object_artifact.rs:270-285`), at-rest hash-validated artifacts, and frozen byte fixtures
   (`fingerprint.rs:697-777`). A backend that classifies one rename/binary/mode-only case
   differently **silently forks revisions on recapture**. By contrast, every read surface
   (revision list/show, liveness, inspect, `pointbreak diff` replay) is presentation-only —
   measured zero diff spawns at read time. A small capture-time carve-out is also
   identity/event-bearing but spec-deterministic: the provenance scalars (`rev-parse
^{commit|tree}`, `write-tree`, `hash-object -t tree`, HEAD oid/ref, worktree root) and the
   writer-identity `config --get` (enters the signed event record — envelope beside payload).
2. **gix cannot own capture diff; nothing else blocks it.** On gix 0.85.0 (current), rename-_source_
   selection diverges from git by documented-permanent design (first-found candidate; reproduced:
   gix picked a 58%-similar source where git picked 77%) — and rename source is an `object_id`
   input. gix also has no `write-tree` API and emits no git raw/patch envelope. Everything else the
   June deferral worried about is `covered`/`covered-with-wrapper` and probe-verified:
   `info/exclude` honored, linked-worktree common-dir via the umbrella (canonicalization wrapper),
   MSRV/edition fit, security clean for a non-checkout user.
3. **git2 loses on supply chain, not capability.** git2 measured faster than gix on most ops and
   near-byte-exact on the one measured patch — but libgit2 shipped a same-day five-CVE cluster
   (2026-07-18) with git2-rs still vendoring unpatched 1.9.4; a static-musl release (which
   Pointbreak ships) would carry known-vulnerable C, and _any_ git2 adoption is a standing
   commitment to track libgit2 security releases on someone else's cadence. Both libraries are at
   least ~5× faster than subprocess on every measured op and often orders of magnitude faster;
   gix's slowest band is its graph walks (1–7 ms), still ≥5× faster than
   one spawn. The goal is eliminating spawns, not winning microseconds between libraries.

The house has already ratified this decision's shape once: ADR-0020's storage backend seam.

## Decision

### D1. A `GitBackend` seam behind the existing typed surface; subprocess stays the default

The ~32 `git_*` functions keep their signatures and call sites. Internally they dispatch through a
closed enum resolved at one choke point (an evolution of the `RepoFact` memo):

```rust
/// Object-safe (ADR-0020 D1 insurance). One method per operation-contract row;
/// every method mirrors the existing typed return (Ancestry / Option / bool / Vec…).
trait GitBackend: Send + Sync { /* one fn per contract row */ }

enum GitBackendKind {
    Subprocess(SubprocessBackend),      // today's command.rs bodies, moved verbatim
    #[cfg(feature = "gix")]
    Gix(GixBackend),
}
```

`gix` enters behind a new cargo feature **`gix`** (optional `dep:gix`, one pinned release train),
fully separate from the storage-qualification `bench` feature. The default build has a
single-variant enum and zero new dependencies; observable behavior is byte-identical (the
ADR-0020 D10 zero-change staging rule).

### D2. The seam contract is the typed value, never the exit status

The 3-valued/allowed-status exit semantics stay absorbed inside each operation (`Ancestry`,
`Option`, `bool`, empty-on-128/129) exactly as today; no exit code crosses the seam. Honest-error
helpers keep their existing backend-invariant `Message` texts. `ShoreError` gains **no** variant; a
library backend never synthesizes `GitCommand{command,status,…}`. Library rough edges are absorbed
in the backend (use `try_id()` — `.id()` panics on symbolic refs; canonicalize gix's non-normalized
common-dir paths, incl. mixed separators on Windows). The subprocess-only old-git `--path-format`
fallback has no gix analog; parity is asserted on the _resolved common dir_, not the path taken.

### D3. Cache facts, not handles; call-scoped library handles

The process-lifetime memo keeps caching only immutable discovery facts — production `RepoFact` today
is worktree root + common dir (the info-exclude-path fact has only a test-only consumer) — extended
with `object_format` (SHA-1/SHA-256, so backends mint the right empty/index trees). Cached
values are backend-agnostic facts, valid to share only because the harness proves backend equality
for them; the env selector (D4) is resolved once per process, so a memo never mixes backends mid-run.
Live `gix::Repository` handles are **call-scoped**:
opened per operation batch (measured open: ~117–130 µs mac / ~521 µs win — negligible against one
spawn) and dropped. Never share a live handle across threads: the inspect server is
thread-per-connection (`server.rs:289-338`), `git2::Repository` is `!Sync`, and gix wants
thread-local clones. The call-scoped model also discharges the measured gix hazard that a memoized
exclude stack answers `check-ignore` **stale** after an ignore-source mutation (demonstrated in the
research probe with an `info/exclude` append). Pointbreak's actual production mutation is
`ensure_pointbreak_gitignore`: it **probes `check-ignore`, then writes** any missing lines into the
committed `.pointbreak/.gitignore` (`store_init.rs:140-153` — it deliberately never touches the
per-clone `.git/info/exclude`); any later ignore probe in the same process must observe that write.
The rule is general: **`funnel-ignore` operations open/reload their exclude stack after any
ignore-source mutation, Pointbreak's own `.pointbreak/.gitignore` writes explicitly included** — and
the qualification harness performs the post-mutation re-probe as a pinned transition, so a future
long-lived-handle optimization cannot silently reintroduce the bug.

### D4. Per-class qualification and flip, in the ADR-0020 grammar

A differential harness (shape of `bench_support/foundation/candidate.rs`) runs every operation on
both backends over an edge-case fixture battery — linked worktree, detached HEAD, root commit vs
empty tree, unborn repo, non-UTF-8 path (unix), CRLF, submodule entry, type-change, mode-only,
`.gitignore` + `info/exclude` stack, dangling `origin/HEAD`, absent reflog, empty rev-range — and
asserts equality of **typed outputs**; error parity requires matching normalized category _and_
message (both-failed-differently is a divergence).

**Cargo gating (exact):** `gix = ["dep:gix"]`; `gix-parity = ["gix"]`; the harness is gated solely
on `#[cfg(feature = "gix-parity")]` — never bare `cfg(test)`, so the default `cargo test` build
compiles zero gix code. CI adds one differential lane built with `--features gix-parity` on both
platforms.

**Routing and selection (exact):** classification is **per helper, at that helper's highest-risk
use** — dispatch lives inside the helper and call sites are unchanged (D1), so one helper carries
exactly one class even when it serves several contexts (e.g. `git_rev_parse_commit_oid` serves
both capture provenance and liveness reads; `git_config_get` serves both writer identity and
signing-key discovery — each is classified at its capture-time identity-grade use). (This sharpens
the design research's call-site-based framing into helper-level classification; the gates are
unchanged, they just attach to the helper.)

| Class                                   | Helpers                                                                                                                                                                                                                  | Routable to gix?                                                                                                                                                                                                |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| read: graph/refs                        | rev-list range/reachable/reflog-reachable, reflog entries, for-each-ref, ref-state lines, merge-base is-ancestor/independent, object-exists, commit subjects, commit changed-paths, worktree list, default-branch        | yes                                                                                                                                                                                                             |
| read: ignore                            | `git_paths_are_ignored`                                                                                                                                                                                                  | yes (epoch rule, D3)                                                                                                                                                                                            |
| read: inventory                         | ls-files variants (`git_untracked_inventory`, `git_tracked_and_untracked_inventory`, `git_path_is_untracked`)                                                                                                            | yes                                                                                                                                                                                                             |
| read: config, discovery-only            | `git_config_path_get` (signing-key discovery)                                                                                                                                                                            | yes                                                                                                                                                                                                             |
| read: repo discovery                    | `git_common_dir` (store resolution; feeds the D3 memo)                                                                                                                                                                   | yes — parity asserted on the **resolved, canonicalized common dir**, never on the path taken (the old-git `--path-format` stderr fallback is subprocess-only; gix resolves structurally and has no such branch) |
| identity-grade scalars                  | `git_worktree_root`, `git_head_ref`, `git_head_oid`, `git_head_commit_oid_optional`, `git_rev_parse_commit_oid`, `git_commit_tree_oid`, `git_empty_tree_oid`, `git_config_get` (writer identity is its highest-risk use) | yes, only under identity-grade gates (below)                                                                                                                                                                    |
| write-tree (`git_write_index_tree_oid`) | —                                                                                                                                                                                                                        | **no** (D5; extended-battery gate to change)                                                                                                                                                                    |
| funnel-diff                             | two-pass diff, untracked synthesis                                                                                                                                                                                       | **no** (D5)                                                                                                                                                                                                     |

Each routable class has a compiled-in default backend constant; **promoting a class to gix-default
is a one-constant code change, made only on zero mismatch across the full battery on both macOS and
Windows plus a measured win** — and per-class rollback is the same one-constant change in reverse.
`POINTBREAK_GIT_BACKEND` is the runtime override for diagnostics and immediate mitigation, resolved
**once per process**: `subprocess` forces every routable class to subprocess (global rollback
without a release), `gix`
forces every routable class to gix (A/B testing), unset uses the compiled defaults, and any other
value is a hard error (the ADR-0020 D8 rule, incl. in-process injection for tests). The selector
can never route the two non-routable rows — diff and write-tree are subprocess by construction, not
by configuration. Flip order: the read classes first; the identity-grade scalar class second, under
its gates (SHA-1 **and** SHA-256 OID parity; byte-identical writer `config --get`
resolution across git's multi-scope precedence).
The harness keeps a two-tier diff assertion (tier 1 `object_id` equivalence; tier 2 full
`content_hash` byte-equality) so the D5 boundary stays _measurable_ even though diff is not
migrating.

### D5. The permanent hybrid boundary: diff, write-tree, and honest fixtures stay subprocess

- **Capture-time `funnel-diff` stays on subprocess git indefinitely** (2 tracked-diff spawns, +1
  `ls-files` discovery when untracked capture is enabled, +1 `--no-index` spawn per untracked
  file — the entire identity-bearing diff footprint). This is a deliberate end-state, not a transition:
  the measured rename-source divergence means gix diff fails the `object_id` bar by construction,
  and the payoff would be the smallest slice of the spawn budget against a silent-fork failure mode.
- **`git_write_index_tree_oid` (write-tree) stays on subprocess by default** (one spawn, staged/
  unstaged capture only). gix has no write-tree; the index→tree reconstruction wrapper is
  byte-verified on one fixture only. It may flip only if the reconstruction passes an extended
  battery (both object formats, gitlinks, executable bits, conflicted-entry rejection).
- **Fixtures that assert real commit OIDs or real diff bytes stay on real git.** The broader
  fixture layer migrates independently (D7).

### D6. gix-only; git2 is not adopted

No git2/libgit2 in any build. Grounds: the 2026-07-18 CVE cluster with the vendored/static-musl
path unpatched; the standing security-tracking commitment any git2 amount implies; and the design
research's per-operation sweep finding no presentation-only op where gix is `partial`/`missing` and
git2 cleanly `covered` (git2's two wins — write-tree, API present but identity-parity unverified;
fixture index-add — are both dominated by zero-dependency alternatives: subprocess retention and
gix bootstrap/build-trees-directly). **Fallback posture:** if the harness disqualifies gix on a
class, git2 may be reconsidered _for that class only_ after libgit2-sys ships a patched release and
the owner explicitly accepts the tracking cost.

### D7. Fixture lane: template-copy default, feature-gated test-only gix option

`tests/support/git_repo.rs` becomes the single fixture seam. Default bootstrap = **template-dir
copy** of a frozen `.git` skeleton (no dependency; measured ~20× cheaper on macOS and ~24× on
Windows than the 5-spawn bootstrap — the three in-process alternatives collectively span 13–31×).
Fixtures building history programmatically may use in-process `gix` init/commit gated
`#[cfg(all(test, feature = "gix"))]` — activated through the same optional `gix` feature, never a
plain dev-dependency (dev-dependencies cannot be optional), so the release closure and the default
test build stay gix-free. This
lane is independent of D1–D6 and lands in parallel; it is the largest measured suite-speed lever.

### D8. Dependency posture

One pinned gix release train (0.85.0 at drafting) behind the thin facade — no scattered per-crate
`gix-*` pins (the sub-crates lockstep-bump monthly; adopters report per-bump API adaptation).
Umbrella-with-trimmed-features vs sub-crate set is a plan-time choice made with a measured
package-count/binary-size check; either satisfies this ADR. The release binary gains gix only at
the owner-gated Option-B end-state flip; until then `gix` stays off-by-default.

## Consequences

### Accepted

- The spawn tax disappears from every qualified read class (gix: 5–>10,000× per op — graph walks
  the low end, still ≥5× faster than a spawn; ratio largest on Windows) and from the fixture layer
  (~20–24× via template-copy; 13–31× across the alternatives), while capture keeps its
  identity-bearing diff spawns — exactly 2 tracked-diff spawns, plus 1 `ls-files` discovery when
  untracked capture is enabled, plus 1 `--no-index` spawn per untracked file. On the measured
  zero/one-untracked 11–12-spawn profile, per-capture git overhead drops an estimated ~4–5×
  (≈130→30 ms mac, ≈470→85 ms win); untracked-heavy captures retain proportionally more.
- A dual-backend maintenance window and a standing parity-harness upkeep cost — accepted as the
  price of rollback-ability and of making behavior drift _measurable_ instead of latent.
- gix version churn: pre-1.0, ~monthly minor bumps with small API breaks; budgeted by the single
  pinned train + thin facade.
- A runtime `git` binary remains required (diff, write-tree, honest fixtures). Pointbreak already
  requires git by definition of its domain.
- The repo guidance amendment: the VCS abstraction is earned at the seam; "shell out first" remains
  the rule for the identity-bearing diff and honest fixture history.

### Rejected

- **Full library adoption incl. diff (subprocess retired):** largest identity blast radius (silent
  `object_id` revision forks; frozen-fixture re-bless) for the smallest spawn saving; also forfeits
  the rollback escape hatch. Rejected _now_, revisit triggers below.
- **git2 as backend or complement:** supply-chain posture (vendored C, same-day CVE cluster,
  tracking cadence not ours) outweighs its per-op speed edge when both libraries already beat
  subprocess by orders of magnitude.
- **A long-lived shared `Repository` handle** (per-repo global): unsound for git2 (`!Sync`),
  cache-hazardous for gix (stale exclude stack — measured), and unnecessary (open is µs-scale).
- **Continuing the status quo** (the prior defer): the seam is fully funneled and the design-first
  mandate supersedes the wall-clock gates; deferral no longer buys information — the parity
  questions are now answerable only by the harness this ADR creates.

## Revisit Triggers

- gix ships a `write-tree` API, multi-candidate rename-source selection, or a git-envelope patch
  emitter → re-evaluate D5's write-tree line and (with the full two-tier battery + owner re-bless
  gate) the diff boundary.
- A runtime-git-free deployment becomes a product goal → reopens the diff/write-tree boundary as a
  product decision with the tier-1/tier-2 evidence the harness already produces.
- The harness disqualifies gix on a class with an unwrappable divergence → that class stays
  subprocess (this is a supported steady state, not a failure), and the D6 fallback posture may be
  invoked.
- git2-rs demonstrates a prompt patched `libgit2-sys` release for the 2026-07-18 cluster → updates
  the D6 fallback's viability evidence (not the default choice).
- A capture-time `object_id` fork is ever observed in production attributable to backend behavior →
  immediate env-selector rollback of the implicated class + a frozen-fixture audit.

## Amendment: as-landed refinements (2026-07-19)

The seam landed as decided; five refinements sharpen the ADR's letter where implementation and
cross-platform qualification made the boundary more exact. All preserve D1–D8; none re-decides them.

- **The trait covers routable operations only; the two non-routable rows are structural, not
  configured.** `GitBackend` has one method per *routable* contract row (the 27 routable helpers).
  Capture-time `funnel-diff` and `git_write_index_tree_oid` (write-tree) are **not** trait methods
  and **not** `BackendClass` members — they remain the direct-subprocess free function / pipeline
  they were, so the selector has no surface through which it could route them. This realizes D5's
  "subprocess by construction" as a structural property rather than a runtime pin; a test injecting
  `POINTBREAK_GIT_BACKEND=gix` proves neither row moves.
- **`gix-parity` enables the diff/tree-editor capabilities its diagnostic probes need.** The
  as-landed feature is `gix-parity = ["gix", "gix/blob-diff", "gix/tree-editor", "dep:gix-imara-diff"]`.
  The illustrative `["gix"]` in D4 omits the diff capability the reads-only base `gix` feature does
  not carry; the two-tier diff assertion (D4/D5) and the write-tree reconstruction probe need it.
  `gix-imara-diff` is an optional companion pinned to the gix 0.85.0 train's own resolution and is
  activated **only** by `gix-parity` — never by `gix`, the default features, or the release binary.
  The default build stays gix-free.
- **`read:config-discovery` is held on subprocess — the class-hold mechanism working as designed.**
  `git_config_path_get`'s `git config --type=path` spelling is conditional: git renders a
  `~`-expanded value in forward-slash form but preserves an already-absolute stored path's
  backslashes on Windows, and no single normalization reproduces both. The class stays
  `RoutedBackend::Subprocess` (a supported steady state per D4); the enforcing parity gate
  reports it without failing it. This surfaced only under the forced-gix full suite on Windows, not
  the literal-path battery — evidence for keeping the forced-gix suite as the load-bearing
  production-path gate.
- **No object-format cache was added; D3's memo stayed the existing PathBuf discovery facts.** D3
  anticipated extending the process-lifetime memo with an `object_format` field. The landed backend
  needs none: the gix backend mints the empty tree directly from `repo.object_hash()` per call (SHA-1
  and SHA-256 both covered by the pinned `sha1`+`sha256` gix features), so the memo stayed exactly
  today's worktree-root + common-dir facts and no `object_format` field, type, or map was introduced —
  avoiding a dead-code cache. The identity-scalar SHA-256 OID byte-parity (below) is what guarantees
  the correct tree is minted, not a cached format.
- **Five of six routable classes qualified and flipped to gix-default under the feature.**
  `read:graph-refs`, `read:ignore`, `read:inventory`, `read:repo-discovery`, and the
  `identity-grade scalars` class each flipped one constant at a time on zero battery mismatch across
  both platforms plus a measured win; `read:config-discovery` is held (above). Measured per-op wins
  (macOS / Windows, cold discovery each call): graph-refs 11.1× / 17.3×, ignore ~17× / 25.3×,
  inventory 14.1× / 23.3×, repo-discovery 23.0× / 40.6×, identity `head_oid` 18.4× / 34.1×. The
  identity-scalar flip additionally passed SHA-1 **and** SHA-256 OID byte-parity and byte-identical
  writer `config --get` multi-scope precedence on both platforms. The default build keeps a
  single-variant enum and stays byte-identical; shipping gix by default (making it non-optional)
  remains the owner-gated end-state flip, out of scope here.

## Amendment: the Option-B default flip, exercised (2026-07-19)

The owner-gated end-state flip that D8 and the Consequences reserved ("the release binary gains gix
only at the owner-gated Option-B end-state flip") was exercised on 2026-07-19: `default = ["gix"]`
landed as `3bc8b8f` and reached `main` with the integration branch (PR #603; issue #238 closed).
This amendment records the decision, its measured price, and the security posture; it re-decides
nothing else — D1–D8 stand, and the D5 boundary (capture diff and write-tree on subprocess,
permanently) is untouched by the flip.

**Superseded statements.** Wherever this ADR says the default build is gix-free — D1's
"subprocess stays the default" build posture, D4's "a default `cargo test` compiles zero gix
code", D7's release-closure and default-test-build gix-free lines, D8's "until then `gix` stays
off-by-default", and the as-landed refinement lines "the default build stays gix-free" / "stays
byte-identical … remains the owner-gated end-state flip" — those now describe the
`--no-default-features` build, which remains the supported subprocess-only single-variant
configuration. Unchanged: `gix-imara-diff` and the
blob-diff/tree-editor capabilities still enter only through `gix-parity`, never the default build
or the release binary; `POINTBREAK_GIT_BACKEND=subprocess` remains the runtime escape hatch for
every routed class.

**Measured price of the flip** (macOS arm64, tree `3bc8b8f`, default release profile, cold builds;
CI on hosted runners):

- Release binary: 13.2 MiB → 17.5 MiB (**+4.6 MB, +33%**).
- Cold release build: **+30% wall** (25.2 s → 32.8 s), **+47% user CPU** (185 s → 273 s);
  incremental builds are unaffected once the dependency tree is cached.
- Dependency tree: **102 → 258 unique crates (+156)**.
- CI wall: neutral in steady state (warm-cache run −1.4% vs the pre-flip baseline, within runner
  noise; the first run after any manifest change pays a one-time cache-key rebuild).

**What it buys** (the program's measured record): per-capture git overhead ~4–5×
(≈130→30 ms macOS, ≈470→85 ms Windows), per-op reads 11–41×, per-capture spawns 9–11 → 2–4, local
suite wall −11.6% vs the pre-integration baseline — now in the shipped default rather than behind a
feature.

**Security posture, stated plainly.** The flip widens the default supply-chain surface by 156
pure-Rust crates (one pinned train, RUSTSEC/cargo-audit-tracked). This is the same *category* of
standing upstream-tracking commitment that D6 weighed against libgit2 — milder per unit
(memory-safe, advisory-integrated, no vendored C) but broader. Two further facts are recorded so
the clean audit picture is not over-read: (a) the *current* locked graph audits clean
(`cargo audit`: zero active advisories; the resolved `gix-features`/`gix-date` sit above the
patched thresholds), and gitoxide's advisory history is non-empty but patched —
RUSTSEC-2025-0021 (`gix-features` < 0.41.0) and RUSTSEC-2025-0140 (`gix-date` 0.10.0–0.11.1) —
so the record shows some real scrutiny with fixes shipped; independently of that, gitoxide still
receives substantially less fuzzing, deployment, and researcher attention than libgit2, so a
clean audit is weaker evidence there than libgit2's long patched record, with cargo's own gix
adoption and Rust's memory-safety guarantees as the counterweights; (b) routing reads in-process
moves the
malformed-repo-data failure domain into the pointbreak process itself — input that would previously
fail in a `git` subprocess and surface as an error can now panic the process (Rust bounds the
severity class; the runtime escape hatch and the `--no-default-features` build are the rollbacks).
A capture-time identity fork attributable to the backend remains covered by the existing revisit
trigger (immediate env-selector rollback + frozen-fixture audit).
