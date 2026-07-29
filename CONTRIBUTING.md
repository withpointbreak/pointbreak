# Contributing To Pointbreak

Thanks for taking time to improve Pointbreak. The project is still early, so small, focused patches
are easiest to review.

## Development Setup

The toolchain is a stable Rust toolchain, a nightly toolchain (used only for formatting), `just`,
`cargo-nextest`, and Cocogitto, plus a C compiler for the bundled native dependencies. Pick whichever
environment manager you prefer — all three yield the same tools.

### Nix (recommended)

With [Nix](https://nixos.org/download) and flakes enabled:

```bash
nix develop
```

This drops you into a shell with every tool pinned by `flake.nix`. If you use `direnv` (with
`nix-direnv`), the checked-in `.envrc` activates it automatically on `cd`.

For an immutable, host-targeted equivalent of `just build-all`, use:

```bash
nix build .#build-all
```

The resulting store output contains the runnable `bin/pointbreak` and installable `pointbreak.vsix`.

### mise

```bash
mise install
```

reads `mise.toml` and installs the pinned tools. The `.envrc` also activates mise via `direnv` when
Nix is not present.

### Manual

```bash
rustup toolchain install stable
rustup toolchain install nightly
cargo install cargo-nextest --locked
cargo install cocogitto --locked
```

Finally, install the repository hooks:

```bash
just setup-hooks
```

The Nix shell and mise install these hooks for you; run the command yourself after a manual setup.
The hooks validate conventional commits and conventional branch names before changes leave your
machine.

## Common Commands

```bash
just build
just lint
just test
just check
just run --help
```

Use `just lint` before sending a patch that changes Rust code. Use `just test` for the normal test
suite. Use `just check` before opening or updating a pull request; it runs the commit check, build,
lint, and tests. It intentionally remains Rust-only. Use the change-to-gate matrix in
[docs/development.md](docs/development.md) for Inspector, extension, release, installer,
canonical-example, and browser changes. The script operating map in
[scripts/README.md](scripts/README.md) identifies preferred entrypoints and mutation boundaries.

For targeted test work:

```bash
just test-file docs_open_source_readiness
cargo +stable test --test docs_open_source_readiness
```

## Branches

Branches use conventional branch names:

```text
feat/short-description
fix/short-description
hotfix/short-description
release/short-description
chore/short-description
```

Descriptions should use lowercase letters, numbers, and hyphens. For documentation-only work,
`chore/<description>` is the safe branch prefix currently accepted by the repository hook.

## Commits

Use conventional commits:

```text
docs: add getting started guide
fix: correct input request projection
feat: add review unit discovery
```

The commit subject should be lowercase, imperative, and no more than 100 characters. Do not end it
with a period.

Use an unscoped commit unless `cog.toml` grows an explicit scopes list. Today the scopes list is
empty, so scoped commits such as `docs(readme): ...` are rejected.

Check the current branch against the upstream default branch. In a fork, add an `upstream` remote
that points to `withpointbreak/pointbreak`; in the maintainer clone, replace `upstream` with `origin`
if `origin` is the upstream repository.

```bash
cog check upstream/main..HEAD
```

## Pull Requests

Keep pull requests narrow:

- describe what changed and why
- include the validation commands you ran
- update docs when user-facing behavior, command output, setup, or release process changes
- keep generated or unrelated files out of the diff
- avoid public references to private planning or local assistant workflow

CI runs formatting, linting, tests, and conventional commit checks across the supported runner
matrix.

## Project Shape

Pointbreak is a Rust terminal review tool. Keep the headless review model authoritative and make the
TUI or other surfaces project from that model. Public command output JSON is the integration
surface; raw files in the resolved Pointbreak store are local storage details unless a command
explicitly documents them.
