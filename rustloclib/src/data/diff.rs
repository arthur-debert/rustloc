//! Git diff analysis for LOC changes between commits and working directory.
//!
//! This module provides functionality to compute LOC differences between:
//! - Two git commits (using `diff_commits`)
//! - Working directory and HEAD or index (using `diff_workdir`)
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

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::RustlocError;
use crate::query::options::{Aggregation, LineTypes};
use crate::source::filter::FilterConfig;
use crate::source::workspace::WorkspaceInfo;
use crate::Result;

use super::stats::Locs;
use super::visitor::{gather_stats, VisitorContext};

/// Lines of code diff (added vs removed).
///
/// Tracks additions and removals for each of the 6 line types.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocsDiff {
    /// Lines added
    pub added: Locs,
    /// Lines removed
    pub removed: Locs,
}

impl LocsDiff {
    /// Create a new empty diff.
    pub fn new() -> Self {
        Self::default()
    }

    /// Net change for code lines.
    pub fn net_code(&self) -> i64 {
        self.added.code as i64 - self.removed.code as i64
    }

    /// Net change for test lines.
    pub fn net_tests(&self) -> i64 {
        self.added.tests as i64 - self.removed.tests as i64
    }

    /// Net change for example lines.
    pub fn net_examples(&self) -> i64 {
        self.added.examples as i64 - self.removed.examples as i64
    }

    /// Net change for doc comment lines.
    pub fn net_docs(&self) -> i64 {
        self.added.docs as i64 - self.removed.docs as i64
    }

    /// Net change for regular comment lines.
    pub fn net_comments(&self) -> i64 {
        self.added.comments as i64 - self.removed.comments as i64
    }

    /// Net change for blank lines.
    pub fn net_blanks(&self) -> i64 {
        self.added.blanks as i64 - self.removed.blanks as i64
    }

    /// Net change for total lines.
    pub fn net_total(&self) -> i64 {
        self.added.total() as i64 - self.removed.total() as i64
    }

    /// Return a filtered copy with only the specified line types included.
    pub fn filter(&self, types: LineTypes) -> Self {
        Self {
            added: self.added.filter(types),
            removed: self.removed.filter(types),
        }
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

/// Diff statistics for a single file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiffStats {
    /// Path to the file (relative to repo root).
    pub path: PathBuf,
    /// The type of change.
    pub change_type: FileChangeType,
    /// LOC diff for this file.
    pub diff: LocsDiff,
}

impl FileDiffStats {
    /// Return a filtered copy with only the specified line types included.
    pub fn filter(&self, types: LineTypes) -> Self {
        Self {
            path: self.path.clone(),
            change_type: self.change_type,
            diff: self.diff.filter(types),
        }
    }
}

/// Type of file change in the diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileChangeType {
    /// File was added.
    Added,
    /// File was deleted.
    Deleted,
    /// File was modified.
    Modified,
}

/// Diff statistics for a crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrateDiffStats {
    /// Name of the crate.
    pub name: String,
    /// Root path of the crate.
    pub path: PathBuf,
    /// Aggregated LOC diff.
    pub diff: LocsDiff,
    /// Per-file diff (optional, for detailed output).
    pub files: Vec<FileDiffStats>,
}

impl CrateDiffStats {
    /// Create new crate diff stats.
    pub fn new(name: String, path: PathBuf) -> Self {
        Self {
            name,
            path,
            diff: LocsDiff::new(),
            files: Vec::new(),
        }
    }

    /// Add file diff to this crate.
    pub fn add_file(&mut self, file_diff: FileDiffStats) {
        self.diff += file_diff.diff;
        self.files.push(file_diff);
    }

    /// Return a filtered copy with only the specified line types included.
    pub fn filter(&self, types: LineTypes) -> Self {
        Self {
            name: self.name.clone(),
            path: self.path.clone(),
            diff: self.diff.filter(types),
            files: self.files.iter().map(|f| f.filter(types)).collect(),
        }
    }
}

/// Result of a diff operation between two commits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffResult {
    /// Root path of the repository analyzed.
    pub root: PathBuf,
    /// Base commit (from).
    pub from_commit: String,
    /// Target commit (to).
    pub to_commit: String,
    /// Total diff across all files.
    pub total: LocsDiff,
    /// Per-crate diff breakdown.
    pub crates: Vec<CrateDiffStats>,
    /// Per-file diff (optional, for detailed output).
    pub files: Vec<FileDiffStats>,
    /// Lines added in non-Rust files.
    #[serde(default)]
    pub non_rust_added: u64,
    /// Lines removed in non-Rust files.
    #[serde(default)]
    pub non_rust_removed: u64,
}

