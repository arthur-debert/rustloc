# rustloc

A Rust-aware lines-of-code counter. Unlike generic LOC tools, rustloc understands that Rust tests live alongside production code and correctly separates them — even in the same file.

[![CI](https://github.com/arthur-debert/rustloc/actions/workflows/ci.yml/badge.svg)](https://github.com/arthur-debert/rustloc/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/rustloc.svg)](https://crates.io/crates/rustloc)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

![Rust Aware](https://raw.githubusercontent.com/arthur-debert/rustloc/main/assets/output-by-module.png)

## Features

- **Line types:** code, tests, examples, docs, comments, blanks
- **Grouping:** by crate, module, or file
- **Sorting and slicing:** sort by any column, take the top N
- **Filtering:** include only rows matching a threshold (`--code-gte 1000`, `--tests-lt 500`, …)
- **Diffs:** between any two commits, against HEAD, or the working tree
- **Output:** terminal tables, JSON, YAML, XML, CSV — pipeable to a file

## Installation

From crates.io:

```bash
cargo install rustloc
```

Pre-built binaries (signed and notarized on macOS, `.deb` on Linux) are on the [releases page](https://github.com/arthur-debert/rustloc/releases). A Homebrew formula is published to [arthur-debert/homebrew-tools](https://github.com/arthur-debert/homebrew-tools):

```bash
brew install arthur-debert/tools/rustloc
```

## Usage

### Counting

```bash
rustloc                              # totals for current directory
rustloc --by-crate                   # breakdown by crate
rustloc --by-module                  # breakdown by module
rustloc --by-file                    # breakdown by file
rustloc -t code,tests                # only show selected line types
rustloc -c my-lib                    # restrict to a specific crate
rustloc -i "src/**/*.rs"             # include glob
rustloc -e "**/generated/**"         # exclude glob
```

![by-file output](https://raw.githubusercontent.com/arthur-debert/rustloc/main/assets/output-by-file.png)

### Sorting and top-N

```bash
rustloc --by-file -o code            # sort files by code lines (descending)
rustloc --by-file -o +label          # sort by name (ascending)
rustloc --by-file -o -code --top 10  # the 10 largest files by code
```

Sortable fields: `label`, `code`, `tests`, `examples`, `docs`, `comments`, `blanks`, `total`. Prefix with `-` for descending, `+` for ascending; numeric fields default to descending and `label` defaults to ascending.

### Filtering by threshold

Drop rows that don't meet a numeric criterion. The pattern is `--<field>-<op> <N>`, and multiple filters AND together:

```bash
rustloc --by-file --code-gte 1000              # files with ≥ 1000 code lines
rustloc --by-file --tests-eq 0                 # files with no tests
rustloc --by-file --code-gte 500 --tests-lt 50 # both conditions
rustloc --by-file --code-gte 1000 --top 5      # filter first, then take top 5
```

Fields: `code`, `tests`, `examples`, `docs`, `comments`, `blanks`, `total`.
Operators: `gt`, `gte`, `eq`, `ne`, `lt`, `lte`.

The total row always reflects the full data set; the footer shows how many rows were filtered or truncated (e.g. `Total (5 of 247 files)`).

### Diffs

```bash
rustloc diff                         # working tree vs HEAD
rustloc diff --staged                # staged changes only (alias: --cached)
rustloc diff HEAD~5..HEAD            # between two commits
rustloc diff v1.0.0..v2.0.0          # between two tags
rustloc diff main feature --by-file  # two-arg form, per-file breakdown
rustloc diff main...feature          # from the merge base of main and feature
```

Revspec syntax mirrors `git diff` / `git rev-parse`: tags (annotated or lightweight), branches, short hashes, `HEAD~N`, ranges (`a..b`), and merge-base ranges (`a...b`) all work. A single rev is diffed against HEAD; tag objects are peeled to their target commit automatically.

The same `--by-*`, `-o`, `--top`, `-t`, and filter flags work on `diff` results — diff filters operate on the net change.

![diff output](https://raw.githubusercontent.com/arthur-debert/rustloc/main/assets/output-diff.png)

### Output formats

```bash
rustloc --output json
rustloc --output csv
rustloc --output yaml
rustloc --output xml
rustloc --output csv --output-file-path report.csv
```

JSON output preserves the full result structure (totals, breakdowns, applied filters, total-vs-shown counts) so it round-trips through scripts cleanly.

## Library

rustloc is also available as a library crate, [`rustloclib`](https://docs.rs/rustloclib). Quick taste:

```rust
use rustloclib::{count_workspace, CountOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let result = count_workspace(".", CountOptions::new())?;
    println!("Production code: {}", result.total.code);
    println!("Test code:       {}", result.total.tests);
    Ok(())
}
```

The library exposes a four-stage pipeline (source → data → query → output) that the CLI is built on top of. See the [API documentation](https://docs.rs/rustloclib) for the full surface area, including diffs, filtering predicates, and table rendering.

## How it works

rustloc uses a token-based parser with single-character lookahead to analyze Rust source files. It recognizes:

- Test blocks via `#[test]` and `#[cfg(test)]` attributes
- File context from paths (`tests/`, `examples/` directories)
- All Rust comment styles including doc comments
- Raw string literals that may contain comment-like syntax
- Nested block comments

The parsing logic is adapted from [cargo-warloc](https://github.com/Maximkaaa/cargo-warloc) by Maxim Gritsenko.

## License

MIT License — see [LICENSE](LICENSE) for details.
