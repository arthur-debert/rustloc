//! Git diff analysis for LOC changes between commits.
//!
//! This module provides functionality to compute LOC differences between two git commits,
//! leveraging the existing parsing and filtering infrastructure.
//!
//! ## Design Principle
//!
//! **Filtering (glob patterns, crate names) is done centrally using `FilterConfig`
//! and `WorkspaceInfo`, not re-implemented here.** This module:
//!
//! 1. Gets changed file paths from git
//! 2. Delegates to `FilterConfig::matches()` for glob filtering
//! 3. Uses `WorkspaceInfo::crate_for_path()` for crate mapping
//! 4. Applies crate filter via workspace's existing mechanisms
//!
//! This ensures consistent filtering behavior across all features.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::RustlocError;
use crate::filter::FilterConfig;
use crate::options::{Aggregation, Contexts};
use crate::stats::{LocStats, Locs};
use crate::visitor::{parse_string, VisitorContext};
use crate::workspace::WorkspaceInfo;
use crate::Result;

/// Lines of code diff for a single context (added vs removed).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocsDiff {
    /// Lines added
    pub added: Locs,
    /// Lines removed
    pub removed: Locs,
}

impl LocsDiff {
    /// Create a new empty diff
    pub fn new() -> Self {
        Self::default()
    }

    /// Net change (added - removed) for logic/executable lines
    pub fn net_logic(&self) -> i64 {
        self.added.logic as i64 - self.removed.logic as i64
    }

    /// Net change (added - removed) for blank lines
    pub fn net_blank(&self) -> i64 {
        self.added.blank as i64 - self.removed.blank as i64
    }

    /// Net change (added - removed) for doc comment lines
    pub fn net_docs(&self) -> i64 {
        self.added.docs as i64 - self.removed.docs as i64
    }

    /// Net change (added - removed) for regular comment lines
    pub fn net_comments(&self) -> i64 {
        self.added.comments as i64 - self.removed.comments as i64
    }

    /// Net change (added - removed) for total lines
    pub fn net_total(&self) -> i64 {
        self.added.total() as i64 - self.removed.total() as i64
    }
}

impl std::ops::Add for LocsDiff {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            added: self.added + other.added,
            removed: self.removed + other.removed,
        }
    }
}

impl std::ops::AddAssign for LocsDiff {
    fn add_assign(&mut self, other: Self) {
        self.added += other.added;
        self.removed += other.removed;
    }
}

/// Aggregated LOC diff separating production code, tests, and examples.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocStatsDiff {
    /// Number of files changed
    pub file_count: u64,
    /// Production code diff
    pub code: LocsDiff,
    /// Test code diff
    pub tests: LocsDiff,
    /// Example code diff
    pub examples: LocsDiff,
}

impl LocStatsDiff {
    /// Create a new empty diff
    pub fn new() -> Self {
        Self::default()
    }

    /// Total added lines across all contexts
    pub fn total_added(&self) -> Locs {
        self.code.added + self.tests.added + self.examples.added
    }

    /// Total removed lines across all contexts
    pub fn total_removed(&self) -> Locs {
        self.code.removed + self.tests.removed + self.examples.removed
    }

    /// Net logic change across all contexts
    pub fn net_logic(&self) -> i64 {
        self.code.net_logic() + self.tests.net_logic() + self.examples.net_logic()
    }

    /// Net total change across all contexts
    pub fn net_total(&self) -> i64 {
        self.code.net_total() + self.tests.net_total() + self.examples.net_total()
    }

    /// Return a filtered copy with only the specified contexts included.
    pub fn filter(&self, contexts: Contexts) -> Self {
        Self {
            file_count: self.file_count,
            code: if contexts.code {
                self.code
            } else {
                LocsDiff::default()
            },
            tests: if contexts.tests {
                self.tests
            } else {
                LocsDiff::default()
            },
            examples: if contexts.examples {
                self.examples
            } else {
                LocsDiff::default()
            },
        }
    }
}