impl DiffResult {
    /// Return a filtered copy with only the specified line types included.
    pub fn filter(&self, types: LineTypes) -> Self {
        Self {
            root: self.root.clone(),
            from_commit: self.from_commit.clone(),
            to_commit: self.to_commit.clone(),
            total: self.total.filter(types),
            crates: self.crates.iter().map(|c| c.filter(types)).collect(),
            files: self.files.iter().map(|f| f.filter(types)).collect(),
            non_rust_added: self.non_rust_added,
            non_rust_removed: self.non_rust_removed,
        }
    }
}

/// Options for diff computation.
#[derive(Debug, Clone)]
pub struct DiffOptions {
    /// Crate names to include (empty = all crates).
    pub crate_filter: Vec<String>,
    /// File filter configuration.
    pub file_filter: FilterConfig,
    /// Aggregation level for results.
    pub aggregation: Aggregation,
    /// Which line types to include in results.
    pub line_types: LineTypes,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            crate_filter: Vec::new(),
            file_filter: FilterConfig::new(),
            aggregation: Aggregation::Total,
            line_types: LineTypes::default(),
        }
    }
}

impl DiffOptions {
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
    pub fn filter(mut self, config: FilterConfig) -> Self {
        self.file_filter = config;
        self
    }

    /// Set aggregation level.
    pub fn aggregation(mut self, level: Aggregation) -> Self {
        self.aggregation = level;
        self
    }

    /// Set which line types to include.
    pub fn line_types(mut self, types: LineTypes) -> Self {
        self.line_types = types;
        self
    }
}

/// Mode for working directory diff.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkdirDiffMode {
    /// Compare HEAD with working directory (all uncommitted changes).
    /// This is equivalent to `git diff HEAD`.
    #[default]
    All,
    /// Compare HEAD with the staging area/index (staged changes only).
    /// This is equivalent to `git diff --cached` or `git diff --staged`.
    Staged,
}

/// Compute LOC diff for working directory changes.
pub fn diff_workdir(
    repo_path: impl AsRef<Path>,
    mode: WorkdirDiffMode,
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

    // Get HEAD commit and its tree
    let head_commit = repo
        .head_commit()
        .map_err(|e| RustlocError::GitError(format!("Failed to get HEAD commit: {}", e)))?;

    let head_tree = head_commit
        .tree()
        .map_err(|e| RustlocError::GitError(format!("Failed to get HEAD tree: {}", e)))?;

    // Get the index
    let index = repo
        .index()
        .map_err(|e| RustlocError::GitError(format!("Failed to read index: {}", e)))?;

    // Collect changes based on mode
    let (changes, non_rust_added, non_rust_removed) = match mode {
        WorkdirDiffMode::Staged => collect_staged_changes(&repo, &head_tree, &index)?,
        WorkdirDiffMode::All => collect_workdir_changes(&repo, &head_tree, &repo_root)?,
    };

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
    let mut total = LocsDiff::new();
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

        // Apply glob filter
        if !options.file_filter.matches(&path) {
            continue;
        }

        // Determine which crate this file belongs to
        let crate_info = filtered_workspace
            .as_ref()
            .and_then(|ws| ws.crate_for_path(&path));

        // If crate filter is active and file doesn't belong to a filtered crate, skip
        if !options.crate_filter.is_empty() && crate_info.is_none() {
            continue;
        }

        // Compute file diff
        let file_diff = compute_workdir_file_diff(&change, &path)?;

        // Aggregate into total
        total += file_diff.diff;

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
                    crate_stats_entry.diff += file_diff.diff;
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
    let (from_label, to_label) = match mode {
        WorkdirDiffMode::All => ("HEAD", "working tree"),
        WorkdirDiffMode::Staged => ("HEAD", "index"),
    };

    let result = DiffResult {
        root: repo_root,
        from_commit: from_label.to_string(),
        to_commit: to_label.to_string(),
        total,
        crates,
        files,
        non_rust_added,
        non_rust_removed,
    };

    Ok(result.filter(options.line_types))
}

/// Internal representation of a working directory file change
struct WorkdirFileChange {
    path: PathBuf,
    change_type: FileChangeType,
    old_content: Option<String>,
    new_content: Option<String>,
}

