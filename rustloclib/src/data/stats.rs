//! Core data structures for LOC statistics.
//!
//! This module provides the fundamental types for representing line counts
//! in Rust source files. The design uses a single flat structure with 6 line types:
//!
//! - **code**: Logic lines in production code (src/, not in tests)
//! - **tests**: Logic lines in test code (#[test], #[cfg(test)], tests/)
//! - **examples**: Logic lines in example code (examples/)
//! - **docs**: Documentation comments (///, //!, /** */, /*! */) - anywhere
//! - **comments**: Regular comments (//, /* */) - anywhere
//! - **blanks**: Blank/whitespace-only lines - anywhere
//!
//! The key insight: only actual code lines need context (code/tests/examples),
//! because that's the meaningful distinction. A blank is a blank, a comment is
//! a comment - where they appear doesn't matter for most analysis.

use crate::query::options::LineTypes;
use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Sub, SubAssign};
use std::path::PathBuf;

/// Lines of code counts with 7 line types.
///
/// This is the fundamental unit of measurement in rustloc. Each field counts
/// a specific type of line:
///
/// - `code`, `tests`, `examples`: Actual executable/logic lines, distinguished by context
/// - `docs`, `comments`, `blanks`: Metadata lines, counted regardless of location
/// - `total`: Precomputed sum of all line types (total line count)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Locs {
    /// Logic lines in production code (src/, not in test blocks)
    pub code: u64,
    /// Logic lines in test code (#[test], #[cfg(test)], tests/ directory)
    pub tests: u64,
    /// Logic lines in example code (examples/ directory)
    pub examples: u64,
    /// Documentation comment lines (///, //!, /** */, /*! */)
    pub docs: u64,
    /// Regular comment lines (//, /* */)
    pub comments: u64,
    /// Blank lines (whitespace only)
    pub blanks: u64,
    /// Total line count (sum of all types)
    pub total: u64,
}

impl Locs {
    /// Create a new Locs with all zeros.
    pub fn new() -> Self {
        Self::default()
    }

    /// Total lines (returns precomputed `total` field).
    pub fn total(&self) -> u64 {
        self.total
    }

    /// Total logic lines (code + tests + examples).
    pub fn total_logic(&self) -> u64 {
        self.code + self.tests + self.examples
    }

    /// Recompute the `total` field from individual line types.
    /// Call this after manually setting individual fields.
    pub fn recompute_total(&mut self) {
        self.total =
            self.code + self.tests + self.examples + self.docs + self.comments + self.blanks;
    }

    /// Return a filtered copy with only the specified line types included.
    /// Unselected types are zeroed out. The `total` field is always preserved.
    pub fn filter(&self, types: LineTypes) -> Self {
        Self {
            code: if types.code { self.code } else { 0 },
            tests: if types.tests { self.tests } else { 0 },
            examples: if types.examples { self.examples } else { 0 },
            docs: if types.docs { self.docs } else { 0 },
            comments: if types.comments { self.comments } else { 0 },
            blanks: if types.blanks { self.blanks } else { 0 },
            total: self.total, // Always preserved
        }
    }
}

impl Add for Locs {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            code: self.code + other.code,
            tests: self.tests + other.tests,
            examples: self.examples + other.examples,
            docs: self.docs + other.docs,
            comments: self.comments + other.comments,
            blanks: self.blanks + other.blanks,
            total: self.total + other.total,
        }
    }
}

impl AddAssign for Locs {
    fn add_assign(&mut self, other: Self) {
        self.code += other.code;
        self.tests += other.tests;
        self.examples += other.examples;
        self.docs += other.docs;
        self.comments += other.comments;
        self.blanks += other.blanks;
        self.total += other.total;
    }
}

impl Sub for Locs {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            code: self.code.saturating_sub(other.code),
            tests: self.tests.saturating_sub(other.tests),
            examples: self.examples.saturating_sub(other.examples),
            docs: self.docs.saturating_sub(other.docs),
            comments: self.comments.saturating_sub(other.comments),
            blanks: self.blanks.saturating_sub(other.blanks),
            total: self.total.saturating_sub(other.total),
        }
    }
}

impl SubAssign for Locs {
    fn sub_assign(&mut self, other: Self) {
        self.code = self.code.saturating_sub(other.code);
        self.tests = self.tests.saturating_sub(other.tests);
        self.examples = self.examples.saturating_sub(other.examples);
        self.docs = self.docs.saturating_sub(other.docs);
        self.comments = self.comments.saturating_sub(other.comments);
        self.blanks = self.blanks.saturating_sub(other.blanks);
        self.total = self.total.saturating_sub(other.total);
    }
}

/// Statistics for a single file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStats {
    /// Path to the file.
    pub path: PathBuf,
    /// LOC statistics for this file.
    pub stats: Locs,
}

impl FileStats {
    /// Create new file stats.
    pub fn new(path: PathBuf, stats: Locs) -> Self {
        Self { path, stats }
    }

