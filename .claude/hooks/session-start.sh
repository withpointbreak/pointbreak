#!/bin/bash
# Provision a Claude Code cloud session so `just check` runs without manual setup.
# Mirrors the tools mise.toml and the Nix shell provide for the default Rust gate:
# stable Rust with rustfmt and clippy, nightly rustfmt, just, cargo-nextest,
# Cocogitto (commit-check and git hooks) and enough history for origin/main..HEAD.
# Idempotent: each step skips work that is already done.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "$CLAUDE_PROJECT_DIR"

export PATH="$HOME/.cargo/bin:$PATH"
echo "export PATH=\"\$HOME/.cargo/bin:\$PATH\"" >> "$CLAUDE_ENV_FILE"

if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path
fi

# rust-toolchain.toml pins stable with rustfmt and clippy; `just fmt-check` uses nightly rustfmt.
rustup toolchain install stable --profile minimal --component rustfmt --component clippy
rustup toolchain install nightly --profile minimal --component rustfmt

# The image's system just can be too old to parse the Justfile (`[group]` needs 1.27+),
# so install a current one into ~/.cargo/bin, which PATH searches first.
if ! just --summary >/dev/null 2>&1; then
  cargo +stable install just --locked
fi

if ! command -v cargo-nextest >/dev/null 2>&1; then
  cargo +stable install cargo-nextest --locked
fi

# mise.toml pins Cocogitto 6.5.
if ! command -v cog >/dev/null 2>&1 || ! cog --version | grep -q ' 6\.5\.'; then
  cargo +stable install cocogitto --version '~6.5' --locked
fi

# Install the commit-msg and pre-push hooks that `just setup-hooks` would.
cog install-hook --all --overwrite

# `just commit-check` reads origin/main..HEAD, which a shallow clone cannot resolve.
if [ "$(git rev-parse --is-shallow-repository)" = "true" ]; then
  git fetch --quiet --unshallow origin || true
fi
git fetch --quiet origin main || true

# Warm the build cache so the first `just check` in the session is incremental.
cargo +stable fetch --locked