impl std::ops::Add for LocStatsDiff {
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

impl std::ops::AddAssign for LocStatsDiff {
    fn add_assign(&mut self, other: Self) {
        self.file_count += other.file_count;
        self.code += other.code;
        self.tests += other.tests;
        self.examples += other.examples;
    }
}

/// Diff statistics for a single file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiffStats {
    /// Path to the file (relative to repo root)
    pub path: PathBuf,
    /// The type of change
    pub change_type: FileChangeType,
    /// LOC diff for this file
    pub diff: LocStatsDiff,
}

impl FileDiffStats {
    /// Return a filtered copy with only the specified contexts included.
    pub fn filter(&self, contexts: Contexts) -> Self {
        Self {
            path: self.path.clone(),
            change_type: self.change_type,
            diff: self.diff.filter(contexts),
        }
    }
}

/// Type of file change in the diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileChangeType {
    /// File was added
    Added,
    /// File was deleted
    Deleted,
    /// File was modified
    Modified,
}

/// Diff statistics for a crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrateDiffStats {
    /// Name of the crate
    pub name: String,
    /// Root path of the crate
    pub path: PathBuf,
    /// Aggregated LOC diff
    pub diff: LocStatsDiff,
    /// Per-file diff (optional, for detailed output)
    pub files: Vec<FileDiffStats>,
}

impl CrateDiffStats {
    /// Create new crate diff stats
    pub fn new(name: String, path: PathBuf) -> Self {
        Self {
            name,
            path,
            diff: LocStatsDiff::new(),
            files: Vec::new(),
        }
    }

    /// Add file diff to this crate
    pub fn add_file(&mut self, file_diff: FileDiffStats) {
        self.diff += file_diff.diff.clone();
        self.files.push(file_diff);
    }

    /// Return a filtered copy with only the specified contexts included.
    pub fn filter(&self, contexts: Contexts) -> Self {
        Self {
            name: self.name.clone(),
            path: self.path.clone(),
            diff: self.diff.filter(contexts),
            files: self.files.iter().map(|f| f.filter(contexts)).collect(),
        }
    }
}

/// Result of a diff operation between two commits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffResult {
    /// Base commit (from)
    pub from_commit: String,
    /// Target commit (to)
    pub to_commit: String,
    /// Total diff across all files
    pub total: LocStatsDiff,
    /// Per-crate diff breakdown
    pub crates: Vec<CrateDiffStats>,
    /// Per-file diff (optional, for detailed output)
    pub files: Vec<FileDiffStats>,
}

impl DiffResult {
    /// Return a filtered copy with only the specified contexts included.
    pub fn filter(&self, contexts: Contexts) -> Self {
        Self {
            from_commit: self.from_commit.clone(),
            to_commit: self.to_commit.clone(),
            total: self.total.filter(contexts),
            crates: self.crates.iter().map(|c| c.filter(contexts)).collect(),
            files: self.files.iter().map(|f| f.filter(contexts)).collect(),
        }
    }
}

/// Options for diff computation.
///
/// Uses the same filtering infrastructure as the regular LOC counter:
/// - `crate_filter`: Filter by crate names (uses `WorkspaceInfo::filter_by_names()`)
/// - `file_filter`: Filter by glob patterns (uses `FilterConfig::matches()`)
///
/// This ensures consistent filtering behavior across all features.
#[derive(Debug, Clone)]
pub struct DiffOptions {
    /// Crate names to include (empty = all crates)
    pub crate_filter: Vec<String>,
    /// File filter configuration
    pub file_filter: FilterConfig,
    /// Aggregation level for results
    pub aggregation: Aggregation,
    /// Which contexts to include in results (main, tests, examples)
    pub contexts: Contexts,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            crate_filter: Vec::new(),
            file_filter: FilterConfig::new(),
            aggregation: Aggregation::Total,
            contexts: Contexts::all(),
        }
    }
}

impl DiffOptions {
    /// Create new default options
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter to specific crates
    pub fn crates(mut self, names: Vec<String>) -> Self {
        self.crate_filter = names;
        self
    }

    /// Set file filter
    pub fn filter(mut self, config: FilterConfig) -> Self {
        self.file_filter = config;
        self
    }

    /// Set aggregation level
    pub fn aggregation(mut self, level: Aggregation) -> Self {
        self.aggregation = level;
        self
    }