/// Collect staged changes (HEAD vs index)
fn collect_staged_changes(
    repo: &gix::Repository,
    head_tree: &gix::Tree<'_>,
    index: &gix::worktree::Index,
) -> Result<(Vec<WorkdirFileChange>, u64, u64)> {
    use std::collections::HashSet;

    let mut changes = Vec::new();
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    let mut non_rust_added: u64 = 0;
    let mut non_rust_removed: u64 = 0;

    // Build a map of HEAD tree entries
    let mut head_entries: HashMap<PathBuf, gix::ObjectId> = HashMap::new();
    collect_tree_entries(repo, head_tree, PathBuf::new(), &mut head_entries)?;

    // Check each entry in the index against HEAD
    for entry in index.entries() {
        let path = PathBuf::from(gix::path::from_bstr(entry.path(index)));

        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            // Track non-Rust file line changes
            let index_oid = entry.id;
            if let Some(&head_oid) = head_entries.get(&path) {
                if head_oid != index_oid {
                    let old_lines = count_lines(&read_blob(repo, head_oid)?);
                    let new_lines = count_lines(&read_blob(repo, index_oid)?);
                    non_rust_added += new_lines.saturating_sub(old_lines);
                    non_rust_removed += old_lines.saturating_sub(new_lines);
                }
            } else {
                non_rust_added += count_lines(&read_blob(repo, index_oid)?);
            }
            seen_paths.insert(path);
            continue;
        }

        seen_paths.insert(path.clone());
        let index_oid = entry.id;

        if let Some(&head_oid) = head_entries.get(&path) {
            if head_oid != index_oid {
                let old_content = read_blob(repo, head_oid)?;
                let new_content = read_blob(repo, index_oid)?;
                changes.push(WorkdirFileChange {
                    path,
                    change_type: FileChangeType::Modified,
                    old_content: Some(old_content),
                    new_content: Some(new_content),
                });
            }
        } else {
            let new_content = read_blob(repo, index_oid)?;
            changes.push(WorkdirFileChange {
                path,
                change_type: FileChangeType::Added,
                old_content: None,
                new_content: Some(new_content),
            });
        }
    }

    // Check for deleted files
    for (path, head_oid) in head_entries {
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            if !seen_paths.contains(&path) {
                non_rust_removed += count_lines(&read_blob(repo, head_oid)?);
            }
            continue;
        }
        if !seen_paths.contains(&path) {
            let old_content = read_blob(repo, head_oid)?;
            changes.push(WorkdirFileChange {
                path,
                change_type: FileChangeType::Deleted,
                old_content: Some(old_content),
                new_content: None,
            });
        }
    }

    Ok((changes, non_rust_added, non_rust_removed))
}

