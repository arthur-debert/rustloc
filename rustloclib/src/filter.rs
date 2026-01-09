//! File filtering and discovery with glob pattern support.
//!
//! This module provides functionality to discover Rust source files
//! with support for include/exclude glob patterns.

use std::path::{Path, PathBuf};

use glob::Pattern;
use walkdir::WalkDir;

use crate::error::RustlocError;
use crate::Result;

/// Configuration for file filtering.
#[derive(Debug, Clone, Default)]
pub struct FilterConfig {
    /// Glob patterns to include (if empty, include all .rs files)
    pub include: Vec<Pattern>,
    /// Glob patterns to exclude
    pub exclude: Vec<Pattern>,
}

impl FilterConfig {
    /// Create a new empty filter config (includes all .rs files).
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an include pattern.
    pub fn include(mut self, pattern: &str) -> Result<Self> {
        let pat = Pattern::new(pattern).map_err(|e| RustlocError::InvalidGlob {
            pattern: pattern.to_string(),
            message: e.to_string(),
        })?;
        self.include.push(pat);
        Ok(self)
    }

    /// Add an exclude pattern.
    pub fn exclude(mut self, pattern: &str) -> Result<Self> {
        let pat = Pattern::new(pattern).map_err(|e| RustlocError::InvalidGlob {
            pattern: pattern.to_string(),
            message: e.to_string(),
        })?;
        self.exclude.push(pat);
        Ok(self)
    }

    /// Add multiple include patterns.
    pub fn include_many(mut self, patterns: &[&str]) -> Result<Self> {
        for pattern in patterns {
            self = self.include(pattern)?;
        }
        Ok(self)
    }

    /// Add multiple exclude patterns.
    pub fn exclude_many(mut self, patterns: &[&str]) -> Result<Self> {
        for pattern in patterns {
            self = self.exclude(pattern)?;
        }
        Ok(self)
    }

    /// Check if a path matches the filter criteria.
    ///
    /// A path matches if:
    /// 1. It's a .rs file
    /// 2. It matches at least one include pattern (or include is empty)
    /// 3. It doesn't match any exclude pattern
    pub fn matches(&self, path: &Path) -> bool {
        // Must be a .rs file
        if path.extension().is_none_or(|ext| ext != "rs") {
            return false;
        }

        let path_str = path.to_string_lossy();

        // Check excludes first
        for pattern in &self.exclude {
            if pattern.matches(&path_str) {
                return false;
            }
        }

        // If no include patterns, include all
        if self.include.is_empty() {
            return true;
        }

        // Must match at least one include pattern
        for pattern in &self.include {
            if pattern.matches(&path_str) {
                return true;
            }
        }

        false
    }
}

/// Check if a directory should be skipped during traversal.
fn should_skip_dir(name: &str) -> bool {
    // Skip hidden directories and target/
    name.starts_with('.') || name == "target"
}

