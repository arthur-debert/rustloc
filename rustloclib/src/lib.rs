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
//! - **Main**: Production code (main/src code)
//! - **Tests**: Code within `#[test]` or `#[cfg(test)]` blocks, or in `tests/` directories
//! - **Examples**: Code in `examples/` directories
//! - **Comments**: Regular comments (`//`, `/* */`)
//! - **Docs**: Documentation comments (`///`, `//!`, `/** */`, `/*! */`)
//! - **Blank**: Whitespace-only lines
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
//! use rustloclib::{count_file, count_workspace, CountOptions, FilterConfig};
//!
//! // Count a single file
//! let stats = count_file("src/main.rs")?;
//!
//! // Count an entire workspace
//! let result = count_workspace(".", CountOptions::new())?;
//!
//! // Count specific crates with filtering
//! let filter = FilterConfig::new().exclude("**/generated/**")?;
//! let result = count_workspace(".", CountOptions::new()
//!     .crates(vec!["my-lib".to_string()])
//!     .filter(filter))?;
//! ```

pub mod counter;
pub mod diff;
pub mod error;
pub mod filter;
pub mod options;
pub mod stats;
pub mod visitor;
pub mod workspace;

pub use counter::{count_directory, count_file, count_workspace, CountOptions, CountResult};
pub use diff::{
    diff_commits, CrateDiffStats, DiffOptions, DiffResult, FileChangeType, FileDiffStats,
    LocStatsDiff, LocsDiff,
};
pub use error::RustlocError;
pub use filter::FilterConfig;
pub use options::{Aggregation, Contexts};
pub use stats::{CrateStats, FileStats, LocStats, Locs, ModuleStats};
pub use visitor::{parse_file, parse_string, VisitorContext};
pub use workspace::{CrateInfo, WorkspaceInfo};

/// Result type for rustloclib operations
pub type Result<T> = std::result::Result<T, RustlocError>;