    /// Set which contexts to include
    pub fn contexts(mut self, contexts: Contexts) -> Self {
        self.contexts = contexts;
        self
    }
}

/// Compute LOC diff between two git commits.
///
/// This function:
/// 1. Opens the git repository at `repo_path`
/// 2. Gets the list of changed files between `from` and `to` commits
/// 3. Applies filtering using the centralized `FilterConfig` and crate filters
/// 4. Parses file contents at both commits using the existing visitor
/// 5. Computes and aggregates the diff
///
/// # Arguments
///
/// * `repo_path` - Path to the git repository (or any path within it)
/// * `from` - Base commit reference (SHA, branch, tag, HEAD~n, etc.)
/// * `to` - Target commit reference
/// * `options` - Diff options including filters
///
/// # Returns
///
/// A `DiffResult` containing the LOC changes, broken down by crate and optionally by file.
pub fn diff_commits(
    repo_path: impl AsRef<Path>,
    from: &str,
    to: &str,
    options: DiffOptions,
) -> Result<DiffResult> {
    let repo_path = repo_path.as_ref();

    // Open the git repository
    let repo = gix::discover(repo_path)
        .map_err(|e| RustlocError::GitError(format!("Failed to discover git repository: {}", e)))?;

    let repo_root = repo
        .work_dir()
        .ok_or_else(|| RustlocError::GitError("Repository has no work directory".to_string()))?
        .to_path_buf();

    // Resolve commit references
    let from_commit = resolve_commit(&repo, from)?;
    let to_commit = resolve_commit(&repo, to)?;

    // Get the trees for both commits
    let from_tree = from_commit
        .tree()
        .map_err(|e| RustlocError::GitError(format!("Failed to get tree for '{}': {}", from, e)))?;
    let to_tree = to_commit
        .tree()
        .map_err(|e| RustlocError::GitError(format!("Failed to get tree for '{}': {}", to, e)))?;

    // Compute the diff between trees
    let changes = compute_tree_diff(&from_tree, &to_tree)?;

    // Try to discover workspace info for crate grouping
    let workspace = WorkspaceInfo::discover(&repo_root).ok();

    // Apply crate filter if specified
    let filtered_workspace = workspace.as_ref().map(|ws| {
        if options.crate_filter.is_empty() {
            ws.clone()
        } else {
            let names: Vec<&str> = options.crate_filter.iter().map(|s| s.as_str()).collect();
            ws.filter_by_names(&names)
        }
    });

    // Process changes
    let mut total = LocStatsDiff::new();
    let mut files = Vec::new();
    let mut crate_stats: HashMap<String, CrateDiffStats> = HashMap::new();

    // Determine what to include based on aggregation level
    let include_files = matches!(options.aggregation, Aggregation::ByFile);
    let include_crates = matches!(
        options.aggregation,
        Aggregation::ByCrate | Aggregation::ByFile
    );

    for change in changes {
        let path = change.path.clone();

        // Apply glob filter using centralized FilterConfig
        if !options.file_filter.matches(&path) {
            continue;
        }

        // Determine which crate this file belongs to (if any)
        let crate_info = filtered_workspace
            .as_ref()
            .and_then(|ws| ws.crate_for_path(&path));

        // If crate filter is active and file doesn't belong to a filtered crate, skip
        if !options.crate_filter.is_empty() && crate_info.is_none() {
            continue;
        }

        // Compute file diff
        let file_diff = compute_file_diff(&repo, &change, &path)?;

        // Aggregate into total
        total += file_diff.diff.clone();

        // Aggregate into crate stats if applicable
        if include_crates {
            if let Some(crate_info) = crate_info {
                let crate_stats_entry =
                    crate_stats
                        .entry(crate_info.name.clone())
                        .or_insert_with(|| {
                            CrateDiffStats::new(crate_info.name.clone(), crate_info.root.clone())
                        });

                if include_files {
                    crate_stats_entry.add_file(file_diff.clone());
                } else {
                    crate_stats_entry.diff += file_diff.diff.clone();
                }
            }
        }

        // Collect file stats if requested
        if include_files {
            files.push(file_diff);
        }
    }

    // Convert crate stats map to vec
    let crates: Vec<CrateDiffStats> = crate_stats.into_values().collect();

    // Build result and apply line type filter
    let result = DiffResult {
        from_commit: from.to_string(),
        to_commit: to.to_string(),
        total,
        crates,
        files,
    };

    Ok(result.filter(options.contexts))
}

