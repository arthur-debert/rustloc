//! High-level LOC counting API.
//!
//! This module provides the main entry points for counting lines of code
//! in Rust projects, with support for workspace filtering and glob patterns.

use std::path::Path;

use crate::error::RustlocError;
use crate::filter::{discover_files, discover_files_in_dirs, FilterConfig};
use crate::stats::{CrateStats, FileStats, LocStats};
use crate::visitor::parse_file;
use crate::workspace::{CrateInfo, WorkspaceInfo};
use crate::Result;

/// Options for counting LOC.
#[derive(Debug, Clone, Default)]
pub struct CountOptions {
    /// Filter by crate names (empty = all crates)
    pub crate_filter: Vec<String>,
    /// File filter configuration
    pub file_filter: FilterConfig,
    /// Whether to include per-file statistics
    pub per_file_stats: bool,
}

impl CountOptions {
    /// Create new default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter to specific crates.
    pub fn crates(mut self, names: Vec<String>) -> Self {
        self.crate_filter = names;
        self
    }

    /// Set file filter.
    pub fn filter(mut self, filter: FilterConfig) -> Self {
        self.file_filter = filter;
        self
    }

    /// Include per-file statistics in results.
    pub fn with_file_stats(mut self) -> Self {
        self.per_file_stats = true;
        self
    }
}

/// Result of counting LOC in a workspace or directory.
#[derive(Debug, Clone, Default)]
pub struct CountResult {
    /// Aggregated statistics across all files
    pub total: LocStats,
    /// Per-crate statistics (if workspace)
    pub crates: Vec<CrateStats>,
    /// Per-file statistics (if requested)
    pub files: Vec<FileStats>,
}

impl CountResult {
    /// Create a new empty result.
    pub fn new() -> Self {
        Self::default()
    }
}

/// Count LOC in a Cargo workspace.
///
/// This is the main entry point for analyzing a Rust project. It:
/// 1. Discovers the workspace structure
/// 2. Optionally filters to specific crates
/// 3. Applies glob filters to files
/// 4. Parses all matching files and aggregates statistics
///
/// # Example
///
/// ```rust,ignore
/// use rustloclib::{count_workspace, CountOptions, FilterConfig};
///
/// // Count all crates in the workspace
/// let result = count_workspace(".", CountOptions::new())?;
///
/// // Count specific crates only
/// let result = count_workspace(".", CountOptions::new()
///     .crates(vec!["my-lib".to_string()]))?;
///
/// // Exclude test files
/// let filter = FilterConfig::new().exclude("**/tests/**")?;
/// let result = count_workspace(".", CountOptions::new().filter(filter))?;
/// ```
pub fn count_workspace(path: impl AsRef<Path>, options: CountOptions) -> Result<CountResult> {
    let workspace = WorkspaceInfo::discover(path)?;

    // Filter crates if specified
    let crates: Vec<&CrateInfo> = if options.crate_filter.is_empty() {
        workspace.crates.iter().collect()
    } else {
        let names: Vec<&str> = options.crate_filter.iter().map(|s| s.as_str()).collect();
        workspace
            .crates
            .iter()
            .filter(|c| names.contains(&c.name.as_str()))
            .collect()
    };

    let mut result = CountResult::new();

    for crate_info in crates {
        let crate_stats = count_crate(crate_info, &options)?;
        result.total += crate_stats.stats.clone();
        if options.per_file_stats {
            result.files.extend(crate_stats.files.clone());
        }
        result.crates.push(crate_stats);
    }

    Ok(result)
}

/// Count LOC in a single crate.
fn count_crate(crate_info: &CrateInfo, options: &CountOptions) -> Result<CrateStats> {
    let dirs: Vec<&Path> = crate_info.all_dirs();
    let files = discover_files_in_dirs(&dirs, &options.file_filter)?;

    let mut crate_stats = CrateStats::new(crate_info.name.clone(), crate_info.root.clone());

    for file_path in files {
        let stats = parse_file(&file_path)?;
        let file_stats = FileStats::new(file_path, stats);
        crate_stats.add_file(file_stats);
    }

    Ok(crate_stats)
}

/// Count LOC in a directory (non-workspace mode).
///
/// Use this when you want to count files in a directory without
/// Cargo workspace awareness.
///
/// # Example
///
/// ```rust,ignore
/// use rustloclib::{count_directory, FilterConfig};
///
/// let filter = FilterConfig::new();
/// let result = count_directory("src/", &filter)?;
/// ```
pub fn count_directory(path: impl AsRef<Path>, filter: &FilterConfig) -> Result<CountResult> {
    let path = path.as_ref();

    if !path.exists() {
        return Err(RustlocError::PathNotFound(path.to_path_buf()));
    }

    let files = discover_files(path, filter)?;

    let mut result = CountResult::new();

    for file_path in files {
        let stats = parse_file(&file_path)?;
        result.total += stats.clone();
        result.files.push(FileStats::new(file_path, stats));
    }

    Ok(result)
}

