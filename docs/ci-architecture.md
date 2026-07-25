# CI architecture

Why continuous integration is split the way it is, what was measured, and what would justify
changing it again. [docs/development.md](development.md) covers which gate a given change needs;
this file covers why the gates are built the way they are.

The workflows carry short comments at each decision point that name the rule and point here. If
you are about to change one of them, read the matching section first — several of the current
choices look like oversights until you see the number behind them.

## The two lanes

| Workflow | Runs | Purpose |
| --- | --- | --- |
| `ci.yml` | every PR, all three platforms | The gate. Lint, type-check, and the full suite, with `rust-cache` and Windows sharding. |
| `ci-nix.yml` | every PR (Linux); nightly (Linux + macOS) | The hermetic check: the suite in a sandbox, plus a network-free build of the shipped artifact. |
| `nix.yml` | PRs touching `*.nix` | Lints the Nix files themselves. Builds nothing. |
| `ci-nix-windows-spike.yml` | spike branch only | Report-only experiment: cross-compile the tests on Linux, run them on Windows. |

Both lanes compile with the same flake-pinned toolchain, so `ci-nix.yml` is not a second opinion
on the lints. Its unique contribution is stated in [Why keep a Nix lane](#why-keep-a-nix-lane).

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

`ci.yml` gets its speed from a `rust-cache`-restored `target/` and a mutable working directory.
That makes a stale-artifact false green *possible* there. In a Nix derivation it is impossible:
no host tools, no network, nothing reused from a cache. That property, on Linux, is the entire
reason the lane exists.

Linux gates every PR. macOS runs nightly instead, for two reasons: it measured 22m37s against
Linux's 15m46s, and **Nix does not sandbox builds on macOS by default** (`sandbox = false` is the
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

### Store caching uses Magic Nix Cache

`cache-nix-action` tars the whole `/nix/store` into one entry per platform. Measured here: 4.87GB
on Linux and 4.46GB on macOS — **9.33GB before counting anything else**, against GitHub's 10GB
per-repository limit. The two platforms evicted each other, and Linux ran with a cold store on
every run.

Magic Nix Cache stores individual paths and skips anything fetchable from `cache.nixos.org`, so
only this project's own outputs are kept (~3.6GB across both platforms). It needs no external
account. `use-flakehub: false` keeps it on the Actions cache; FlakeHub Cache is a separate
account-backed service.

> Magic Nix Cache broke in February 2025 when GitHub retired the old Actions Cache API, and was
> widely described as dead. It was revived in June 2025 against the new API. Check its current
> state before acting on any advice about it.

### `nix.yml` lints Nix files and nothing else

It used to run `just nix-check`, whose `nix flake check` builds every check — clippy, the whole
suite, tool drift — with no store cache. A job named "Format and lint" was taking 14–18 minutes
and duplicating `ci-nix.yml` from cold. It now runs `just nix-lint` (nixfmt, statix, deadnix) in
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

**Cachix.** Would work — the footprint is ~3.6GB against a 5GB free open-source tier — but it
needs an account and a token secret, and Magic Nix Cache solved the eviction problem without
either. Worth revisiting if the cache is ever needed outside CI.

## Revisit triggers

- **`ci.yml` produces a false green traced to a stale `target/`.** The strongest argument for
  moving more of the gate into derivations.
- **Actions cache pressure returns.** The 10GB limit is shared; both lanes' caches coexist only
  because the whole-store tarballs are gone. If usage approaches the ceiling again, the next step
  is a real binary cache (Cachix or FlakeHub), not more tuning.
- **macOS runners get materially faster,** or a macOS-specific sandbox regression appears — then
  reconsider gating macOS per PR rather than nightly.
- **Nix gains native Windows support,** or the cross-compile spike graduates — either removes the
  last rustup dependency.
- **Test execution starts dominating compile time.** The `test` profile was chosen when compiling
  dominated; if that inverts, re-measure `release`.

## Windows

`ci-nix-windows-spike.yml` cross-compiles the suite to `x86_64-pc-windows-msvc` on Linux with
cargo-xwin and runs the resulting `cargo nextest` archive on a plain Windows runner that builds no
Rust at all. It works: validated against an ARM64 Windows machine at 2924/2924 after the test
suite was changed to resolve its binary and fixtures at runtime rather than through compile-time
`env!()`.

It stays report-only because it is not yet competitive on wall-clock: 15m20s (cross-compile then
run, serialized) against 10m02s for the sharded rustup legs. Sharding the Windows run the way
`ci.yml` shards it is the work required before it can replace rustup on Windows.

One test is an intentional exception to the no-build premise:
`package_identity::cargo_install_exposes_only_pointbreak_executable` builds the crate on the
runner by design.
