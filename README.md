# rustloc

A language-aware lines-of-code counter with deep Rust, Python, and TypeScript classification. Unlike generic LOC tools, rustloc understands when tests live alongside production code and separates them correctly — even in the same file.

[![CI](https://github.com/arthur-debert/rustloc/actions/workflows/ci.yml/badge.svg)](https://github.com/arthur-debert/rustloc/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/rustloc.svg)](https://crates.io/crates/rustloc)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

![Language-aware output](https://raw.githubusercontent.com/arthur-debert/rustloc/main/assets/output-by-module.png)

## Features

- **Line types:** code, tests, examples, docs, comments, blanks
- **Language backends:** Rust by default; opt into Python, TypeScript, or generic source counting with `--lang`
- **Grouping:** by crate, module, or file
- **Sorting and slicing:** sort by any column, take the top N
- **Filtering:** include only rows matching a threshold (`--code-gte 1000`, `--tests-lt 500`, …)
- **Diffs:** between any two commits, against HEAD, or the working tree, classified by changed lines
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
rustloc --lang typescript            # analyze TypeScript files only
rustloc --lang rust,typescript       # analyze Rust and TypeScript files
rustloc -c my-lib                    # restrict to a specific crate
rustloc --lang python                # analyze Python files only
rustloc --lang rust,python           # analyze Rust and Python files
rustloc -i "src/**/*.rs"             # include glob
rustloc -e "**/generated/**"         # exclude glob
```

![by-file output](https://raw.githubusercontent.com/arthur-debert/rustloc/main/assets/output-by-file.png)

### Languages

By default, rustloc analyzes Rust files. Additional backends are available but opt-in:

```bash
rustloc --lang rust                  # default
rustloc --lang python                # Python only
rustloc --lang typescript            # TypeScript and TSX only
rustloc --lang rust,python           # Rust and Python
rustloc --lang rust,typescript       # Rust and TypeScript
rustloc --lang all                   # all available backend groups
```

Rust and Python use semantic backends that can classify tests inside production files. The TypeScript backend uses Oxc parser comment spans to classify JSDoc docs, comments, blanks, and path-level test/example files. The generic backend is file-level only: it recognizes common source extensions and uses path conventions such as `tests/` and `examples/` for context.

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

The same `--by-*`, `-o`, `--top`, `-t`, `--lang`, and filter flags work on `diff` results — diff filters operate on the net change.

Diffs use the active language selection. Files outside that selection are not analyzed semantically; their added and removed physical lines are reported separately as `Skipped changes` so branch sanity checks still show that something changed outside the counted language set.

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

## GitHub Action

`arthur-debert/rustloc` ships a composite action that posts a rustloc diff as a sticky comment on a pull request. Add it to a workflow:

```yaml
name: rustloc

on:
  pull_request:

permissions:
  contents: read
  pull-requests: write

jobs:
  rustloc-diff:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0          # required so base...head is reachable
      - uses: arthur-debert/rustloc@v0          # or pin to a tag like @v0.16.0
        with:
          args: --by-file         # any flags accepted by `rustloc diff`
```

The action downloads the prebuilt binary from the [releases page](https://github.com/arthur-debert/rustloc/releases), runs `rustloc diff <base>...<head>`, and posts the text output in a fenced block. Subsequent runs on the same PR edit the existing comment in place (matched by a hidden `<!-- rustloc-diff -->` marker).

Inputs:

| Input          | Default                                     | Description                                                          |
|----------------|---------------------------------------------|----------------------------------------------------------------------|
| `version`      | `latest`                                    | rustloc version to install (e.g. `0.16.0`), or `latest`.             |
| `base`         | `${{ github.event.pull_request.base.sha }}` | Base git ref to diff from.                                           |
| `head`         | `${{ github.event.pull_request.head.sha }}` | Head git ref to diff to.                                             |
| `args`         | `--by-file`                                 | Extra args passed to `rustloc diff`.                                 |
| `comment`      | `true`                                      | Set to `false` to skip posting; the body is still in `outputs.body`. |
| `github-token` | `${{ github.token }}`                       | Token used to post the comment (needs `pull-requests: write`).       |

Supported runners: `ubuntu-latest` (x86_64 + arm64) and `macos-latest` (arm64).

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

The reusable library discovers and counts source, computes diffs, and returns
canonical typed query sets. It deliberately stops before presentation: table
layout, wording, colours, and output writing belong to the CLI's Standout
layer. See the [API documentation](https://docs.rs/rustloclib) for the data,
diff, filtering, ordering, and aggregation APIs.

## Language Backends

By default, rustloc analyzes Rust files. Additional backends are opt-in:

```bash
rustloc --lang rust                  # default
rustloc --lang python                # Python only
rustloc --lang typescript            # TypeScript and TSX only
rustloc --lang rust,typescript       # Rust and TypeScript
rustloc --lang all                   # all available backend groups
```

Rust and Python use semantic backends that can classify tests inside production files. The TypeScript backend uses Oxc parser comment spans to classify JSDoc docs, comments, blanks, and path-level test/example files. The generic backend is file-level only: it recognizes common source extensions and uses path conventions such as `tests/` and `examples/` for context.

## How it works

rustloc routes files through language backends. Rust is enabled by default; Python, TypeScript, and generic source counting can be selected with `--lang`.

The Rust backend uses a token-based parser with single-character lookahead. It recognizes:

- Test blocks via `#[test]` and `#[cfg(test)]` attributes
- File context from paths (`tests/`, `examples/` directories)
- All Rust comment styles including doc comments
- Raw string literals that may contain comment-like syntax
- Nested block comments

The Python backend uses Ruff's parser and syntax ranges to classify pytest functions, unittest classes, docstrings, comments, blanks, and path-level test/example files. The TypeScript backend uses Oxc parser comment spans for JSDoc and regular comments, with path-level test/example classification. The generic backend provides file-level classification for common source extensions when selected.

The parsing logic is adapted from [cargo-warloc](https://github.com/Maximkaaa/cargo-warloc) by Maxim Gritsenko.

## License

MIT License — see [LICENSE](LICENSE) for details.