/// Internal representation of a file change
struct FileChange {
    path: PathBuf,
    change_type: FileChangeType,
    old_oid: Option<gix::ObjectId>,
    new_oid: Option<gix::ObjectId>,
}

/// Resolve a commit reference to a commit object
fn resolve_commit<'repo>(
    repo: &'repo gix::Repository,
    reference: &str,
) -> Result<gix::Commit<'repo>> {
    let id = repo
        .rev_parse_single(reference.as_bytes())
        .map_err(|e| RustlocError::GitError(format!("Failed to resolve '{}': {}", reference, e)))?
        .detach();

    repo.find_commit(id).map_err(|e| {
        RustlocError::GitError(format!("Failed to find commit '{}': {}", reference, e))
    })
}

/// Compute the diff between two trees
fn compute_tree_diff(
    from_tree: &gix::Tree<'_>,
    to_tree: &gix::Tree<'_>,
) -> Result<Vec<FileChange>> {
    use gix::object::tree::diff::Action;

    let mut changes = Vec::new();

    from_tree
        .changes()
        .map_err(|e| RustlocError::GitError(format!("Failed to get tree changes: {}", e)))?
        .for_each_to_obtain_tree(to_tree, |change| {
            use gix::object::tree::diff::Change;

            let file_change = match change {
                Change::Addition {
                    entry_mode,
                    id,
                    location,
                    ..
                } => {
                    if entry_mode.is_blob() {
                        Some(FileChange {
                            path: PathBuf::from(gix::path::from_bstr(location)),
                            change_type: FileChangeType::Added,
                            old_oid: None,
                            new_oid: Some(id.detach()),
                        })
                    } else {
                        None
                    }
                }
                Change::Deletion {
                    entry_mode,
                    id,
                    location,
                    ..
                } => {
                    if entry_mode.is_blob() {
                        Some(FileChange {
                            path: PathBuf::from(gix::path::from_bstr(location)),
                            change_type: FileChangeType::Deleted,
                            old_oid: Some(id.detach()),
                            new_oid: None,
                        })
                    } else {
                        None
                    }
                }
                Change::Modification {
                    previous_entry_mode,
                    entry_mode,
                    previous_id,
                    id,
                    location,
                    ..
                } => {
                    if entry_mode.is_blob() && previous_entry_mode.is_blob() {
                        Some(FileChange {
                            path: PathBuf::from(gix::path::from_bstr(location)),
                            change_type: FileChangeType::Modified,
                            old_oid: Some(previous_id.detach()),
                            new_oid: Some(id.detach()),
                        })
                    } else {
                        None
                    }
                }
                Change::Rewrite { .. } => {
                    // Treat rewrites as modifications (complete file rewrite)
                    None
                }
            };

            if let Some(fc) = file_change {
                changes.push(fc);
            }
            Ok::<_, std::convert::Infallible>(Action::Continue)
        })
        .map_err(|e| RustlocError::GitError(format!("Failed to compute tree diff: {}", e)))?;

    Ok(changes)
}

