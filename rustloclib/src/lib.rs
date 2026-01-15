//! # rustloclib
//!
//! A Rust-aware lines of code counter library with a simple, flat data model.
//!
//! ## Overview
//!
//! Unlike generic LOC counters (tokei, cloc, scc), this library understands Rust's
//! unique structure where tests live alongside production code. It uses AST-aware
//! parsing to categorize lines into one of 6 types:
//!
//! - **code**: Production code logic lines (in src/, not in test blocks)
//! - **tests**: Test code logic lines (#[test], #[cfg(test)], tests/)
//! - **examples**: Example code logic lines (examples/)
//! - **docs**: Documentation comments (///, //!, /** */, /*! */)
//! - **comments**: Regular comments (//, /* */)
//! - **blanks**: Blank/whitespace-only lines
//!
//! The key insight: only actual code lines need context (code/tests/examples).
//! A blank is a blank, a comment is a comment - where they appear doesn't matter.
//!
//! ## Data Pipeline
//!
//! The library is organized into four stages that form a clear data pipeline:
//!
//! ```text
//! ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
//! │  source  │ -> │   data   │ -> │  query   │ -> │  output  │
//! └──────────┘    └──────────┘    └──────────┘    └──────────┘
//!   Discover       Parse &         Filter,         Format
//!   files          collect         sort            strings
//! ```
//!
//! ### Stage 1: Source Discovery ([`source`])
//!
//! Find what files to analyze:
//! - [`WorkspaceInfo`]: Discover Cargo workspace structure
//! - [`FilterConfig`]: Include/exclude files with glob patterns
//!
//! ### Stage 2: Data Collection ([`data`])
//!
//! Parse files and collect statistics:
//! - [`gather_stats`]: Parse a single file into [`Locs`]
//! - [`count_workspace`]: Count all files, returns [`CountResult`]
//! - [`diff_commits`]: Compare commits, returns [`DiffResult`]
//!
//! ### Stage 3: Query Processing ([`query`])
//!
//! Filter, aggregate, and sort the collected data:
//! - [`CountQuerySet`]: Processed count data ready for display
//! - [`LineTypes`]: Which line types to include
//! - [`Ordering`]: How to sort results
//!
//! ### Stage 4: Output Formatting ([`output`])
//!
//! Format data for presentation:
//! - [`LOCTable`]: Table with headers, rows, footer (all strings)
//!
//! ## Example
//!
//! ```rust
//! use rustloclib::{count_file, count_workspace, CountOptions, FilterConfig};
//! use std::fs;
//! use tempfile::tempdir;
//!
//! // Set up a temporary project
//! let dir = tempdir().unwrap();
//! fs::write(dir.path().join("Cargo.toml"), r#"
//! [package]
//! name = "my-lib"
//! version = "0.1.0"
//! edition = "2021"
//! "#).unwrap();
//! fs::create_dir(dir.path().join("src")).unwrap();
//! let file_path = dir.path().join("src/lib.rs");
//! fs::write(&file_path, "pub fn hello() {\n    println!(\"Hi\");\n}\n").unwrap();
//!
//! // Count a single file
//! let stats = count_file(&file_path).unwrap();
//! assert_eq!(stats.code, 3);  // 3 lines of production code
//!
//! // Count an entire workspace
//! let result = count_workspace(dir.path(), CountOptions::new()).unwrap();
//! assert!(result.total.code >= 1);
//!
//! // Count with filtering
//! let filter = FilterConfig::new().exclude("**/generated/**").unwrap();
//! let result = count_workspace(dir.path(), CountOptions::new().filter(filter)).unwrap();
//! ```
//!
//! ## Full Pipeline Example
//!
//! ```rust,ignore
//! use rustloclib::{
//!     count_workspace, CountOptions, CountQuerySet, LOCTable,
//!     Aggregation, LineTypes, Ordering,
//! };
//!
//! // Stage 1-2: Discover and collect
//! let result = count_workspace(".", CountOptions::new())?;
//!
//! // Stage 3: Query (filter, aggregate, sort)
//! let queryset = CountQuerySet::from_result(
//!     &result,
//!     Aggregation::ByCrate,
//!     LineTypes::everything(),
//!     Ordering::by_code(),
//! );
//!
//! // Stage 4: Format for output
//! let table = LOCTable::from_count_queryset(&queryset);
//! ```
//!
//! ## Origins
//!
//! The parsing logic is adapted from [cargo-warloc](https://github.com/Maximkaaa/cargo-warloc)
//! by Maxim Gritsenko. We thank the original author for the excellent parsing implementation.
//! cargo-warloc is MIT licensed.

// Pipeline modules (in order)
pub mod data;
pub mod output;
pub mod query;
pub mod source;

// Infrastructure
pub mod error;

// Re-export all public types at crate root for convenience
pub use data::{
    count_directory, count_file, count_workspace, diff_commits, diff_workdir, gather_stats,
    gather_stats_for_path, CountOptions, CountResult, CrateDiffStats, CrateStats, DiffOptions,
    DiffResult, FileChangeType, FileDiffStats, FileStats, Locs, LocsDiff, ModuleStats,
    VisitorContext, WorkdirDiffMode,
};
pub use error::RustlocError;
pub use output::{LOCTable, TableRow};
pub use query::{
    Aggregation, CountQuerySet, DiffQuerySet, LineTypes, OrderBy, OrderDirection, Ordering,
    QueryItem,
};
pub use source::{CrateInfo, FilterConfig, WorkspaceInfo};

/// Result type for rustloclib operations
pub type Result<T> = std::result::Result<T, RustlocError>;