/// Collect all uncommitted changes (HEAD vs working directory)
fn collect_workdir_changes(
    repo: &gix::Repository,
    head_tree: &gix::Tree<'_>,
    repo_root: &Path,
) -> Result<(Vec<WorkdirFileChange>, u64, u64)> {
    use std::collections::HashSet;

    let mut changes = Vec::new();
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    let mut non_rust_added: u64 = 0;
    let mut non_rust_removed: u64 = 0;

    // Build a map of HEAD tree entries
    let mut head_entries: HashMap<PathBuf, gix::ObjectId> = HashMap::new();
    collect_tree_entries(repo, head_tree, PathBuf::new(), &mut head_entries)?;

    // Get tracked files from index
    let index = repo
        .index()
        .map_err(|e| RustlocError::GitError(format!("Failed to read index: {}", e)))?;
    let tracked_paths: HashSet<PathBuf> = index
        .entries()
        .iter()
        .map(|e| PathBuf::from(gix::path::from_bstr(e.path(&index))))
        .collect();

    // Walk the working directory
    let walker = walkdir::WalkDir::new(repo_root)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_str();
            name.is_none_or(|s| s != ".git" && s != "target")
        });

    for entry in walker.filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }

        let abs_path = entry.path();
        let rel_path = abs_path
            .strip_prefix(repo_root)
            .unwrap_or(abs_path)
            .to_path_buf();

        // Skip untracked files
        if !tracked_paths.contains(&rel_path) && !head_entries.contains_key(&rel_path) {
            continue;
        }

        if abs_path.extension().and_then(|e| e.to_str()) != Some("rs") {
            // Track non-Rust file line changes
            seen_paths.insert(rel_path.clone());
            let workdir_content = match std::fs::read_to_string(abs_path) {
                Ok(content) => content,
                Err(_) => continue,
            };
            let new_lines = count_lines(&workdir_content);
            if let Some(&head_oid) = head_entries.get(&rel_path) {
                let old_lines = count_lines(&read_blob(repo, head_oid)?);
                if old_lines != new_lines {
                    non_rust_added += new_lines.saturating_sub(old_lines);
                    non_rust_removed += old_lines.saturating_sub(new_lines);
                }
            } else {
                non_rust_added += new_lines;
            }
            continue;
        }

        seen_paths.insert(rel_path.clone());

        let workdir_content = match std::fs::read_to_string(abs_path) {
            Ok(content) => content,
            Err(_) => continue,
        };

        if let Some(&head_oid) = head_entries.get(&rel_path) {
            let head_content = read_blob(repo, head_oid)?;
            if head_content != workdir_content {
                changes.push(WorkdirFileChange {
                    path: rel_path,
                    change_type: FileChangeType::Modified,
                    old_content: Some(head_content),
                    new_content: Some(workdir_content),
                });
            }
        } else {
            changes.push(WorkdirFileChange {
                path: rel_path,
                change_type: FileChangeType::Added,
                old_content: None,
                new_content: Some(workdir_content),
            });
        }
    }

    // Check for deleted files
    for (path, head_oid) in head_entries {
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            if !seen_paths.contains(&path) {
                non_rust_removed += count_lines(&read_blob(repo, head_oid)?);
            }
            continue;
        }
        if !seen_paths.contains(&path) {
            let old_content = read_blob(repo, head_oid)?;
            changes.push(WorkdirFileChange {
                path,
                change_type: FileChangeType::Deleted,
                old_content: Some(old_content),
                new_content: None,
            });
        }
    }

    Ok((changes, non_rust_added, non_rust_removed))
}

/// Recursively collect all blob entries from a tree
fn collect_tree_entries(
    repo: &gix::Repository,
    tree: &gix::Tree<'_>,
    prefix: PathBuf,
    entries: &mut HashMap<PathBuf, gix::ObjectId>,
) -> Result<()> {
    for entry in tree.iter() {
        let entry = entry
            .map_err(|e| RustlocError::GitError(format!("Failed to read tree entry: {}", e)))?;

        let name = gix::path::from_bstr(entry.filename());
        let path = prefix.join(name);

        if entry.mode().is_blob() {
            entries.insert(path, entry.oid().to_owned());
        } else if entry.mode().is_tree() {
            let subtree = repo
                .find_object(entry.oid())
                .map_err(|e| RustlocError::GitError(format!("Failed to find tree: {}", e)))?
                .try_into_tree()
                .map_err(|_| RustlocError::GitError("Object is not a tree".to_string()))?;
            collect_tree_entries(repo, &subtree, path, entries)?;
        }
    }
    Ok(())
}

/// Compute the LOC diff for a working directory file change
fn compute_workdir_file_diff(change: &WorkdirFileChange, path: &Path) -> Result<FileDiffStats> {
    let context = VisitorContext::from_file_path(path);

    let (old_stats, new_stats) = match change.change_type {
        FileChangeType::Added => {
            let stats = gather_stats(change.new_content.as_ref().unwrap(), context);
            (Locs::new(), stats)
        }
        FileChangeType::Deleted => {
            let stats = gather_stats(change.old_content.as_ref().unwrap(), context);
            (stats, Locs::new())
        }
        FileChangeType::Modified => {
            let old_stats = gather_stats(change.old_content.as_ref().unwrap(), context);
            let new_stats = gather_stats(change.new_content.as_ref().unwrap(), context);
            (old_stats, new_stats)
        }
    };

    let diff = compute_locs_diff(&old_stats, &new_stats);

    Ok(FileDiffStats {
        path: path.to_path_buf(),
        change_type: change.change_type,
        diff,
    })
}