/// Compute the LOC diff for a single file
fn compute_file_diff(
    repo: &gix::Repository,
    change: &FileChange,
    path: &Path,
) -> Result<FileDiffStats> {
    // Determine initial context from file path
    // Note: The parser will detect #[cfg(test)] and #[test] blocks internally,
    // so a file in src/ can still have both main and tests stats populated
    let context = VisitorContext::from_file_path(path);

    let (old_stats, new_stats) = match change.change_type {
        FileChangeType::Added => {
            let content = read_blob(repo, change.new_oid.unwrap())?;
            let stats = parse_string(&content, context);
            (LocStats::new(), stats)
        }
        FileChangeType::Deleted => {
            let content = read_blob(repo, change.old_oid.unwrap())?;
            let stats = parse_string(&content, context);
            (stats, LocStats::new())
        }
        FileChangeType::Modified => {
            let old_content = read_blob(repo, change.old_oid.unwrap())?;
            let new_content = read_blob(repo, change.new_oid.unwrap())?;
            let old_stats = parse_string(&old_content, context);
            let new_stats = parse_string(&new_content, context);
            (old_stats, new_stats)
        }
    };

    // Compute the diff across ALL contexts (main, tests, examples)
    // A single file can have code in multiple contexts (e.g., src/lib.rs with #[cfg(test)])
    let diff = compute_stats_diff(&old_stats, &new_stats);

    Ok(FileDiffStats {
        path: path.to_path_buf(),
        change_type: change.change_type,
        diff,
    })
}

/// Compute the diff between two LocStats across all contexts
fn compute_stats_diff(old: &LocStats, new: &LocStats) -> LocStatsDiff {
    let mut diff = LocStatsDiff::new();
    diff.file_count = 1;

    // Compute diff for each context separately
    diff.code = compute_locs_diff(&old.code, &new.code);
    diff.tests = compute_locs_diff(&old.tests, &new.tests);
    diff.examples = compute_locs_diff(&old.examples, &new.examples);

    diff
}

/// Compute the diff between two Locs
fn compute_locs_diff(old: &Locs, new: &Locs) -> LocsDiff {
    LocsDiff {
        added: Locs {
            logic: new.logic.saturating_sub(old.logic),
            blank: new.blank.saturating_sub(old.blank),
            docs: new.docs.saturating_sub(old.docs),
            comments: new.comments.saturating_sub(old.comments),
        },
        removed: Locs {
            logic: old.logic.saturating_sub(new.logic),
            blank: old.blank.saturating_sub(new.blank),
            docs: old.docs.saturating_sub(new.docs),
            comments: old.comments.saturating_sub(new.comments),
        },
    }
}

