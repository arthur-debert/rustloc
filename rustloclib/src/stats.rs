//! Core data structures for LOC statistics

use crate::options::Contexts;
use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Sub, SubAssign};
use std::path::PathBuf;

/// A cell value that can represent either a count or a diff.
///
/// This provides a unified interface for displaying both count and diff statistics
/// using the same layout (rows = objects, columns = contexts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CellValue {
    /// A simple count value
    Count(u64),
    /// A diff value with added and removed counts
    Diff { added: u64, removed: u64 },
}

impl CellValue {
    /// Create a count cell
    pub fn count(value: u64) -> Self {
        CellValue::Count(value)
    }

    /// Create a diff cell
    pub fn diff(added: u64, removed: u64) -> Self {
        CellValue::Diff { added, removed }
    }

    /// Get the net value (for counts, this is just the value; for diffs, added - removed)
    pub fn net(&self) -> i64 {
        match self {
            CellValue::Count(v) => *v as i64,
            CellValue::Diff { added, removed } => *added as i64 - *removed as i64,
        }
    }

    /// Check if this is a count value
    pub fn is_count(&self) -> bool {
        matches!(self, CellValue::Count(_))
    }

    /// Check if this is a diff value
    pub fn is_diff(&self) -> bool {
        matches!(self, CellValue::Diff { .. })
    }
}

impl Default for CellValue {
    fn default() -> Self {
        CellValue::Count(0)
    }
}

impl std::fmt::Display for CellValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            CellValue::Count(v) => v.to_string(),
            CellValue::Diff { added, removed } => {
                let net = *added as i64 - *removed as i64;
                format!("+{}/-{}/{}", added, removed, net)
            }
        };

        // Respect width and alignment from the formatter
        if let Some(width) = f.width() {
            if f.align() == Some(std::fmt::Alignment::Left) {
                write!(f, "{:<width$}", s, width = width)
            } else {
                write!(f, "{:>width$}", s, width = width)
            }
        } else {
            write!(f, "{}", s)
        }
    }
}

/// A unified statistics row for display purposes.
///
/// This struct provides a common interface for displaying both count and diff statistics.
/// Each cell (code, tests, examples, total) uses `CellValue` which can be either a count or diff.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatsRow {
    /// Row label (file path, crate name, module name, or "Total")
    pub name: String,
    /// Production code cell
    pub code: CellValue,
    /// Test code cell
    pub tests: CellValue,
    /// Example code cell
    pub examples: CellValue,
    /// Total cell (sum of all contexts)
    pub total: CellValue,
    /// Number of files
    pub file_count: u64,
}

impl StatsRow {
    /// Create a new stats row from a count stats
    pub fn from_count(name: impl Into<String>, stats: &LocStats) -> Self {
        Self {
            name: name.into(),
            code: CellValue::count(stats.code.total()),
            tests: CellValue::count(stats.tests.total()),
            examples: CellValue::count(stats.examples.total()),
            total: CellValue::count(stats.total()),
            file_count: stats.file_count,
        }
    }

    /// Check if all cells are counts
    pub fn is_count(&self) -> bool {
        self.code.is_count()
    }

    /// Check if all cells are diffs
    pub fn is_diff(&self) -> bool {
        self.code.is_diff()
    }
}

/// Lines of code counts for a single context (code, tests, or examples)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Locs {
    /// Blank lines (whitespace only)
    pub blank: u64,
    /// Executable/logic lines (actual code, not comments or blanks)
    pub logic: u64,
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
        self.blank + self.logic + self.docs + self.comments
    }
}

impl Add for Locs {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            blank: self.blank + other.blank,
            logic: self.logic + other.logic,
            docs: self.docs + other.docs,
            comments: self.comments + other.comments,
        }
    }
}

impl AddAssign for Locs {
    fn add_assign(&mut self, other: Self) {
        self.blank += other.blank;
        self.logic += other.logic;
        self.docs += other.docs;
        self.comments += other.comments;
    }
}

impl Sub for Locs {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            blank: self.blank.saturating_sub(other.blank),
            logic: self.logic.saturating_sub(other.logic),
            docs: self.docs.saturating_sub(other.docs),
            comments: self.comments.saturating_sub(other.comments),
        }
    }
}

impl SubAssign for Locs {
    fn sub_assign(&mut self, other: Self) {
        self.blank = self.blank.saturating_sub(other.blank);
        self.logic = self.logic.saturating_sub(other.logic);
        self.docs = self.docs.saturating_sub(other.docs);
        self.comments = self.comments.saturating_sub(other.comments);
    }
}