/// Compute LOC diff between two git commits.
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

    // Try to discover workspace info
    let workspace = WorkspaceInfo::discover(&repo_root).ok();

    // Apply crate filter
    let filtered_workspace = workspace.as_ref().map(|ws| {
        if options.crate_filter.is_empty() {
            ws.clone()
        } else {
            let names: Vec<&str> = options.crate_filter.iter().map(|s| s.as_str()).collect();
            ws.filter_by_names(&names)
        }
    });

    // Process changes
    let mut total = LocsDiff::new();
    let mut files = Vec::new();
    let mut crate_stats: HashMap<String, CrateDiffStats> = HashMap::new();
    let mut non_rust_added: u64 = 0;
    let mut non_rust_removed: u64 = 0;

    let include_files = matches!(options.aggregation, Aggregation::ByFile);
    let include_crates = matches!(
        options.aggregation,
        Aggregation::ByCrate | Aggregation::ByFile
    );

    for change in changes {
        let path = change.path.clone();

        // Track non-Rust file line changes
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            let old_lines = change
                .old_oid
                .and_then(|oid| read_blob(&repo, oid).ok().map(|c| count_lines(&c)))
                .unwrap_or(0);
            let new_lines = change
                .new_oid
                .and_then(|oid| read_blob(&repo, oid).ok().map(|c| count_lines(&c)))
                .unwrap_or(0);
            non_rust_added += new_lines.saturating_sub(old_lines);
            non_rust_removed += old_lines.saturating_sub(new_lines);
            continue;
        }

        if !options.file_filter.matches(&path) {
            continue;
        }

        let crate_info = filtered_workspace
            .as_ref()
            .and_then(|ws| ws.crate_for_path(&path));

        if !options.crate_filter.is_empty() && crate_info.is_none() {
            continue;
        }

        let file_diff = compute_file_diff(&repo, &change, &path)?;

        total += file_diff.diff;

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
                    crate_stats_entry.diff += file_diff.diff;
                }
            }
        }

        if include_files {
            files.push(file_diff);
        }
    }

    let crates: Vec<CrateDiffStats> = crate_stats.into_values().collect();

    let result = DiffResult {
        root: repo_root,
        from_commit: from.to_string(),
        to_commit: to.to_string(),
        total,
        crates,
        files,
        non_rust_added,
        non_rust_removed,
    };

    Ok(result.filter(options.line_types))
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
                Change::Rewrite { .. } => None,
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
    let context = VisitorContext::from_file_path(path);

    let (old_stats, new_stats) = match change.change_type {
        FileChangeType::Added => {
            let content = read_blob(repo, change.new_oid.unwrap())?;
            let stats = gather_stats(&content, context);
            (Locs::new(), stats)
        }
        FileChangeType::Deleted => {
            let content = read_blob(repo, change.old_oid.unwrap())?;
            let stats = gather_stats(&content, context);
            (stats, Locs::new())
        }
        FileChangeType::Modified => {
            let old_content = read_blob(repo, change.old_oid.unwrap())?;
            let new_content = read_blob(repo, change.new_oid.unwrap())?;
            let old_stats = gather_stats(&old_content, context);
            let new_stats = gather_stats(&new_content, context);
            (old_stats, new_stats)
        }
    };

    let diff = compute_locs_diff(&old_stats, &new_stats);

    Ok(FileDiffStats {
        path: path.to_path_buf(),
        change_type: change.change_type,
        diff,
    })
}

/// Compute the diff between two Locs
fn compute_locs_diff(old: &Locs, new: &Locs) -> LocsDiff {
    LocsDiff {
        added: Locs {
            code: new.code.saturating_sub(old.code),
            tests: new.tests.saturating_sub(old.tests),
            examples: new.examples.saturating_sub(old.examples),
            docs: new.docs.saturating_sub(old.docs),
            comments: new.comments.saturating_sub(old.comments),
            blanks: new.blanks.saturating_sub(old.blanks),
            total: new.total.saturating_sub(old.total),
        },
        removed: Locs {
            code: old.code.saturating_sub(new.code),
            tests: old.tests.saturating_sub(new.tests),
            examples: old.examples.saturating_sub(new.examples),
            docs: old.docs.saturating_sub(new.docs),
            comments: old.comments.saturating_sub(new.comments),
            blanks: old.blanks.saturating_sub(new.blanks),
            total: old.total.saturating_sub(new.total),
        },
    }
}

/// Count lines in a text string.
fn count_lines(content: &str) -> u64 {
    content.lines().count() as u64
}

