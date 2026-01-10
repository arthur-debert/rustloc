# rustloc

A Rust-aware lines of code counter that separates production code from tests.

[![CI](https://github.com/arthur-debert/rustloc/actions/workflows/ci.yml/badge.svg)](https://github.com/arthur-debert/rustloc/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/rustloc.svg)](https://crates.io/crates/rustloc)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Why rustloc?

Unlike generic LOC counters (tokei, cloc, scc), rustloc understands Rust's unique structure where tests live alongside production code. It parses Rust syntax to accurately separate:

- **Main**: Production code lines
- **Tests**: Code within `#[test]` or `#[cfg(test)]` blocks
- **Examples**: Code in `examples/` directories
- **Docs**: Documentation comments (`///`, `//!`, `/** */`, `/*! */`)
- **Comments**: Regular comments (`//`, `/* */`)
- **Blank**: Whitespace-only lines

## Installation

### From crates.io

```bash
cargo install rustloc
```

### From source

```bash
git clone https://github.com/arthur-debert/rustloc
cd rustloc
cargo install --path rustloc
```

### Pre-built binaries

Download from [GitHub Releases](https://github.com/arthur-debert/rustloc/releases).

## Usage

```bash
# Analyze current directory
rustloc .

# Analyze specific path
rustloc /path/to/rust/project

# Filter to specific crates in a workspace
rustloc . --crate my-lib --crate my-cli

# Output as JSON
rustloc . --format json

# Output as CSV
rustloc . --format csv

# Show per-crate breakdown
rustloc . --per-crate

# Show per-file breakdown
rustloc . --per-file

# Exclude files matching glob pattern
rustloc . --exclude "**/generated/**"

# Include only files matching glob pattern
rustloc . --include "**/src/**"
```

## Output Formats

### Table (default)

```
File count: 10
Context      |         Code |        Blank |         Docs |     Comments |        Total
-------------------------------------------------------------------------------
Main         |         1256 |          224 |          296 |           25 |         1801
Tests        |          586 |          113 |           78 |           30 |          807
Examples     |           46 |            4 |            1 |            0 |           51
-------------------------------------------------------------------------------
             |         1888 |          341 |          375 |           55 |         2659
```

### JSON

```bash
rustloc . --format json
```

```json
{
  "file_count": 10,
  "totals": {
    "main": { "code": 1256, "blank": 224, "docs": 296, "comments": 25 },
    "tests": { "code": 586, "blank": 113, "docs": 78, "comments": 30 },
    "examples": { "code": 46, "blank": 4, "docs": 1, "comments": 0 }
  }
}
```

### CSV

```bash
rustloc . --format csv
```

```csv
type,name,code,blank,docs,comments,total
main,"total",1256,224,296,25,1801
tests,"total",586,113,78,30,807
examples,"total",46,4,1,0,51
total,"total",1888,341,375,55,2659
```

## Library Usage

rustloc is also available as a library (`rustloclib`) for programmatic use:

```rust
use rustloclib::{count_workspace, CountOptions, FilterConfig};

// Count all crates in a workspace
let result = count_workspace(".", CountOptions::new())?;
println!("Total code: {}", result.total.code());

// Count specific crates with filtering
let filter = FilterConfig::new().exclude("**/generated/**")?;
let result = count_workspace(".", CountOptions::new()
    .crates(vec!["my-lib".to_string()])
    .filter(filter))?;
```

See [rustloclib documentation](https://docs.rs/rustloclib) for full API details.

## How it works

rustloc uses a token-based parser with single-character lookahead to analyze Rust source files. It recognizes:

- Test blocks via `#[test]` and `#[cfg(test)]` attributes
- File context from paths (`tests/`, `examples/` directories)
- All Rust comment styles including doc comments
- Raw string literals that may contain comment-like syntax
- Nested block comments

The parsing logic is adapted from [cargo-warloc](https://github.com/Maximkaaa/cargo-warloc) by Maxim Gritsenko.

## Project Structure

```
rustloc/
├── rustloclib/     # Core library with parsing and counting logic
└── rustloc/        # CLI binary
```

## License

MIT License - see [LICENSE](LICENSE) for details.

## Acknowledgments

- [cargo-warloc](https://github.com/Maximkaaa/cargo-warloc) by Maxim Gritsenko for the original parsing implementation
