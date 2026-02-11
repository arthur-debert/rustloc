//! Data collection: parse files and collect statistics.
//!
//! This module handles the second stage of the pipeline - parsing Rust source
//! files and collecting LOC statistics. It provides:
//!
//! - **Parsing**: AST-aware visitor that categorizes lines
//! - **Statistics**: Core data structures (`Locs`, `FileStats`, etc.)
//! - **Counting**: High-level API (`count_workspace`, `count_file`)
//! - **Diffing**: Git diff analysis (`diff_commits`, `diff_workdir`)
//!
//! ## Example
//!
//! ```rust,ignore
//! use rustloclib::data::{count_workspace, CountOptions};
//!
//! let result = count_workspace(".", CountOptions::new())?;
//! println!("Total code lines: {}", result.total.code);
//! ```

pub mod counter;
pub mod diff;
pub mod stats;
pub mod visitor;

pub use counter::{
    compute_module_name, count_directory, count_file, count_workspace, CountOptions, CountResult,
};
pub use diff::{
    diff_commits, diff_workdir, CrateDiffStats, DiffOptions, DiffResult, FileChangeType,
    FileDiffStats, LocsDiff, WorkdirDiffMode,
};
pub use stats::{CrateStats, FileStats, Locs, ModuleStats};
pub use visitor::{gather_stats, gather_stats_for_path, VisitorContext};