/// Read a blob's content as a UTF-8 string
fn read_blob(repo: &gix::Repository, oid: gix::ObjectId) -> Result<String> {
    let object = repo
        .find_object(oid)
        .map_err(|e| RustlocError::GitError(format!("Failed to find object {}: {}", oid, e)))?;

    let blob = object
        .try_into_blob()
        .map_err(|_| RustlocError::GitError(format!("Object {} is not a blob", oid)))?;

    String::from_utf8(blob.data.to_vec())
        .or_else(|e| Ok(String::from_utf8_lossy(&e.into_bytes()).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locs_diff_default() {
        let diff = LocsDiff::new();
        assert_eq!(diff.added.code, 0);
        assert_eq!(diff.removed.code, 0);
        assert_eq!(diff.net_code(), 0);
    }

    #[test]
    fn test_locs_diff_net() {
        let diff = LocsDiff {
            added: Locs {
                code: 100,
                tests: 50,
                examples: 20,
                docs: 10,
                comments: 5,
                blanks: 15,
                total: 200,
            },
            removed: Locs {
                code: 30,
                tests: 20,
                examples: 10,
                docs: 2,
                comments: 1,
                blanks: 5,
                total: 68,
            },
        };

        assert_eq!(diff.net_code(), 70);
        assert_eq!(diff.net_tests(), 30);
        assert_eq!(diff.net_examples(), 10);
        assert_eq!(diff.net_docs(), 8);
        assert_eq!(diff.net_comments(), 4);
        assert_eq!(diff.net_blanks(), 10);
        assert_eq!(diff.net_total(), 132); // 200 - 68
    }

    #[test]
    fn test_locs_diff_add() {
        let a = LocsDiff {
            added: Locs {
                code: 10,
                tests: 5,
                examples: 2,
                docs: 1,
                comments: 1,
                blanks: 1,
                total: 20,
            },
            removed: Locs {
                code: 5,
                tests: 2,
                examples: 1,
                docs: 0,
                comments: 0,
                blanks: 0,
                total: 8,
            },
        };
        let b = LocsDiff {
            added: Locs {
                code: 20,
                tests: 10,
                examples: 4,
                docs: 2,
                comments: 2,
                blanks: 2,
                total: 40,
            },
            removed: Locs {
                code: 10,
                tests: 5,
                examples: 2,
                docs: 1,
                comments: 1,
                blanks: 1,
                total: 20,
            },
        };

        let sum = a + b;
        assert_eq!(sum.added.code, 30);
        assert_eq!(sum.removed.code, 15);
        assert_eq!(sum.net_code(), 15);
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
    fn test_compute_locs_diff_added_file() {
        let old = Locs::new();
        let new = Locs {
            code: 100,
            tests: 0,
            examples: 0,
            docs: 10,
            comments: 5,
            blanks: 20,
            total: 135,
        };

        let diff = compute_locs_diff(&old, &new);
        assert_eq!(diff.added.code, 100);
        assert_eq!(diff.removed.code, 0);
        assert_eq!(diff.added.docs, 10);
        assert_eq!(diff.removed.docs, 0);
    }

    #[test]
    fn test_compute_locs_diff_deleted_file() {
        let old = Locs {
            code: 0,
            tests: 50,
            examples: 0,
            docs: 5,
            comments: 2,
            blanks: 10,
            total: 67,
        };
        let new = Locs::new();

        let diff = compute_locs_diff(&old, &new);
        assert_eq!(diff.added.tests, 0);
        assert_eq!(diff.removed.tests, 50);
    }

    #[test]
    fn test_compute_locs_diff_modified_file() {
        let old = Locs {
            code: 100,
            tests: 0,
            examples: 0,
            docs: 10,
            comments: 5,
            blanks: 20,
            total: 135,
        };
        let new = Locs {
            code: 120,
            tests: 0,
            examples: 0,
            docs: 8,
            comments: 5,
            blanks: 25,
            total: 158,
        };

        let diff = compute_locs_diff(&old, &new);
        assert_eq!(diff.added.code, 20);
        assert_eq!(diff.removed.code, 0);
        assert_eq!(diff.added.docs, 0);
        assert_eq!(diff.removed.docs, 2);
    }

    #[test]
    fn test_workdir_diff_mode_default() {
        assert_eq!(WorkdirDiffMode::default(), WorkdirDiffMode::All);
    }

    #[test]
    fn test_diff_commits_same_commit() {
        let result = diff_commits(".", "e3b2667", "e3b2667", DiffOptions::new());
        assert!(result.is_ok());
        let diff = result.unwrap();
        assert_eq!(diff.total.net_total(), 0);
    }

    #[test]
    fn test_diff_commits_invalid_commit() {
        let result = diff_commits(".", "invalid_commit_hash", "HEAD", DiffOptions::new());
        assert!(result.is_err());
    }
}
