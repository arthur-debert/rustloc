# rustloclib

A Rust-aware lines of code counter library that separates code, tests, comments, and blanks.

[![Crates.io](https://img.shields.io/crates/v/rustloclib.svg)](https://crates.io/crates/rustloclib)
[![Documentation](https://docs.rs/rustloclib/badge.svg)](https://docs.rs/rustloclib)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Overview

Unlike generic LOC counters, this library understands Rust's unique structure where tests live alongside production code. It uses syntax-aware parsing to distinguish:

- **Code**: Production code lines
- **Tests**: Code within `#[test]` or `#[cfg(test)]` blocks, or in `tests/` directories
- **Examples**: Code in `examples/` directories
- **Comments**: Regular comments (`//`, `/* */`)
- **Doc comments**: Documentation comments (`///`, `//!`, `/** */`, `/*! */`)
- **Blanks**: Whitespace-only lines

## Features

- **Rust-aware parsing**: Properly handles `#[cfg(test)]`, `#[test]` attributes
- **Cargo workspace support**: Discover and filter crates within a workspace
- **Glob filtering**: Include/exclude files with glob patterns
- **Pure Rust data types**: Returns structured data for easy integration

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
rustloclib = "0.1"
```

## Usage

### Count a workspace

```rust
use rustloclib::{count_workspace, CountOptions};

let result = count_workspace(".", CountOptions::new())?;

println!("Files: {}", result.total.file_count);
println!("Main code: {}", result.total.main.code);
println!("Test code: {}", result.total.tests.code);
println!("Total: {}", result.total.total());
```

### Count specific crates

```rust
use rustloclib::{count_workspace, CountOptions};

let result = count_workspace(".", CountOptions::new()
    .crates(vec!["my-lib".to_string(), "my-cli".to_string()]))?;
```

### Filter files with globs

```rust
use rustloclib::{count_workspace, CountOptions, FilterConfig};

let filter = FilterConfig::new()
    .exclude("**/generated/**")?
    .exclude("**/vendor/**")?;

let result = count_workspace(".", CountOptions::new().filter(filter))?;
```

### Count a single file

```rust
use rustloclib::count_file;

let stats = count_file("src/main.rs")?;
println!("Code: {}, Docs: {}", stats.main.code, stats.main.docs);
```

### Parse from a string (for testing)

```rust
use rustloclib::{parse_string, VisitorContext};

let source = r#"
fn main() {
    println!("Hello");
}

#[test]
fn test_main() {
    assert!(true);
}
"#;

let stats = parse_string(source, VisitorContext::Main);
assert_eq!(stats.main.code, 3);  // fn, println, }
assert_eq!(stats.tests.code, 4); // #[test], fn, assert, }
```

## Data Structures

### LocStats

Aggregated statistics for a collection of files:

```rust
pub struct LocStats {
    pub file_count: u64,
    pub main: Locs,      // Production code
    pub tests: Locs,     // Test code
    pub examples: Locs,  // Example code
}
```

### Locs

Line counts for a single context:

```rust
pub struct Locs {
    pub blanks: u64,    // Whitespace-only lines
    pub code: u64,      // Code lines
    pub docs: u64,      // Doc comment lines
    pub comments: u64,  // Regular comment lines
}
```

### CountResult

Result from counting operations:

```rust
pub struct CountResult {
    pub total: LocStats,           // Aggregated stats
    pub crates: Vec<CrateStats>,   // Per-crate breakdown
    pub files: Vec<FileStats>,     // Per-file breakdown (if requested)
}
```

## Acknowledgments

The parsing logic is adapted from [cargo-warloc](https://github.com/Maximkaaa/cargo-warloc) by Maxim Gritsenko (MIT licensed).

## License

MIT License
