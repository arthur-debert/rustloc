//! Core data structures for LOC statistics

use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Sub, SubAssign};
use std::path::PathBuf;

/// Lines of code counts for a single context (main, tests, or examples)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Locs {
    /// Blank lines (whitespace only)
    pub blanks: u64,
    /// Code lines
    pub code: u64,
    /// Documentation comment lines (`///`, `//!`, `/** */`, `/*! */`)
    pub docs: u64,
    /// Regular comment lines (`//`, `/* */`)
    pub comments: u64,
}

impl Locs {
    /// Create a new Locs with all zeros
    pub fn new() -> Self {
        Self::default()
    }

    /// Total lines in this context
    pub fn total(&self) -> u64 {
        self.blanks + self.code + self.docs + self.comments
    }
}

impl Add for Locs {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            blanks: self.blanks + other.blanks,
            code: self.code + other.code,
            docs: self.docs + other.docs,
            comments: self.comments + other.comments,
        }
    }
}

impl AddAssign for Locs {
    fn add_assign(&mut self, other: Self) {
        self.blanks += other.blanks;
        self.code += other.code;
        self.docs += other.docs;
        self.comments += other.comments;
    }
}

impl Sub for Locs {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            blanks: self.blanks.saturating_sub(other.blanks),
            code: self.code.saturating_sub(other.code),
            docs: self.docs.saturating_sub(other.docs),
            comments: self.comments.saturating_sub(other.comments),
        }
    }
}

impl SubAssign for Locs {
    fn sub_assign(&mut self, other: Self) {
        self.blanks = self.blanks.saturating_sub(other.blanks);
        self.code = self.code.saturating_sub(other.code);
        self.docs = self.docs.saturating_sub(other.docs);
        self.comments = self.comments.saturating_sub(other.comments);
    }
}

/// Aggregated LOC statistics separating main code, tests, and examples
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocStats {
    /// Number of files analyzed
    pub file_count: u64,
    /// Main/production code
    pub main: Locs,
    /// Test code (`#[test]`, `#[cfg(test)]`, `tests/` directory)
    pub tests: Locs,
    /// Example code (`examples/` directory)
    pub examples: Locs,
}

impl LocStats {
    /// Create new empty stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Total blank lines across all contexts
    pub fn blanks(&self) -> u64 {
        self.main.blanks + self.tests.blanks + self.examples.blanks
    }

    /// Total code lines across all contexts
    pub fn code(&self) -> u64 {
        self.main.code + self.tests.code + self.examples.code
    }

    /// Total doc comment lines across all contexts
    pub fn docs(&self) -> u64 {
        self.main.docs + self.tests.docs + self.examples.docs
    }

    /// Total regular comment lines across all contexts
    pub fn comments(&self) -> u64 {
        self.main.comments + self.tests.comments + self.examples.comments
    }

    /// Total lines across all contexts
    pub fn total(&self) -> u64 {
        self.main.total() + self.tests.total() + self.examples.total()
    }
}

impl Add for LocStats {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            file_count: self.file_count + other.file_count,
            main: self.main + other.main,
            tests: self.tests + other.tests,
            examples: self.examples + other.examples,
        }
    }
}

impl AddAssign for LocStats {
    fn add_assign(&mut self, other: Self) {
        self.file_count += other.file_count;
        self.main += other.main;
        self.tests += other.tests;
        self.examples += other.examples;
    }
}

impl Sub for LocStats {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            file_count: self.file_count.saturating_sub(other.file_count),
            main: self.main - other.main,
            tests: self.tests - other.tests,
            examples: self.examples - other.examples,
        }
    }
}

impl SubAssign for LocStats {
    fn sub_assign(&mut self, other: Self) {
        self.file_count = self.file_count.saturating_sub(other.file_count);
        self.main -= other.main;
        self.tests -= other.tests;
        self.examples -= other.examples;
    }
}

/// Statistics for a single file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStats {
    /// Path to the file
    pub path: PathBuf,
    /// LOC statistics for this file
    pub stats: LocStats,
}

