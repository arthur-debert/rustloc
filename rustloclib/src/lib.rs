//! # rustloclib
//!
//! A Rust-aware lines of code counter library that separates code, tests, comments,
//! and blank lines.
//!
//! ## Overview
//!
//! Unlike generic LOC counters (tokei, cloc, scc), this library understands Rust's
//! unique structure where tests live alongside production code. It uses AST-aware
//! parsing to distinguish:
//!
//! - **Code**: Production code lines
//! - **Tests**: Code within `#[test]` or `#[cfg(test)]` blocks, or in `tests/` directories
//! - **Examples**: Code in `examples/` directories
//! - **Comments**: Regular comments (`//`, `/* */`)
//! - **Doc comments**: Documentation comments (`///`, `//!`, `/** */`, `/*! */`)
//! - **Blanks**: Whitespace-only lines
//!
//! ## Features
//!
//! - **Rust-aware parsing**: Properly handles `#[cfg(test)]`, `#[test]` attributes
//! - **Cargo workspace support**: Can filter by crate within a workspace
//! - **Glob filtering**: Filter files/directories with glob patterns
//! - **Pure Rust data types**: Returns structured data, no I/O side effects
//!
//! ## Origins
//!
//! The parsing logic is adapted from [cargo-warloc](https://github.com/Maximkaaa/cargo-warloc)
//! by Maxim Gritsenko. We thank the original author for the excellent parsing implementation.
//! cargo-warloc is MIT licensed.
//!
//! ## Example
//!
//! ```rust,ignore
//! use rustloclib::{count_file, count_workspace, LocStats};
//!
//! // Count a single file
//! let stats = count_file("src/main.rs")?;
//!
//! // Count an entire workspace with filters
//! let stats = count_workspace(".", Some(&["my-crate"]), None)?;
//! ```

pub mod error;
pub mod stats;
pub mod visitor;
// pub mod workspace; // TODO: Add workspace support
// pub mod filter;    // TODO: Add glob filtering

pub use error::RustlocError;
pub use stats::{CrateStats, FileStats, LocStats, Locs};
pub use visitor::{parse_file, parse_string, VisitorContext};

/// Result type for rustloclib operations
pub type Result<T> = std::result::Result<T, RustlocError>;
