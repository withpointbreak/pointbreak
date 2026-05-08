# List available recipes.
default:
    @just --list

# Run all tests.
test *args:
    cargo +stable nextest run --no-tests pass {{ args }}

# Run all tests (CI mode: no fail-fast, verbose).
test-ci *args:
    cargo +stable nextest run --profile ci --no-tests pass {{ args }}

# Run a specific test file (e.g. just test-file integration).
test-file name *args:
    cargo +stable nextest run --test {{ name }} {{ args }}

# Build (debug).
build *args:
    cargo +stable build {{ args }}

# Build (release).
release *args:
    cargo +stable build --release {{ args }}

# Run clippy and fmt check.
lint: fmt-check
    cargo +stable clippy --workspace --all-targets --all-features -- -D warnings

# Run clippy with auto-fix.
fix *args: fmt
    cargo +stable clippy --fix --workspace --all-targets --all-features --allow-dirty --allow-staged -- -D warnings {{ args }}

# Format code.
fmt *args:
    cargo +nightly fmt --all {{ args }}

fmt-check:
    cargo +nightly fmt --all -- --check

# Install git hooks (commit-msg and pre-push validation via cocogitto).
setup-hooks:
    cog install-hook --all --overwrite

# Check commit messages on the current branch.
commit-check range='origin/main..HEAD':
    cog check "{{ range }}"

# Run the CLI.
run *args:
    cargo +stable run -- {{ args }}

# Check commit messages, compile, lint, and tests.
check: commit-check build lint test
