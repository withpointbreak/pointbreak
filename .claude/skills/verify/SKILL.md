---
name: verify
description: Run full project verification
---

Run the full project verification suite:

```bash
just check
```

This runs commit-message validation, a debug build, formatting, clippy, and
tests.

If any stage fails, report the failure clearly and fix it before re-running.

If only a specific stage needs re-checking after a fix, run it individually:

- `just lint` - format + clippy only
- `just test` - tests only
- `just build` - debug build only
- `just commit-check` - conventional commit history only