    /// Return a filtered copy with only the specified line types included.
    pub fn filter(&self, types: LineTypes) -> Self {
        Self {
            path: self.path.clone(),
            stats: self.stats.filter(types),
        }
    }
}

/// Statistics for a Rust module.
///
/// A module aggregates files at the directory level. In Rust's module syntax:
/// - `foo/` directory and its sibling `foo.rs` file together form module "foo"
/// - `foo/bar.rs` is submodule "foo::bar"
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleStats {
    /// Module path (e.g., "foo", "foo::bar", or "" for root).
    pub name: String,
    /// Aggregated LOC statistics.
    pub stats: Locs,
    /// Files belonging to this module.
    pub files: Vec<PathBuf>,
}

impl ModuleStats {
    /// Create new module stats.
    pub fn new(name: String) -> Self {
        Self {
            name,
            stats: Locs::new(),
            files: Vec::new(),
        }
    }

    /// Add stats from a file to this module.
    pub fn add_file(&mut self, path: PathBuf, stats: Locs) {
        self.stats += stats;
        self.files.push(path);
    }

    /// Return a filtered copy with only the specified line types included.
    pub fn filter(&self, types: LineTypes) -> Self {
        Self {
            name: self.name.clone(),
            stats: self.stats.filter(types),
            files: self.files.clone(),
        }
    }
}

/// Statistics for a crate within a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrateStats {
    /// Name of the crate.
    pub name: String,
    /// Root path of the crate.
    pub path: PathBuf,
    /// Aggregated LOC statistics.
    pub stats: Locs,
    /// Per-file statistics (for detailed output).
    pub files: Vec<FileStats>,
}

impl CrateStats {
    /// Create new crate stats.
    pub fn new(name: String, path: PathBuf) -> Self {
        Self {
            name,
            path,
            stats: Locs::new(),
            files: Vec::new(),
        }
    }

    /// Add file stats to this crate.
    pub fn add_file(&mut self, file_stats: FileStats) {
        self.stats += file_stats.stats;
        self.files.push(file_stats);
    }

    /// Return a filtered copy with only the specified line types included.
    pub fn filter(&self, types: LineTypes) -> Self {
        Self {
            name: self.name.clone(),
            path: self.path.clone(),
            stats: self.stats.filter(types),
            files: self.files.iter().map(|f| f.filter(types)).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locs_default() {
        let locs = Locs::new();
        assert_eq!(locs.code, 0);
        assert_eq!(locs.tests, 0);
        assert_eq!(locs.examples, 0);
        assert_eq!(locs.docs, 0);
        assert_eq!(locs.comments, 0);
        assert_eq!(locs.blanks, 0);
        assert_eq!(locs.total, 0);
        assert_eq!(locs.total(), 0);
    }

    #[test]
    fn test_locs_total() {
        let locs = Locs {
            code: 100,
            tests: 50,
            examples: 20,
            docs: 30,
            comments: 10,
            blanks: 15,
            total: 225,
        };
        assert_eq!(locs.total(), 225);
        assert_eq!(locs.total_logic(), 170);
    }

    #[test]
    fn test_locs_add() {
        let a = Locs {
            code: 100,
            tests: 50,
            examples: 20,
            docs: 30,
            comments: 10,
            blanks: 15,
            total: 225,
        };
        let b = Locs {
            code: 50,
            tests: 25,
            examples: 10,
            docs: 15,
            comments: 5,
            blanks: 10,
            total: 115,
        };
        let sum = a + b;
        assert_eq!(sum.code, 150);
        assert_eq!(sum.tests, 75);
        assert_eq!(sum.examples, 30);
        assert_eq!(sum.docs, 45);
        assert_eq!(sum.comments, 15);
        assert_eq!(sum.blanks, 25);
        assert_eq!(sum.total, 340);
    }

    #[test]
    fn test_locs_filter() {
        let locs = Locs {
            code: 100,
            tests: 50,
            examples: 20,
            docs: 30,
            comments: 10,
            blanks: 15,
            total: 225,
        };

        // Filter to only code - total is preserved
        let code_only = locs.filter(LineTypes::new().with_code());
        assert_eq!(code_only.code, 100);
        assert_eq!(code_only.tests, 0);
        assert_eq!(code_only.examples, 0);
        assert_eq!(code_only.docs, 0);
        assert_eq!(code_only.comments, 0);
        assert_eq!(code_only.blanks, 0);
        assert_eq!(code_only.total, 225); // Preserved

        // Filter to code + tests
        let code_tests = locs.filter(LineTypes::new().with_code().with_tests());
        assert_eq!(code_tests.code, 100);
        assert_eq!(code_tests.tests, 50);
        assert_eq!(code_tests.examples, 0);
        assert_eq!(code_tests.total, 225); // Preserved
    }

    #[test]
    fn test_recompute_total() {
        let mut locs = Locs {
            code: 100,
            tests: 50,
            examples: 20,
            docs: 30,
            comments: 10,
            blanks: 15,
            total: 0, // Intentionally wrong
        };
        locs.recompute_total();
        assert_eq!(locs.total, 225);
    }
}
