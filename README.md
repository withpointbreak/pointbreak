# Pointbreak Review

[![Crates.io](https://img.shields.io/crates/v/pointbreak.svg)](https://crates.io/crates/pointbreak)
[![Documentation](https://docs.rs/pointbreak/badge.svg)](https://docs.rs/pointbreak)
[![CI](https://github.com/withpointbreak/pointbreak/actions/workflows/ci.yml/badge.svg)](https://github.com/withpointbreak/pointbreak/actions/workflows/ci.yml)

Pointbreak Review is a durable, local-first review record for code changes that humans and coding
agents build together. It is designed for the iteration that happens long before a pull request
opens, where you might guide one agent to author a change and another to review it.

Decisions outlive conversations. Coding agents generate far more activity than anyone can follow.
Rather than store or replay full transcripts, Pointbreak keeps only the facts that move a review
forward: what changed and why, the open questions, and each assessment. It records them as an
append-only log you can read in the terminal, browse in a local web inspector, or consume as JSON.

Every fact carries the actor that asserted it, human or agent, and can be signed with an Ed25519 key.
Signing never blocks a write, but when a signature is present the record becomes tamper-evident, and
a reader can tell whether each fact is merely signed or bound to a trusted identity. See
[docs/signing-ux.md](docs/signing-ux.md).

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/withpointbreak/pointbreak/main/assets/shore-inspector-dark.png">
  <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/withpointbreak/pointbreak/main/assets/shore-inspector-light.png">
  <img alt="The Pointbreak Review inspector: a filterable event timeline with per-actor tracks and signature-trust badges, beside an event detail pane" src="https://raw.githubusercontent.com/withpointbreak/pointbreak/main/assets/shore-inspector-light.png" width="800">
</picture>

*Watching a review in the Pointbreak Review inspector opened by `pointbreak inspect`: the event timeline, each fact attributed to its track, with signature-trust badges.*

## Install

On macOS or Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.sh | sh
```

On Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.ps1 | iex
```

The installers select the correct release archive, verify its SHA-256 checksum, and install the
`pointbreak` command. The published `pointbreak` crate also provides the `pointbreak` command and can be
installed with `cargo install pointbreak`.

Release `0.7.0` is a hard operational cutover to this executable and the canonical Pointbreak
environment and storage names. Existing installations must move local state offline before use; see
[Upgrading to 0.7.0](docs/installation.md#upgrading-to-070).

See [Installation](docs/installation.md) for version pinning, custom install directories, supported
platforms, manual downloads, and checksum verification.

## Quick Start

Make a real change in a Git repository — modify a tracked file — then capture it with a useful
summary and open Review:

```bash
cd path/to/git-worktree
pointbreak capture --summary "Explain the fallback behavior"
pointbreak inspect --open
```

Review is a local, read-only view of the durable record: the captured diff, every fact on its
author's track, and the current call. A review moves through five stages —
`Work -> Claims -> Evidence -> Questions -> Call` — owned by the existing `capture`/`revision`/
`inspect`, `observation`, `validation`, `input-request`, and `assessment` command families.

Continue with the complete paired author/reviewer loop — claims, validation evidence, questions,
the call, and landing the commit on the same revision — in
[docs/getting-started.md](docs/getting-started.md).

In a real collaboration each actor records on its own track — the coding agent that authored the
change, a reviewer that is a human or another agent, and you — so every fact stays attributed to
whoever asserted it. See [the review workflow](docs/review-workflow.md) and
[agent authoring handoffs](docs/agent-authoring.md) for how the author and reviewer hand off.

Repository config lives in `.pointbreak/`. Review facts normally live in the Git common directory's
`pointbreak/` store, shared by every linked worktree; an ephemeral worktree uses `.pointbreak/data/`.
Run `pointbreak store paths --format text` to see the canonical locations for a repository. Command
output JSON is the integration surface; raw event files, artifact paths, and `state.json` are internal
storage details unless a command explicitly documents them. Consumers that prefer to read and write
those facts in process can use the supported library API instead of the CLI — see
[docs/library-api.md](docs/library-api.md).

## Commands

The `pointbreak` command surface is still taking shape and will change before v1. See
[docs/cli-reference.md](docs/cli-reference.md) for the current commands, their options, output
documents, schema names, and V1 limitations.

## Agent Skills

Pointbreak ships a portable author-handoff skill under [skills/](skills/README.md). Install it with:

```bash
npx skills add withpointbreak/pointbreak
```

## Documentation

For users:

- [Getting started](docs/getting-started.md) - first local review from a scratch Git repository.
- [Installation](docs/installation.md) - installers, releases, supported platforms, and checksums.
- [CLI reference](docs/cli-reference.md) - commands, options, output JSON, and V1 boundaries.
- [Review workflow](docs/review-workflow.md) - the five review stages, the author and reviewer
  roles, and when to reach for each command family.
- [Agent authoring handoffs](docs/agent-authoring.md) - how a coding agent captures a durable
  handoff record before declaring implementation work done.
- [Agent skills](skills/README.md) - install the portable Pointbreak author-handoff skill.
- [Library API](docs/library-api.md) - the supported in-process library surface (reads, attributed
  writes, event ingest, documents) and its stability contract.
- [Signing UX](docs/signing-ux.md) - human, agent, and CI signing flows and the
  unsigned/untrusted_key/valid verification ladder.

For contributors and maintainers:

- [CONTRIBUTING.md](CONTRIBUTING.md) - setup, hooks, branch names, commits, tests, and PR flow.
- [docs/releasing.md](docs/releasing.md) - release planning and publish automation.
- [docs/manual-testing.md](docs/manual-testing.md) - maintainer spot-check recipes.
- [TRADEMARKS.md](TRADEMARKS.md) - trademark use for names and logos.

Architecture and model notes:

- [docs/storage-model.md](docs/storage-model.md) - durable events, artifacts, and rebuildable
  projections.
- [docs/input-request-model.md](docs/input-request-model.md) - operative and advisory input
  requests.
- [docs/assessment-model.md](docs/assessment-model.md) - review assessments and replacements.
- [docs/adr/](docs/adr/) - architectural decision records.

## Project Status

Pointbreak Review is experimental and under active development. The crate and sole installed command
are both `pointbreak`.

The current focus is a headless, durable review model and the surfaces derived from it:

- Git working-tree or commit-range (`--base`) capture into a revision
- append-only local events in the resolved Pointbreak store
- immutable snapshot and note-body artifacts in that store
- rebuildable projections and command-output JSON
- read-only terminal and local web views over the same model

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. The short validation path is:

```bash
just setup-hooks
just check
```

Security-sensitive reports should follow [SECURITY.md](SECURITY.md), not public issues.

## License And Trademarks

This repository's source code is licensed under Apache-2.0. See [LICENSE](LICENSE).

Pointbreak, Pointbreak Review, and the Pointbreak logo are trademarks of Kevin Swiber.
Trademark rights are reserved; see [NOTICE](NOTICE) and [TRADEMARKS.md](TRADEMARKS.md).
