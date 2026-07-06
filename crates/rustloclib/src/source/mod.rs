//! Source discovery: find files to analyze.
//!
//! This module handles the first stage of the pipeline - discovering what
//! files to count. It provides:
//!
//! - **Workspace discovery**: Find crates in a Cargo workspace
//! - **File filtering**: Include/exclude files with glob patterns
//!
//! ## Example
//!
//! ```rust,ignore
//! use rustloclib::source::{WorkspaceInfo, FilterConfig};
//!
//! // Discover workspace structure
//! let workspace = WorkspaceInfo::discover(".")?;
//!
//! // Configure file filters
//! let filter = FilterConfig::new()
//!     .exclude("**/generated/**")?;
//! ```

pub mod filter;
pub mod workspace;

pub use filter::{discover_files, discover_files_in_dirs, FilterConfig};
pub use workspace::{CrateInfo, WorkspaceInfo};
