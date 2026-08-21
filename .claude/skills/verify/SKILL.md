---
name: verify
description: Run the default Rust verification gate
---

Run the default Rust verification gate:

```bash
just check
```

This runs commit-message validation, a debug build, formatting, clippy, and the
default test suite. It is not a universal repository gate: the Inspector,
extension, release, installer, and browser surfaces have their own recipes, and
the qualification evidence harness is compiled and linted here but its tests run
only under `just test-full`. See `docs/development.md` for the change-to-gate
matrix.

If any stage fails, report the failure clearly and fix it before re-running.

If only a specific stage needs re-checking after a fix, run it individually:

- `just lint` - format + clippy only
- `just test` - the default test suite only
- `just test-full` - the default suite plus the qualification evidence harness
- `just build` - debug build only
- `just commit-check` - conventional commit history only
