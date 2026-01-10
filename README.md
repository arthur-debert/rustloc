# rustloc

A Rust-aware lines of code counter that separates production code from tests.

[![CI](https://github.com/arthur-debert/rustloc/actions/workflows/ci.yml/badge.svg)](https://github.com/arthur-debert/rustloc/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/rustloc.svg)](https://crates.io/crates/rustloc)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Why rustloc?

Unlike generic LOC counters (tokei, cloc, scc), rustloc understands Rust's unique structure where tests live alongside production code. It parses Rust syntax to accurately separate:

- **Code**: Production code lines
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

Columns show LOC totals per context (Code, Tests, Examples):

```
        Code       Tests    Examples       Total     Files
----------------------------------------------------------
        1801         807          51        2659        10
```

With `--by-crate`:

```
Crate                                        Code       Tests    Examples       Total
--------------------------------------------------------------------------------------
my-lib                                       1256         500          45        1801
my-cli                                        545         307           6         858
--------------------------------------------------------------------------------------
Total (10 files)                             1801         807          51        2659
```

Filter columns with `--type`:

```bash
rustloc . --type tests          # Only show Tests column
rustloc . --type code,tests     # Show Code and Tests columns
```

### JSON

```bash
rustloc . --output json
```

```json
{
  "total": {
    "file_count": 10,
    "code": { "logic": 1256, "blank": 224, "docs": 296, "comments": 25 },
    "tests": { "logic": 586, "blank": 113, "docs": 78, "comments": 30 },
    "examples": { "logic": 46, "blank": 4, "docs": 1, "comments": 0 }
  }
}
```

### CSV

```bash
rustloc . --output csv
```

```csv
name,code,tests,examples,total,files
"total",1801,807,51,2659,10
```

With `--by-crate`:

```csv
name,code,tests,examples,total,files
"my-lib",1256,500,45,1801,6
"my-cli",545,307,6,858,4
"total",1801,807,51,2659,10
```

## Library Usage

rustloc is also available as a library (`rustloclib`) for programmatic use:

```rust
use rustloclib::{count_workspace, CountOptions, FilterConfig};

// Count all crates in a workspace
let result = count_workspace(".", CountOptions::new())?;
println!("Total logic lines: {}", result.total.logic());

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