/// Aggregated LOC statistics separating code, tests, and examples
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocStats {
    /// Number of files analyzed
    pub file_count: u64,
    /// Production code (not tests, not examples)
    pub code: Locs,
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
    pub fn blank(&self) -> u64 {
        self.code.blank + self.tests.blank + self.examples.blank
    }

    /// Total logic/executable lines across all contexts
    pub fn logic(&self) -> u64 {
        self.code.logic + self.tests.logic + self.examples.logic
    }

    /// Total doc comment lines across all contexts
    pub fn docs(&self) -> u64 {
        self.code.docs + self.tests.docs + self.examples.docs
    }

    /// Total regular comment lines across all contexts
    pub fn comments(&self) -> u64 {
        self.code.comments + self.tests.comments + self.examples.comments
    }

    /// Total lines across all contexts
    pub fn total(&self) -> u64 {
        self.code.total() + self.tests.total() + self.examples.total()
    }

    /// Return a filtered copy with only the specified contexts included.
    pub fn filter(&self, contexts: Contexts) -> Self {
        Self {
            file_count: self.file_count,
            code: if contexts.code {
                self.code
            } else {
                Locs::new()
            },
            tests: if contexts.tests {
                self.tests
            } else {
                Locs::new()
            },
            examples: if contexts.examples {
                self.examples
            } else {
                Locs::new()
            },
        }
    }
}

impl Add for LocStats {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            file_count: self.file_count + other.file_count,
            code: self.code + other.code,
            tests: self.tests + other.tests,
            examples: self.examples + other.examples,
        }
    }
}

impl AddAssign for LocStats {
    fn add_assign(&mut self, other: Self) {
        self.file_count += other.file_count;
        self.code += other.code;
        self.tests += other.tests;
        self.examples += other.examples;
    }
}

impl Sub for LocStats {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            file_count: self.file_count.saturating_sub(other.file_count),
            code: self.code - other.code,
            tests: self.tests - other.tests,
            examples: self.examples - other.examples,
        }
    }
}

impl SubAssign for LocStats {
    fn sub_assign(&mut self, other: Self) {
        self.file_count = self.file_count.saturating_sub(other.file_count);
        self.code -= other.code;
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

    /// Return a filtered copy with only the specified contexts included.
    pub fn filter(&self, contexts: Contexts) -> Self {
        Self {
            path: self.path.clone(),
            stats: self.stats.filter(contexts),
        }
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

    /// Return a filtered copy with only the specified contexts included.
    pub fn filter(&self, contexts: Contexts) -> Self {
        Self {
            name: self.name.clone(),
            stats: self.stats.filter(contexts),
            files: self.files.clone(),
        }
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

    /// Return a filtered copy with only the specified contexts included.
    pub fn filter(&self, contexts: Contexts) -> Self {
        Self {
            name: self.name.clone(),
            path: self.path.clone(),
            stats: self.stats.filter(contexts),
            files: self.files.iter().map(|f| f.filter(contexts)).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locs_default() {
        let locs = Locs::new();
        assert_eq!(locs.blank, 0);
        assert_eq!(locs.logic, 0);
        assert_eq!(locs.docs, 0);
        assert_eq!(locs.comments, 0);
        assert_eq!(locs.total(), 0);
    }

    #[test]
    fn test_locs_total() {
        let locs = Locs {
            blank: 10,
            logic: 100,
            docs: 20,
            comments: 5,
        };
        assert_eq!(locs.total(), 135);
    }

    #[test]
    fn test_locs_add() {
        let a = Locs {
            blank: 10,
            logic: 100,
            docs: 20,
            comments: 5,
        };
        let b = Locs {
            blank: 5,
            logic: 50,
            docs: 10,
            comments: 2,
        };
        let sum = a + b;
        assert_eq!(sum.blank, 15);
        assert_eq!(sum.logic, 150);
        assert_eq!(sum.docs, 30);
        assert_eq!(sum.comments, 7);
    }

    #[test]
    fn test_loc_stats_totals() {
        let stats = LocStats {
            file_count: 3,
            code: Locs {
                blank: 10,
                logic: 100,
                docs: 20,
                comments: 5,
            },
            tests: Locs {
                blank: 5,
                logic: 50,
                docs: 2,
                comments: 3,
            },
            examples: Locs {
                blank: 2,
                logic: 20,
                docs: 5,
                comments: 1,
            },
        };

        assert_eq!(stats.blank(), 17);
        assert_eq!(stats.logic(), 170);
        assert_eq!(stats.docs(), 27);
        assert_eq!(stats.comments(), 9);
        assert_eq!(stats.total(), 223);
    }

    #[test]
    fn test_loc_stats_add() {
        let a = LocStats {
            file_count: 2,
            code: Locs {
                blank: 10,
                logic: 100,
                docs: 20,
                comments: 5,
            },
            tests: Locs::new(),
            examples: Locs::new(),
        };
        let b = LocStats {
            file_count: 1,
            code: Locs {
                blank: 5,
                logic: 50,
                docs: 10,
                comments: 2,
            },
            tests: Locs::new(),
            examples: Locs::new(),
        };

        let sum = a + b;
        assert_eq!(sum.file_count, 3);
        assert_eq!(sum.code.logic, 150);
    }
}