impl FileStats {
    /// Create new file stats
    pub fn new(path: PathBuf, stats: LocStats) -> Self {
        Self { path, stats }
    }
}

/// Statistics for a Rust module.
///
/// A module aggregates files at the directory level. In Rust's new-style module syntax:
/// - `foo/` directory and its sibling `foo.rs` file together form module "foo"
/// - `foo/bar.rs` is submodule "foo::bar"
/// - Files in `foo/` without a sibling `foo.rs` are still grouped under "foo"
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleStats {
    /// Module path (e.g., "foo", "foo::bar", or "" for root)
    pub name: String,
    /// Aggregated LOC statistics
    pub stats: LocStats,
    /// Files belonging to this module
    pub files: Vec<PathBuf>,
}

impl ModuleStats {
    /// Create new module stats
    pub fn new(name: String) -> Self {
        Self {
            name,
            stats: LocStats::new(),
            files: Vec::new(),
        }
    }

    /// Add stats from a file to this module
    pub fn add_file(&mut self, path: PathBuf, stats: LocStats) {
        self.stats += stats;
        self.files.push(path);
    }
}

/// Statistics for a crate within a workspace
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrateStats {
    /// Name of the crate
    pub name: String,
    /// Root path of the crate
    pub path: PathBuf,
    /// Aggregated LOC statistics
    pub stats: LocStats,
    /// Per-file statistics (optional, for detailed output)
    pub files: Vec<FileStats>,
}

impl CrateStats {
    /// Create new crate stats
    pub fn new(name: String, path: PathBuf) -> Self {
        Self {
            name,
            path,
            stats: LocStats::new(),
            files: Vec::new(),
        }
    }

    /// Add file stats to this crate
    pub fn add_file(&mut self, file_stats: FileStats) {
        self.stats += file_stats.stats.clone();
        self.files.push(file_stats);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locs_default() {
        let locs = Locs::new();
        assert_eq!(locs.blanks, 0);
        assert_eq!(locs.code, 0);
        assert_eq!(locs.docs, 0);
        assert_eq!(locs.comments, 0);
        assert_eq!(locs.total(), 0);
    }

    #[test]
    fn test_locs_total() {
        let locs = Locs {
            blanks: 10,
            code: 100,
            docs: 20,
            comments: 5,
        };
        assert_eq!(locs.total(), 135);
    }

    #[test]
    fn test_locs_add() {
        let a = Locs {
            blanks: 10,
            code: 100,
            docs: 20,
            comments: 5,
        };
        let b = Locs {
            blanks: 5,
            code: 50,
            docs: 10,
            comments: 2,
        };
        let sum = a + b;
        assert_eq!(sum.blanks, 15);
        assert_eq!(sum.code, 150);
        assert_eq!(sum.docs, 30);
        assert_eq!(sum.comments, 7);
    }

    #[test]
    fn test_loc_stats_totals() {
        let stats = LocStats {
            file_count: 3,
            main: Locs {
                blanks: 10,
                code: 100,
                docs: 20,
                comments: 5,
            },
            tests: Locs {
                blanks: 5,
                code: 50,
                docs: 2,
                comments: 3,
            },
            examples: Locs {
                blanks: 2,
                code: 20,
                docs: 5,
                comments: 1,
            },
        };

        assert_eq!(stats.blanks(), 17);
        assert_eq!(stats.code(), 170);
        assert_eq!(stats.docs(), 27);
        assert_eq!(stats.comments(), 9);
        assert_eq!(stats.total(), 223);
    }

    #[test]
    fn test_loc_stats_add() {
        let a = LocStats {
            file_count: 2,
            main: Locs {
                blanks: 10,
                code: 100,
                docs: 20,
                comments: 5,
            },
            tests: Locs::new(),
            examples: Locs::new(),
        };
        let b = LocStats {
            file_count: 1,
            main: Locs {
                blanks: 5,
                code: 50,
                docs: 10,
                comments: 2,
            },
            tests: Locs::new(),
            examples: Locs::new(),
        };

        let sum = a + b;
        assert_eq!(sum.file_count, 3);
        assert_eq!(sum.main.code, 150);
    }
}
