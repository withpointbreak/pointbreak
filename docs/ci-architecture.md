# CI architecture

Why continuous integration is split the way it is, what was measured, and what would justify
changing it again. [docs/development.md](development.md) covers which gate a given change needs;
this file covers why the gates are built the way they are.

The workflows carry short comments at each decision point that name the rule and point here. If
you are about to change one of them, read the matching section first — several of the current
choices look like oversights until you see the number behind them.

## The workflows

| Workflow | Runs | Purpose |
| --- | --- | --- |
| `ci.yml` | every PR, all three platforms | The whole gate. The suite on each platform — Linux as sandboxed derivations, macOS as plain cargo, Windows from a cross-compiled archive — plus installers, skills, workflow lint, the front-end and extension checks, store qualification, and git parity. |
| `nightly.yml` | nightly and on demand | What is too slow to gate: the hermetic check on macOS, and `nix flake check` in full. |
| `nix.yml` | PRs touching `*.nix` | Lints the Nix files themselves. Builds nothing. |
| `cache-gc.yml` | nightly on the default branch | Prunes the hestia cache so the Nix lane stays inside the shared 10GB quota. |

Every leg compiles with the same flake-pinned toolchain, so the Linux job is not a second opinion
on the lints. What only it establishes is stated in [Why keep a Nix lane](#why-keep-a-nix-lane).

The Linux leg lives in `ci.yml` alongside the other two rather than in its own workflow. It ran
separately while it was still an experiment, but that left `ci.yml` showing macOS and Windows
tests and no Linux — a gate that reads as though it has a hole in it.

## Toolchain comes from the flake

`CONTRIBUTING.md` tells contributors to set up with `nix develop`. When CI resolved its own
toolchain through rustup, the compiler that gated a change was not the compiler contributors
built with. Linux and macOS now run `nix develop .#ci -c just <recipe>`, so both agree, pinned by
`flake.lock`.

`.#ci` is a separate, lean shell. The default shell carries interactive extras — cocogitto, which
is built from source in this flake, plus `gh` and `cargo-edit` — that no workflow uses.

**Windows still uses rustup**, because Nix has no native Windows support. Retiring that is the
open item; see [Windows](#windows).

## Why keep a Nix lane

The macOS and Windows legs get their speed from a `rust-cache`-restored `target/` and a mutable
working directory.
That makes a stale-artifact false green *possible* there. In a Nix derivation it is impossible:
no host tools, no network, nothing reused from a cache. That property, on Linux, is the entire
reason the lane exists.

Linux gates every PR, as `ci.yml`'s `test-linux` job. macOS runs the same check nightly instead,
for two reasons: it measured 22m37s against Linux's 15m46s, and **Nix does not sandbox builds on macOS by default** (`sandbox = false` is the
Darwin default), so the hermeticity argument is weaker there anyway.

## Decisions, with the numbers

### Test gates build with `CARGO_PROFILE = "test"`

Crane defaults every derivation to `release`, so the gates were paying optimized codegen for
throwaway test binaries. Cargo's `test` profile is what `just lint` and `just test` use locally,
and what crane's own checks use.

| | release | test |
| --- | --- | --- |
| crate + test binaries compile | 2m54s | **25s** |
| suite execution | 253s | **179s** |

Both axes improved; release was actively hurting. The delivery build keeps its own release
artifacts, so `nix build .#build-all` is still optimized.

### The clippy gate installs no cargo artifacts

`cargoClippy` used to install its compiled target directory as the derivation output: a
164–255MB archive that nothing consumed, that changed on every commit, and that cost the build a
632MB → 164MB compression pass. `cargoNextest` already set `doInstallCargoArtifacts = false` for
the same reason. Clippy's output is now 0B. The reusable part is the shared dependency artifacts,
built separately.

### The PR gate names its checks; the nightly one does not

`nix flake check` builds every check in one arbitrary-order batch, so it cannot express "cheap
gate first". Naming the checks individually can, which is why the PR job runs
`fmt → clippy → test → build` and stops at the first failure.

The cost is that the explicit list can drift from the flake's actual `checks`. The nightly job
runs `nix flake check` precisely so a newly added check cannot be silently left ungated.

### Reading the Nix lane's logs

`nix build` prints nothing on success, so a green Linux gate showed no evidence the suite had
run — the job log contained no test counts at all. Passing `-L` fixes that by streaming every
compile line, which is what made these runs unreadable in the first place.

The `suite summary` step splits the difference: it pulls the counts back out of the derivation's
own build log after the fact. Two things to know when reading it. Nextest colourises even inside
the sandbox — it has no TTY, but nothing tells it not to, and the workflow's `CARGO_TERM_COLOR`
does not reach into a derivation — so the escape codes are stripped before matching. And on a
cache hit there is no local log, because nothing was built; the step says so rather than printing
an ambiguous blank. That case is not a gap: the derivation output *is* the proof those exact
inputs passed, just from an earlier run.

### Store caching

Three approaches, each of which hit a different GitHub ceiling:

| Approach | Granularity | Outcome |
| --- | --- | --- |
| `cache-nix-action` | one tarball of the whole `/nix/store`, per platform | 4.87GB (Linux) + 4.46GB (macOS) = **9.33GB before anything else**, against a 10GB storage limit. The platforms evicted each other and Linux ran cold every time. |
| `magic-nix-cache` | one entry per store path | **1788 entries**, whose API calls tripped GitHub's rate limiter. On throttle it logs `Not trying to use it again on this run` and disables itself, so the job silently finishes cold. |
| `Mic92/hestia` (current) | content-defined chunks, a few large entries | **491 paths packed into 37 entries** — about 48x fewer objects than per-path storage, with no throttling observed. |

Measured on this repository: a cold run costs 22m52s and drains 2.3 GiB in about a minute;
re-running the same commit costs **48s** end to end (fmt 17s, clippy 2s, test 3s, build 8s),
because identical inputs make every derivation output a cache hit. A real change still
recompiles the crate and its test binaries — only the dependency artifacts stay cached — so
expect a normal pull request to land between those two numbers rather than near the low one.

Hestia needs no account: uploads authenticate with the runner-injected
`ACTIONS_RUNTIME_TOKEN`, so build jobs need only `permissions: contents: read`. It is
pinned by commit SHA, matching how this repository pins its other third-party actions.

`cache-gc.yml` prunes it nightly. Hestia tracks liveness through *roots* — one per branch
and system, e.g. `main-x86_64-linux` — and collects whatever no root reaches once it falls
out of the push grace period. It only ever considers paths hestia itself pushed, so the
rust-cache and setup-node entries sharing the quota are untouched. GC must run on the
default branch, because a pull request's cache scope is read-only towards it, and it is the
only workflow here holding `actions: write`.

If quota is still the binding constraint after that, the action takes an
`upstream-cache-filter` input that skips paths already signed by an upstream cache.

> Two corrections worth keeping, because both cost time here. Magic Nix Cache broke in
> February 2025 and was widely written off, but was revived in June 2025 against the new
> API — it is not dead. And hestia *is* MIT licensed (README and both `Cargo.toml`
> files); GitHub's license detector reports nothing only because there is no `LICENSE`
> file at the repository root.

### `nix.yml` lints Nix files and nothing else

It used to run `just nix-check`, whose `nix flake check` builds every check — clippy, the whole
suite, tool drift — with no store cache. A job named "Format and lint" was taking 14–18 minutes
and duplicating the Linux gate from cold. It now runs `just nix-lint` (nixfmt, statix, deadnix) in
about a second. `just nix-check` keeps the fuller behaviour for local use.

`nix flake check --no-build` is not a cheap substitute: `cleanCargoSource` filters a derivation
output, so evaluation has to build the source derivation first (import-from-derivation).

## Rejected alternatives

**A full Nix gate on both platforms, per PR.** Ran fmt, clippy, tests, and the artifact build on
Linux and macOS. It re-checked lints `ci.yml` already covers, on the same code with the same
toolchain, for roughly 1.5× the time — and macOS was the wall-clock long pole at 22m37s. Only the
sandboxed suite was unique, so only that was kept.

**`Swatinem/rust-cache` on the Nix lane.** Frequently suggested, but it caches `~/.cargo` and
`./target`, and `nix build` runs in a sandbox that never writes the workspace `target/`. It would
cache an empty directory. It is the right tool for `ci.yml`, which does run cargo in the
workspace, and it is used there.

**`nix-options: "sandbox = false"` on macOS.** Suggested as a macOS speed-up. It is a no-op:
`sandbox = false` is already the Darwin default (`/etc/nix/nix.conf` sets no `sandbox` line and
Nix still reports `false`). On Linux it would be actively harmful, deleting the one property the
Nix lane exists to provide. It remains a legitimate *debugging* lever for a derivation that fails
only under the sandbox.

**Cachix.** Would work, and is the one option where GitHub's rate limits cannot apply at all,
since it does not use the Actions cache. The footprint is ~3.6GB against a 5GB free
open-source tier, which is workable but not roomy, and it needs an account plus a token
secret. It stays the fallback if the current approach hits either ceiling again, or if the
cache is ever wanted outside CI.

## Revisit triggers

- **`ci.yml` produces a false green traced to a stale `target/`.** The strongest argument for
  moving more of the gate into derivations.
- **Either GitHub cache ceiling is hit again** — storage (10GB, shared with `ci.yml`'s
  rust-cache entries) or the API rate limit. First try hestia's `upstream-cache-filter` and a
  GC workflow; if that is not enough, move to a real binary cache (Cachix or FlakeHub), which
  is subject to neither.
- **macOS runners get materially faster,** or a macOS-specific sandbox regression appears — then
  reconsider gating macOS per PR rather than nightly.
- **Nix gains native Windows support,** or the cross-compile spike graduates — either removes the
  last rustup dependency.
- **Test execution starts dominating compile time.** The `test` profile was chosen when compiling
  dominated; if that inverts, re-measure `release`.

## Windows

`ci.yml`'s `windows-cross-archive` job cross-compiles the suite to `x86_64-pc-windows-msvc` on
Linux with cargo-xwin; three `test-windows` shards then execute the resulting `cargo nextest`
archive on plain Windows runners that build no Rust at all. This was a report-only spike until it
proved both green and competitive on x64, and is now the gate.

It became possible once the suite was changed to resolve its test binary, fixtures, and cargo at
runtime rather than through compile-time `env!()`, which bakes the build machine's paths in and
which `--workspace-remap` cannot relocate.

`tests/runtime_path_resolution.rs` guards that convention. Reintroducing a compile-time path is
easy to do by accident and fails nowhere except the Windows shards, with a bare "system cannot
find the path specified" that names no cause — so the guard fails on Linux instead, naming the
file, the line, and the resolver to use. `include_str!`/`include_bytes!` are exempt: they embed
bytes, so no path survives into the binary.

**Rustup is not gone from Windows.** Three things still need a toolchain there: the
`--all-features` type-check (`test-windows-check`, which compiles the `cfg(windows)` arms behind
`bench` and `gix-parity` that the default-feature archive never sees), plus the
store-foundation-qualification and git-parity legs. Only the test execution was freed.

The Windows run is now fanned across three shards, the same `slice:N/3` partitioning `ci.yml`
uses, which splits evenly (2922 tests, 974 per shard, reconciling exactly). Before that it was a
single leg and the lane cost 15m20s — cross-compile then run, serialized — against 10m02s for the
sharded rustup legs.

The shape is better than `ci.yml`'s, not just equal: there, each Windows shard rebuilds the test
binaries, so sharding multiplies compile work. Here the archive is cross-built once on Linux and
all three shards consume it, so sharding buys execution time and nothing else. Whether that is
enough to beat the rustup legs is now a measurement rather than an argument.

It stays report-only until it is confirmed green on the x86_64 runner across a few runs — the
2924/2924 validation was on ARM64.

No shard compiles Rust. The one test that did — a `cargo install` packaging check — was a
leftover of the `shore` -> `pointbreak` rename and has been retired; the invariant it still
carried (exactly one installed binary, named `pointbreak`) is asserted from `cargo metadata`
by `package_identity_declares_only_pointbreak_binary`. Retiring it also removed the ~2-3
minute on-target build that made whichever shard drew it the long pole.

Nothing now exercises `cargo install` itself, which is the route the README and
[installation guide](installation.md) give users. That path is covered indirectly by the
release workflows building and verifying the published binaries, but not directly.