/// Discover Rust source files in a directory.
///
/// Walks the directory tree and returns all .rs files that match the filter.
pub fn discover_files(root: impl AsRef<Path>, filter: &FilterConfig) -> Result<Vec<PathBuf>> {
    let root = root.as_ref();

    if !root.exists() {
        return Err(RustlocError::PathNotFound(root.to_path_buf()));
    }

    let mut files = Vec::new();

    if root.is_file() {
        if filter.matches(root) {
            files.push(root.to_path_buf());
        }
        return Ok(files);
    }

    let walker = WalkDir::new(root).follow_links(true).into_iter();

    for entry in walker.filter_entry(|e| {
        // Always include the root directory
        if e.depth() == 0 {
            return true;
        }
        // For non-root entries, skip hidden dirs and target/
        if e.file_type().is_dir() {
            let name = e.file_name().to_str().unwrap_or("");
            return !should_skip_dir(name);
        }
        // Include files
        true
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();

        if path.is_file() && filter.matches(path) {
            files.push(path.to_path_buf());
        }
    }

    // Sort for deterministic output
    files.sort();

    Ok(files)
}

/// Discover Rust source files in multiple directories.
pub fn discover_files_in_dirs(dirs: &[&Path], filter: &FilterConfig) -> Result<Vec<PathBuf>> {
    let mut all_files = Vec::new();

    for dir in dirs {
        let files = discover_files(dir, filter)?;
        all_files.extend(files);
    }

    // Remove duplicates and sort
    all_files.sort();
    all_files.dedup();

    Ok(all_files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn create_test_files(dir: &Path) {
        // Create directory structure
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::create_dir_all(dir.join("src/utils")).unwrap();
        fs::create_dir_all(dir.join("tests")).unwrap();
        fs::create_dir_all(dir.join("examples")).unwrap();
        fs::create_dir_all(dir.join("target/debug")).unwrap();
        fs::create_dir_all(dir.join(".hidden")).unwrap();

        // Create files
        fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(dir.join("src/lib.rs"), "pub mod utils;").unwrap();
        fs::write(dir.join("src/utils/mod.rs"), "pub fn util() {}").unwrap();
        fs::write(dir.join("src/utils/helper.rs"), "pub fn help() {}").unwrap();
        fs::write(dir.join("tests/integration.rs"), "#[test] fn test() {}").unwrap();
        fs::write(dir.join("examples/demo.rs"), "fn main() {}").unwrap();
        fs::write(dir.join("target/debug/build.rs"), "// generated").unwrap();
        fs::write(dir.join(".hidden/secret.rs"), "// hidden").unwrap();
        fs::write(dir.join("README.md"), "# Readme").unwrap();
    }

    #[test]
    fn test_filter_matches_rs_files() {
        let filter = FilterConfig::new();

        assert!(filter.matches(Path::new("src/main.rs")));
        assert!(filter.matches(Path::new("lib.rs")));
        assert!(!filter.matches(Path::new("README.md")));
        assert!(!filter.matches(Path::new("Cargo.toml")));
    }

    #[test]
    fn test_filter_with_include_pattern() {
        let filter = FilterConfig::new().include("**/utils/*.rs").unwrap();

        assert!(filter.matches(Path::new("src/utils/mod.rs")));
        assert!(filter.matches(Path::new("src/utils/helper.rs")));
        assert!(!filter.matches(Path::new("src/main.rs")));
        assert!(!filter.matches(Path::new("src/lib.rs")));
    }

    #[test]
    fn test_filter_with_exclude_pattern() {
        let filter = FilterConfig::new().exclude("**/tests/**").unwrap();

        assert!(filter.matches(Path::new("src/main.rs")));
        assert!(!filter.matches(Path::new("tests/integration.rs")));
        assert!(!filter.matches(Path::new("src/tests/test.rs")));
    }

    #[test]
    fn test_filter_with_multiple_patterns() {
        let filter = FilterConfig::new()
            .include_many(&["**/src/**", "**/tests/**"])
            .unwrap()
            .exclude("**/utils/**")
            .unwrap();

        assert!(filter.matches(Path::new("project/src/main.rs")));
        assert!(filter.matches(Path::new("project/tests/test.rs")));
        assert!(!filter.matches(Path::new("project/src/utils/helper.rs")));
        assert!(!filter.matches(Path::new("project/examples/demo.rs")));
    }

    #[test]
    fn test_discover_files() {
        let temp = tempdir().unwrap();
        create_test_files(temp.path());

        let filter = FilterConfig::new();
        let files = discover_files(temp.path(), &filter).unwrap();

        // Should find all .rs files except in target/ and .hidden/
        assert!(files.iter().any(|p| p.ends_with("src/main.rs")));
        assert!(files.iter().any(|p| p.ends_with("src/lib.rs")));
        assert!(files.iter().any(|p| p.ends_with("src/utils/mod.rs")));
        assert!(files.iter().any(|p| p.ends_with("tests/integration.rs")));
        assert!(files.iter().any(|p| p.ends_with("examples/demo.rs")));

        // Should not find files in target/ or .hidden/
        assert!(!files.iter().any(|p| p.to_string_lossy().contains("target")));
        assert!(!files
            .iter()
            .any(|p| p.to_string_lossy().contains(".hidden")));
    }

    #[test]
    fn test_discover_files_with_filter() {
        let temp = tempdir().unwrap();
        create_test_files(temp.path());

        let filter = FilterConfig::new()
            .exclude("**/tests/**")
            .unwrap()
            .exclude("**/examples/**")
            .unwrap();

        let files = discover_files(temp.path(), &filter).unwrap();

        // Should find src files only
        assert!(files.iter().any(|p| p.ends_with("src/main.rs")));
        assert!(!files.iter().any(|p| p.ends_with("tests/integration.rs")));
        assert!(!files.iter().any(|p| p.ends_with("examples/demo.rs")));
    }

    #[test]
    fn test_discover_single_file() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("test.rs");
        fs::write(&file_path, "fn test() {}").unwrap();

        let filter = FilterConfig::new();
        let files = discover_files(&file_path, &filter).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0], file_path);
    }

    #[test]
    fn test_discover_files_nonexistent() {
        let filter = FilterConfig::new();
        let result = discover_files("/nonexistent/path", &filter);

        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_glob_pattern() {
        let result = FilterConfig::new().include("[invalid");

        assert!(result.is_err());
        if let Err(RustlocError::InvalidGlob { pattern, .. }) = result {
            assert_eq!(pattern, "[invalid");
        } else {
            panic!("Expected InvalidGlob error");
        }
    }
}
