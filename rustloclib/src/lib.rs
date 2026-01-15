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
//! ## Features
//!
//! - **Rust-aware parsing**: Properly handles #[cfg(test)], #[test] attributes
//! - **Cargo workspace support**: Can filter by crate within a workspace
//! - **Glob filtering**: Filter files/directories with glob patterns
//! - **Git diff support**: Compare LOC changes between commits
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

pub mod counter;
pub mod diff;
pub mod error;
pub mod filter;
pub mod options;
pub mod stats;
pub mod table;
pub mod visitor;
pub mod workspace;

pub use counter::{count_directory, count_file, count_workspace, CountOptions, CountResult};
pub use diff::{
    diff_commits, diff_workdir, CrateDiffStats, DiffOptions, DiffResult, FileChangeType,
    FileDiffStats, LocsDiff, WorkdirDiffMode,
};
pub use error::RustlocError;
pub use filter::FilterConfig;
pub use options::{Aggregation, LineTypes, OrderBy, OrderDirection, Ordering};
pub use stats::{CrateStats, FileStats, Locs, ModuleStats};
pub use table::{LOCTable, TableRow};
pub use visitor::{gather_stats, gather_stats_for_path, VisitorContext};
pub use workspace::{CrateInfo, WorkspaceInfo};

/// Result type for rustloclib operations
pub type Result<T> = std::result::Result<T, RustlocError>;