/// Count LOC in a single file.
///
/// # Example
///
/// ```rust,ignore
/// use rustloclib::count_file;
///
/// let stats = count_file("src/main.rs")?;
/// println!("Code: {}, Tests: {}", stats.main.code, stats.tests.code);
/// ```
pub fn count_file(path: impl AsRef<Path>) -> Result<LocStats> {
    parse_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn create_rust_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn create_simple_project(root: &Path) {
        // Cargo.toml
        fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "test-project"
version = "0.1.0"
edition = "2021"
"#,
        )
        .unwrap();

        // src/main.rs
        create_rust_file(
            &root.join("src/main.rs"),
            r#"fn main() {
    println!("Hello");
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_main() {
        assert!(true);
    }
}
"#,
        );

        // src/lib.rs
        create_rust_file(
            &root.join("src/lib.rs"),
            r#"//! Library documentation

/// A public function
pub fn hello() {
    println!("Hello from lib");
}
"#,
        );
    }

    fn create_workspace(root: &Path) {
        // Workspace Cargo.toml
        fs::write(
            root.join("Cargo.toml"),
            r#"[workspace]
members = ["crate-a", "crate-b"]
"#,
        )
        .unwrap();

        // crate-a
        fs::create_dir_all(root.join("crate-a/src")).unwrap();
        fs::write(
            root.join("crate-a/Cargo.toml"),
            r#"[package]
name = "crate-a"
version = "0.1.0"
edition = "2021"
"#,
        )
        .unwrap();
        create_rust_file(
            &root.join("crate-a/src/lib.rs"),
            r#"pub fn a() {
    println!("A");
}
"#,
        );

        // crate-b
        fs::create_dir_all(root.join("crate-b/src")).unwrap();
        fs::write(
            root.join("crate-b/Cargo.toml"),
            r#"[package]
name = "crate-b"
version = "0.1.0"
edition = "2021"
"#,
        )
        .unwrap();
        create_rust_file(
            &root.join("crate-b/src/lib.rs"),
            r#"pub fn b() {
    println!("B");
}

// A comment
"#,
        );
    }

    #[test]
    fn test_count_directory() {
        let temp = tempdir().unwrap();
        let src = temp.path().join("src");
        fs::create_dir_all(&src).unwrap();

        create_rust_file(
            &src.join("main.rs"),
            r#"fn main() {
    println!("Hello");
}
"#,
        );

        let filter = FilterConfig::new();
        let result = count_directory(&src, &filter).unwrap();

        assert_eq!(result.total.file_count, 1);
        assert_eq!(result.total.main.code, 3);
    }

    #[test]
    fn test_count_file() {
        let temp = tempdir().unwrap();
        let file = temp.path().join("test.rs");

        create_rust_file(
            &file,
            r#"/// Doc comment
fn foo() {
    // Regular comment
    let x = 1;
}
"#,
        );

        let stats = count_file(&file).unwrap();

        assert_eq!(stats.file_count, 1);
        assert_eq!(stats.main.docs, 1);
        assert_eq!(stats.main.code, 3); // fn, let, }
        assert_eq!(stats.main.comments, 1);
    }

    #[test]
    fn test_count_workspace() {
        let temp = tempdir().unwrap();
        create_workspace(temp.path());

        let result = count_workspace(temp.path(), CountOptions::new()).unwrap();

        assert_eq!(result.crates.len(), 2);
        assert_eq!(result.total.file_count, 2);
    }

    #[test]
    fn test_count_workspace_filtered() {
        let temp = tempdir().unwrap();
        create_workspace(temp.path());

        let options = CountOptions::new().crates(vec!["crate-a".to_string()]);
        let result = count_workspace(temp.path(), options).unwrap();

        assert_eq!(result.crates.len(), 1);
        assert_eq!(result.crates[0].name, "crate-a");
        assert_eq!(result.total.file_count, 1);
    }

    #[test]
    fn test_count_workspace_with_file_stats() {
        let temp = tempdir().unwrap();
        create_workspace(temp.path());

        let options = CountOptions::new().with_file_stats();
        let result = count_workspace(temp.path(), options).unwrap();

        assert_eq!(result.files.len(), 2);
    }

    #[test]
    fn test_count_mixed_code_and_tests() {
        let temp = tempdir().unwrap();
        create_simple_project(temp.path());

        let result = count_workspace(temp.path(), CountOptions::new()).unwrap();

        // main.rs has 3 main code lines + test block
        // lib.rs has doc comment + code
        assert!(result.total.main.code > 0);
        assert!(result.total.tests.code > 0);
        assert!(result.total.main.docs > 0);
    }
}
