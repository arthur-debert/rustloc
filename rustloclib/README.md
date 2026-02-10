# rustloclib

A Rust-aware lines-of-code counter library. Unlike generic LOC tools, rustloclib understands that Rust tests live alongside production code and correctly separates them — even in the same file.

[![Crates.io](https://img.shields.io/crates/v/rustloclib.svg)](https://crates.io/crates/rustloclib)
[![Documentation](https://docs.rs/rustloclib/badge.svg)](https://docs.rs/rustloclib)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Line types

Every line is classified into one of six types:

- **code** — production logic lines
- **tests** — test logic lines (`#[test]`, `#[cfg(test)]`, `tests/`)
- **examples** — example logic lines (`examples/`)
- **docs** — doc comments (`///`, `//!`, `/** */`, `/*! */`)
- **comments** — regular comments (`//`, `/* */`)
- **blanks** — whitespace-only lines

## Quick start

```toml
[dependencies]
rustloclib = "0.8"
```

### Count a workspace

```rust,ignore
use rustloclib::{count_workspace, CountOptions};

let result = count_workspace(".", CountOptions::new())?;
println!("Code: {}, Tests: {}, Docs: {}", result.total.code, result.total.tests, result.total.docs);
```

### Count a single file

```rust,ignore
use rustloclib::count_file;

let stats = count_file("src/lib.rs")?;
println!("Code: {}, Tests: {}", stats.code, stats.tests);
```

### Filter by crate or glob

```rust,ignore
use rustloclib::{count_workspace, CountOptions, FilterConfig};

let filter = FilterConfig::new().exclude("**/generated/**")?;
let result = count_workspace(".", CountOptions::new()
    .crates(vec!["my-lib".into()])
    .filter(filter))?;
```

### Diff between commits

```rust,ignore
use rustloclib::{diff_commits, DiffOptions};

let diff = diff_commits(".", "HEAD~5", "HEAD", DiffOptions::new())?;
println!("Code: +{}/-{}", diff.total.added.code, diff.total.removed.code);
```

### Working directory diff

```rust,ignore
use rustloclib::{diff_workdir, DiffOptions, WorkdirDiffMode};

let diff = diff_workdir(".", WorkdirDiffMode::Staged, DiffOptions::new())?;
```

## Data pipeline

The library is organized into four stages:

```text
source  →  data  →  query  →  output
Find       Parse    Filter    Format
files      & count  & sort    strings
```

### Full pipeline example

```rust,ignore
use rustloclib::{
    count_workspace, CountOptions, CountQuerySet, LOCTable,
    Aggregation, LineTypes, Ordering,
};

// Stages 1–2: discover and count
let result = count_workspace(".", CountOptions::new())?;

// Stage 3: query (filter line types, aggregate, sort)
let queryset = CountQuerySet::from_result(
    &result,
    Aggregation::ByCrate,
    LineTypes::everything(),
    Ordering::by_code(),
);

// Stage 4: format for display
let table = LOCTable::from_count_queryset(&queryset);
```

## Key types

| Type | Description |
|------|-------------|
| `Locs` | Counts for a single item: `code`, `tests`, `examples`, `docs`, `comments`, `blanks`, `all` |
| `CountResult` | Result from counting: `total`, `crates`, `modules`, `files` |
| `DiffResult` | Result from diffing: `total`, `crates`, `files` (each with `LocsDiff`) |
| `LocsDiff` | Added/removed `Locs` with `net_*()` helpers |
| `CountOptions` | Builder for counting: `.crates()`, `.filter()`, `.aggregation()`, `.line_types()` |
| `DiffOptions` | Builder for diffing: same API as `CountOptions` |
| `Aggregation` | `Total`, `ByCrate`, `ByModule`, `ByFile` |
| `LineTypes` | Which columns to include: `default()`, `everything()`, `code_only()`, etc. |
| `Ordering` | Sort control: `by_code()`, `by_tests()`, `by_total()`, `by_label()` |
| `FilterConfig` | Glob-based file filtering: `.include()`, `.exclude()` |

All data types implement `serde::Serialize` and `serde::Deserialize`.

## Acknowledgments

The parsing logic is adapted from [cargo-warloc](https://github.com/Maximkaaa/cargo-warloc) by Maxim Gritsenko (MIT licensed).

## License

MIT License
