//! # rustloclib
//!
//! A Rust-aware lines of code counter library with a simple, flat data model.
//!
//! ## Overview
//!
//! Unlike generic LOC counters (tokei, cloc, scc), this library has semantic
//! backends for languages where tests can live alongside production code. Rust is
//! enabled by default; Python, TypeScript, and generic source backends can be
//! selected through [`FilterConfig`]. It categorizes lines into one of 6 types:
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
//! - [`diff_revspec`]: Compare commits via a git revspec string, returns [`DiffResult`]
//!
//! ### Stage 3: Query Processing ([`query`])
//!
//! Filter, aggregate, sort, and slice the collected data:
//! - [`CountQuerySet`] / [`DiffQuerySet`]: Processed data ready for display
//! - [`Aggregation`]: Total, ByCrate, ByModule, ByFile
//! - [`LineTypes`]: Which line types to include in output
//! - [`Ordering`]: How to sort results
//! - [`Predicate`] (built from [`Field`] + [`Op`]): Threshold filters,
//!   chained via `CountQuerySet::filter(&[Predicate])`
//! - `CountQuerySet::top(N)`: Truncate to the first N rows after sorting
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
//!     Aggregation, Field, LineTypes, Op, Ordering, Predicate,
//! };
//!
//! // Stage 1-2: Discover and collect
//! let result = count_workspace(".", CountOptions::new())?;
//!
//! // Stage 3: Query — aggregate, sort, then filter and slice. Chain
//! // `.filter(...)` and `.top(...)` for the equivalent of the CLI's
//! // `--code-gte 1000 --top 10`.
//! let queryset = CountQuerySet::from_result(
//!     &result,
//!     Aggregation::ByFile,
//!     LineTypes::everything(),
//!     Ordering::by_code(),
//! )
//! .filter(&[Predicate::new(Field::Code, Op::Gte, 1000)])
//! .top(10);
//!
//! // Stage 4: Format for output
//! let table = LOCTable::from_count_queryset(&queryset);
//! ```
//!
//! [`DiffQuerySet`] mirrors [`CountQuerySet`] for the diff side; both
//! support the same `.filter()` / `.top()` chain. Diff filters operate
//! on the net change (added − removed) per row.
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
    available_languages, count_directory, count_file, count_workspace, default_languages,
    diff_revspec, diff_workdir, gather_stats, gather_stats_for_path, CountOptions, CountResult,
    CrateDiffStats, CrateStats, DiffOptions, DiffResult, FileChangeType, FileDiffStats, FileStats,
    LanguageName, LanguageSelection, Locs, LocsDiff, ModuleStats, VisitorContext, WorkdirDiffMode,
};
pub use error::RustlocError;
pub use output::{LOCTable, TableRow};
pub use query::{
    Aggregation, CountQuerySet, DiffQuerySet, Field, LineTypes, Op, OrderBy, OrderDirection,
    Ordering, Predicate, QueryItem,
};
pub use source::{CrateInfo, FilterConfig, WorkspaceInfo};

/// Result type for rustloclib operations
pub type Result<T> = std::result::Result<T, RustlocError>;
