# rustloc

A CLI tool for counting lines of code in Rust projects with test/code separation.

[![Crates.io](https://img.shields.io/crates/v/rustloc.svg)](https://crates.io/crates/rustloc)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Installation

```bash
cargo install rustloc
```

Or download pre-built binaries from [GitHub Releases](https://github.com/arthur-debert/rustloc/releases).

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

# Filter files with glob patterns
rustloc . --exclude "**/generated/**" --include "**/src/**"
```

## Options

```
Usage: rustloc [OPTIONS] [PATH]

Arguments:
  [PATH]  Path to analyze [default: .]

Options:
  -c, --crate <CRATES>     Filter by crate name (can be repeated)
  -i, --include <INCLUDE>  Include files matching glob pattern (can be repeated)
  -e, --exclude <EXCLUDE>  Exclude files matching glob pattern (can be repeated)
  -f, --format <FORMAT>    Output format [default: table] [values: table, json, csv]
      --per-crate          Show per-crate breakdown
      --per-file           Show per-file breakdown
  -h, --help               Print help
  -V, --version            Print version
```

## Output Formats

### Table (default)

```
File count: 10
Type         |         Code |        Blank | Doc comments |     Comments |        Total
-------------------------------------------------------------------------------
Main         |         1256 |          224 |          296 |           25 |         1801
Tests        |          586 |          113 |           78 |           30 |          807
Examples     |           46 |            4 |            1 |            0 |           51
-------------------------------------------------------------------------------
             |         1888 |          341 |          375 |           55 |         2659
```

### JSON

Structured output suitable for programmatic consumption.

### CSV

Machine-readable format for spreadsheet import or data analysis.

## Library

For programmatic use, see [rustloclib](https://crates.io/crates/rustloclib).

## License

MIT License