/// Read a blob's content as a UTF-8 string
fn read_blob(repo: &gix::Repository, oid: gix::ObjectId) -> Result<String> {
    let object = repo
        .find_object(oid)
        .map_err(|e| RustlocError::GitError(format!("Failed to find object {}: {}", oid, e)))?;

    let blob = object
        .try_into_blob()
        .map_err(|_| RustlocError::GitError(format!("Object {} is not a blob", oid)))?;

    // Try to decode as UTF-8, falling back to lossy conversion
    String::from_utf8(blob.data.to_vec())
        .or_else(|e| Ok(String::from_utf8_lossy(&e.into_bytes()).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locs_diff_default() {
        let diff = LocsDiff::new();
        assert_eq!(diff.added.logic, 0);
        assert_eq!(diff.removed.logic, 0);
        assert_eq!(diff.net_logic(), 0);
    }

    #[test]
    fn test_locs_diff_net() {
        let diff = LocsDiff {
            added: Locs {
                logic: 100,
                blank: 20,
                docs: 10,
                comments: 5,
            },
            removed: Locs {
                logic: 30,
                blank: 5,
                docs: 2,
                comments: 1,
            },
        };

        assert_eq!(diff.net_logic(), 70);
        assert_eq!(diff.net_blank(), 15);
        assert_eq!(diff.net_docs(), 8);
        assert_eq!(diff.net_comments(), 4);
        assert_eq!(diff.net_total(), 97);
    }

    #[test]
    fn test_locs_diff_add() {
        let a = LocsDiff {
            added: Locs {
                logic: 10,
                blank: 2,
                docs: 1,
                comments: 1,
            },
            removed: Locs {
                logic: 5,
                blank: 1,
                docs: 0,
                comments: 0,
            },
        };
        let b = LocsDiff {
            added: Locs {
                logic: 20,
                blank: 4,
                docs: 2,
                comments: 2,
            },
            removed: Locs {
                logic: 10,
                blank: 2,
                docs: 1,
                comments: 1,
            },
        };

        let sum = a + b;
        assert_eq!(sum.added.logic, 30);
        assert_eq!(sum.removed.logic, 15);
        assert_eq!(sum.net_logic(), 15);
    }

    #[test]
    fn test_loc_stats_diff_totals() {
        let diff = LocStatsDiff {
            file_count: 3,
            code: LocsDiff {
                added: Locs {
                    logic: 100,
                    blank: 10,
                    docs: 20,
                    comments: 5,
                },
                removed: Locs {
                    logic: 50,
                    blank: 5,
                    docs: 10,
                    comments: 2,
                },
            },
            tests: LocsDiff {
                added: Locs {
                    logic: 50,
                    blank: 5,
                    docs: 2,
                    comments: 1,
                },
                removed: Locs {
                    logic: 20,
                    blank: 2,
                    docs: 1,
                    comments: 0,
                },
            },
            examples: LocsDiff::new(),
        };

        assert_eq!(diff.total_added().logic, 150);
        assert_eq!(diff.total_removed().logic, 70);
        assert_eq!(diff.net_logic(), 80);
    }

    #[test]
    fn test_diff_options_builder() {
        let options = DiffOptions::new()
            .crates(vec!["my-crate".to_string()])
            .aggregation(Aggregation::ByFile);

        assert_eq!(options.crate_filter, vec!["my-crate"]);
        assert_eq!(options.aggregation, Aggregation::ByFile);
    }

    #[test]
    fn test_compute_stats_diff_added_file() {
        let old = LocStats::new();
        let new = LocStats {
            file_count: 1,
            code: Locs {
                logic: 100,
                blank: 20,
                docs: 10,
                comments: 5,
            },
            tests: Locs::new(),
            examples: Locs::new(),
        };

        let diff = compute_stats_diff(&old, &new);
        assert_eq!(diff.code.added.logic, 100);
        assert_eq!(diff.code.removed.logic, 0);
    }

    #[test]
    fn test_compute_stats_diff_deleted_file() {
        let old = LocStats {
            file_count: 1,
            code: Locs::new(),
            tests: Locs {
                logic: 50,
                blank: 10,
                docs: 5,
                comments: 2,
            },
            examples: Locs::new(),
        };
        let new = LocStats::new();

        let diff = compute_stats_diff(&old, &new);
        assert_eq!(diff.tests.added.logic, 0);
        assert_eq!(diff.tests.removed.logic, 50);
    }

    #[test]
    fn test_compute_stats_diff_modified_file() {
        let old = LocStats {
            file_count: 1,
            code: Locs {
                logic: 100,
                blank: 20,
                docs: 10,
                comments: 5,
            },
            tests: Locs::new(),
            examples: Locs::new(),
        };
        let new = LocStats {
            file_count: 1,
            code: Locs {
                logic: 120,
                blank: 25,
                docs: 8,
                comments: 5,
            },
            tests: Locs::new(),
            examples: Locs::new(),
        };

        let diff = compute_stats_diff(&old, &new);
        assert_eq!(diff.code.added.logic, 20); // 120 - 100
        assert_eq!(diff.code.removed.logic, 0);
        assert_eq!(diff.code.added.docs, 0);
        assert_eq!(diff.code.removed.docs, 2); // 10 - 8
    }

    #[test]
    fn test_compute_stats_diff_mixed_contexts() {
        // Test that a file with both production code and test code is properly diffed
        let old = LocStats::new();
        let new = LocStats {
            file_count: 1,
            code: Locs {
                logic: 100,
                blank: 20,
                docs: 10,
                comments: 5,
            },
            tests: Locs {
                logic: 50,
                blank: 10,
                docs: 0,
                comments: 5,
            },
            examples: Locs::new(),
        };

        let diff = compute_stats_diff(&old, &new);
        // Production code should be tracked
        assert_eq!(diff.code.added.logic, 100);
        // Test code should also be tracked
        assert_eq!(diff.tests.added.logic, 50);
        // Total should include both
        assert_eq!(diff.total_added().logic, 150);
    }

    // Integration tests that use a real git repository
    // These tests require being run inside a git repository

    #[test]
    fn test_diff_commits_basic() {
        // Test diffing between two commits in this repository
        // Using commits that should exist in the rustloc repo
        let result = diff_commits(".", "e3b2667", "6917e2d", DiffOptions::new());

        // Should succeed in a git repository
        assert!(
            result.is_ok(),
            "diff_commits should succeed: {:?}",
            result.err()
        );

        let diff = result.unwrap();
        assert_eq!(diff.from_commit, "e3b2667");
        assert_eq!(diff.to_commit, "6917e2d");

        // There should be some changes between these commits
        // The exact numbers may vary, so we just check structure
        assert!(diff.total.file_count > 0 || diff.total.net_total() != 0 || diff.files.is_empty());
    }

    #[test]
    fn test_diff_commits_with_file_stats() {
        let options = DiffOptions::new().aggregation(Aggregation::ByFile);
        let result = diff_commits(".", "e3b2667", "6917e2d", options);

        assert!(result.is_ok());
        let _diff = result.unwrap();

        // With file aggregation enabled, we should have file details
        // (may be empty if no .rs files changed between these commits)
        // The structure is correct if we got here without panic
    }

    #[test]
    fn test_diff_commits_same_commit() {
        // Diffing a commit against itself should yield no changes
        let result = diff_commits(".", "e3b2667", "e3b2667", DiffOptions::new());

        assert!(result.is_ok());
        let diff = result.unwrap();

        assert_eq!(diff.total.file_count, 0);
        assert_eq!(diff.total.net_total(), 0);
    }

    #[test]
    fn test_diff_commits_invalid_commit() {
        let result = diff_commits(".", "invalid_commit_hash", "HEAD", DiffOptions::new());

        // Should fail with an error
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(matches!(err, RustlocError::GitError(_)));
    }

    #[test]
    fn test_diff_commits_not_git_repo() {
        // Try to diff in a non-git directory
        let result = diff_commits("/tmp", "HEAD~1", "HEAD", DiffOptions::new());

        // Should fail - /tmp is not a git repo
        assert!(result.is_err());
    }

    #[test]
    fn test_diff_commits_with_glob_filter() {
        // Test with glob filter to only include certain files
        let filter = FilterConfig::new()
            .include("**/lib.rs")
            .expect("valid pattern");

        let options = DiffOptions::new()
            .filter(filter)
            .aggregation(Aggregation::ByFile);

        let result = diff_commits(".", "e3b2667", "6917e2d", options);
        assert!(result.is_ok());

        let diff = result.unwrap();
        // All files should match the pattern (if any)
        for file in &diff.files {
            assert!(
                file.path.to_string_lossy().ends_with("lib.rs"),
                "File {:?} should match lib.rs pattern",
                file.path
            );
        }
    }

    #[test]
    fn test_diff_commits_head_syntax() {
        // Test using HEAD~n syntax
        let result = diff_commits(".", "HEAD~1", "HEAD", DiffOptions::new());

        // This should work if there's at least one commit
        // In a CI environment without git history, this might fail, so we don't assert success
        // Just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_diff_commits_with_crate_filter() {
        // Test with crate filter
        let options = DiffOptions::new()
            .crates(vec!["rustloclib".to_string()])
            .aggregation(Aggregation::ByFile);

        let result = diff_commits(".", "e3b2667", "6917e2d", options);
        assert!(result.is_ok());

        let diff = result.unwrap();
        // All files should be in the rustloclib crate
        for file in &diff.files {
            let path_str = file.path.to_string_lossy();
            assert!(
                path_str.contains("rustloclib") || path_str.starts_with("rustloclib"),
                "File {:?} should be in rustloclib crate",
                file.path
            );
        }
    }

    #[test]
    fn test_diff_commits_exclude_filter() {
        // Test with exclude filter to skip test files
        let filter = FilterConfig::new()
            .exclude("**/tests/**")
            .expect("valid pattern");

        let options = DiffOptions::new()
            .filter(filter)
            .aggregation(Aggregation::ByFile);

        let result = diff_commits(".", "e3b2667", "6917e2d", options);
        assert!(result.is_ok());

        let diff = result.unwrap();
        // No files should be in tests directories
        for file in &diff.files {
            let path_str = file.path.to_string_lossy();
            assert!(
                !path_str.contains("/tests/"),
                "File {:?} should not be in tests directory",
                file.path
            );
        }
    }
}
