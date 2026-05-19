# AGENTS.md

This file provides guidance to AI code assistants when working with code in this repository.

## Project Overview

Shore is an experimental Rust terminal review tool for understanding what a coding agent changed
and why. It should stay focused on a small, Rust-native agent-review core with a data model that is
easy to reason about, test, and eventually expose to other tools.

Build the review stream as a pure, headless data layer before building the TUI. Rendering,
scrolling, navigation, and note placement should derive from one explicit review-stream model rather
than parallel sources of truth.

## Commit Conventions

This project uses [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), enforced
by [cocogitto](https://docs.cocogitto.io/) via a `commit-msg` git hook.

Format: `<type>(<optional scope>): <subject>`

Types: `feat`, `fix`, `perf`, `revert`, `docs`, `test`, `build`, `ci`, `refactor`, `chore`, `style`

Suggested scopes: `cli`, `model`, `tui`, `git`, `review`, `fixtures`. Scopes are optional and not
yet enforced.

Rules:

- Header must be 100 characters or fewer
- Subject must start with a lowercase letter
- Subject must not end with a period
- Use imperative mood ("add feature" not "added feature")

For non-trivial changes, include a body after a blank line explaining what changed and why. A
one-liner is fine for truly simple changes.

Use `cog check` to validate commit history and `cog changelog` to preview changelog output. Use
`git commit` for creating commits; the commit-msg hook handles validation.

## Branch Conventions

This project uses Conventional Branch names, enforced by a `pre-push` git hook.

Format: `<type>/<description>`

Types: `feat/`, `fix/`, `hotfix/`, `release/`, `chore/`

Rules:

- Use lowercase letters, numbers, and hyphens only
- Include issue numbers when applicable, such as `feat/issue-42-add-review-stream`
- Keep descriptions concise

## Common Commands

Use `just` for day-to-day work. Tests use `cargo-nextest` for parallel execution.

```bash
just test                      # Run all tests
just test-file integration     # Run a specific test file
just test -E 'test(test_name)' # Run a specific nextest filter
just lint                      # fmt check + clippy
just check                     # commit check + build + lint + test
just build                     # Debug build
just release                   # Release build
just run --help                # Run the CLI
just fmt                       # Format code
```

## Implementation Guidance

Keep the first version deliberately smaller than hunk. Prefer shelling out to `git` at first, and
let a VCS abstraction come later if the review model earns it.

The headless model should own file identity, file order, hunk identity, row geometry, note anchors,
navigation cursors, and serializable review/session state. The TUI should be a projection of that
model, not the authoritative owner of review semantics.

Shore's internal architecture language treats review units, task attempts, and similar subjects as
software work objects coordinated through an append-only event log and purpose-built projections.
Read `docs/substrate-language.md`, `docs/substrate-thesis-summary.md`, and
`docs/adr/adr-0003-agent-resource-claims-advisory-first.md` before substrate-shaped refactors.
Substrate vocabulary is internal; user-facing commands and JSON documents should stay domain-named.

## Testing

Start with headless tests before TUI tests. Useful fixtures include multi-file diffs with sidecar
ordering, untracked files, renames, binary and mode-only changes, context-row note anchors,
annotated-hunk navigation, terminal resize geometry, and large synthetic changesets.
