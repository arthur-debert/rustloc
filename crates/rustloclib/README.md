# rustloclib

A Rust-aware lines-of-code counter library. Unlike generic LOC tools, rustloclib understands that Rust tests live alongside production code and correctly separates them — even in the same file.

[![Crates.io](https://img.shields.io/crates/v/rustloclib.svg)](https://crates.io/crates/rustloclib)
[![Documentation](https://docs.rs/rustloclib/badge.svg)](https://docs.rs/rustloclib)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Line types

Every line is classified into one of six types:

- **code** — production logic lines
- **tests** — test logic lines (`#[test]`, `#[cfg(test)]`, `tests/`, and whole files a parent module declares only under `cfg(test)`)
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

Counting a workspace also reads its Cargo module graph, because a Rust file's
own contents do not say whether it is production code. When `archive.rs`
declares `#[cfg(all(test, unix))] #[path = "archive_tests.rs"] mod tests;`,
every line of `archive_tests.rs` belongs to the test build, and rustloclib
counts them as tests. Cargo test targets and the modules only they reach are
test code for the same reason.

`count_directory` and `count_file` stay file-local: given a directory or one
path, rustloclib does not search parent directories for a manifest, so those
entry points classify each file on its own syntax.

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
use rustloclib::{diff_revspec, DiffOptions};

// Single revspec — resolved by `git rev-parse`. Accepts ranges (`a..b`),
// merge-base (`a...b`), tags, branches, short hashes, HEAD~N. A single
// rev is diffed against HEAD. Requires `git` on PATH.
let diff = diff_revspec(".", "HEAD~5..HEAD", DiffOptions::new())?;
println!("Code: +{}/-{}", diff.total.added.code, diff.total.removed.code);
```

### Working directory diff

```rust,ignore
use rustloclib::{diff_workdir, DiffOptions, WorkdirDiffMode};

let diff = diff_workdir(".", WorkdirDiffMode::Staged, DiffOptions::new())?;
```

## Data pipeline

The library is organized into three stages:

```text
source  →  data  →  query
Find       Parse    Filter
files      & count  & sort
```

The pipeline ends at typed query data. Presentation — tables, headers, display
widths, colors, footer wording — is the caller's job: this library returns
numbers, never display-ready strings.

### Full pipeline example

```rust,ignore
use rustloclib::{
    count_workspace, CountOptions, CountQuerySet,
    Aggregation, LineTypes, Ordering,
};

// Stages 1–2: discover and count
let result = count_workspace(".", CountOptions::new())?;

// Stage 3: query (aggregate, sort; `LineTypes` records which types the
// caller wants to *see* — it never zeroes the underlying counts)
let queryset = CountQuerySet::from_result(
    &result,
    Aggregation::ByCrate,
    LineTypes::everything(),
    Ordering::by_code(),
);

// The query set is the canonical, output-mode-independent response and the
// end of the library: serialize it, or render it however you like.
for item in &queryset.items {
    println!("{}: {} code lines", item.label, item.stats.code);
}
```

## Key types

| Type | Description |
| ------ | ------------- |
| `Locs` | Counts for a single item: `code`, `tests`, `examples`, `docs`, `comments`, `blanks`, `total` |
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

## License

MIT License
