---
name: test
description: Run tests with optional nextest filter expression
---

Run project tests using cargo-nextest.

If `$ARGUMENTS` is provided, pass it as a nextest filter expression:

```bash
just test -E '$ARGUMENTS'
```

If no arguments are provided, run the default suite:

```bash
just test
```

`just test` excludes the qualification evidence harness, which is behind the `bench`
feature. Use `just test-full` for the default suite plus that harness when the change
touches `src/bench_support/**`, `src/session/benchmark.rs`, `benches/store_foundation.rs`,
or any longitudinal counting site:

```bash
just test-full
```

Common filter patterns:

- `test(test_name)` - match test by name
- `test(~keyword)` - fuzzy match
- `package(shore)` - only the main crate

To run a specific test file instead of a filter expression, use:

```bash
just test-file <name>
```

where `<name>` is the filename without extension.
