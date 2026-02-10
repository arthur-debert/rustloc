//! Query set: processed data ready for table rendering.
//!
//! A QuerySet sits between raw counting/diff results and the final table output.
//! It represents data that has been:
//! - Aggregated to the requested level (crate, module, file)
//! - Filtered to include only requested line types
//! - Sorted according to the ordering preference
//!
//! The data pipeline is:
//! 1. Raw Data (CountResult, DiffResult)
//! 2. QuerySet (filtered, aggregated, sorted)
//! 3. LOCTable (formatted strings for display)

use serde::{Deserialize, Serialize};

use crate::data::counter::CountResult;
use crate::data::diff::{DiffResult, LocsDiff};
use crate::data::stats::Locs;

use super::options::{Aggregation, LineTypes, OrderBy, OrderDirection, Ordering};

/// A single item in a query set (one row of data before string formatting).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryItem<T> {
    /// Row label (file path, crate name, module name, etc.)
    pub label: String,
    /// Statistics for this item
    pub stats: T,
}

/// Query set for count results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountQuerySet {
    /// Aggregation level used
    pub aggregation: Aggregation,
    /// Line types included
    pub line_types: LineTypes,
    /// Data rows (filtered and sorted)
    pub items: Vec<QueryItem<Locs>>,
    /// Total across all items
    pub total: Locs,
    /// Number of files analyzed
    pub file_count: usize,
}

/// Query set for diff results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffQuerySet {
    /// Aggregation level used
    pub aggregation: Aggregation,
    /// Line types included
    pub line_types: LineTypes,
    /// Data rows (filtered and sorted)
    pub items: Vec<QueryItem<LocsDiff>>,
    /// Total diff across all items
    pub total: LocsDiff,
    /// Number of files changed
    pub file_count: usize,
    /// Source commit
    pub from_commit: String,
    /// Target commit
    pub to_commit: String,
}

impl CountQuerySet {
    /// Create a QuerySet from a CountResult.
    ///
    /// Applies aggregation level, line type filters, and ordering.
    pub fn from_result(
        result: &CountResult,
        aggregation: Aggregation,
        line_types: LineTypes,
        ordering: Ordering,
    ) -> Self {
        let items = build_count_items(result, &aggregation, &line_types, &ordering);
        let total = result.total.filter(line_types);

        CountQuerySet {
            aggregation,
            line_types,
            items,
            total,
            file_count: result.file_count,
        }
    }
}

/// Compute a relative path label for a file.
/// Returns the path relative to the workspace root, falling back to the full path if strip fails.
fn relative_path_label(path: &std::path::Path, root: &std::path::Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

impl DiffQuerySet {
    /// Create a QuerySet from a DiffResult.
    ///
    /// Applies aggregation level, line type filters, and ordering.
    pub fn from_result(
        result: &DiffResult,
        aggregation: Aggregation,
        line_types: LineTypes,
        ordering: Ordering,
    ) -> Self {
        let items = build_diff_items(result, &aggregation, &line_types, &ordering);
        let total = result.total.filter(line_types);

        DiffQuerySet {
            aggregation,
            line_types,
            items,
            total,
            file_count: result.files.len(),
            from_commit: result.from_commit.clone(),
            to_commit: result.to_commit.clone(),
        }
    }
}

/// Compute filtered total for a Locs struct.
fn locs_filtered_total(locs: &Locs, line_types: &LineTypes) -> u64 {
    let mut total = 0;
    if line_types.code {
        total += locs.code;
    }
    if line_types.tests {
        total += locs.tests;
    }
    if line_types.examples {
        total += locs.examples;
    }
    if line_types.docs {
        total += locs.docs;
    }
    if line_types.comments {
        total += locs.comments;
    }
    if line_types.blanks {
        total += locs.blanks;
    }
    total
}

/// Get sort key for Locs based on OrderBy.
fn count_sort_key(locs: &Locs, order_by: &OrderBy, line_types: &LineTypes) -> u64 {
    match order_by {
        OrderBy::Label => 0, // Label sorting handled separately
        OrderBy::Code => locs.code,
        OrderBy::Tests => locs.tests,
        OrderBy::Examples => locs.examples,
        OrderBy::Docs => locs.docs,
        OrderBy::Comments => locs.comments,
        OrderBy::Blanks => locs.blanks,
        OrderBy::Total => locs_filtered_total(locs, line_types),
    }
}

/// Build query items from CountResult based on aggregation level.
fn build_count_items(
    result: &CountResult,
    aggregation: &Aggregation,
    line_types: &LineTypes,
    ordering: &Ordering,
) -> Vec<QueryItem<Locs>> {
    let mut items: Vec<(String, Locs)> = match aggregation {
        Aggregation::Total => return vec![],
        Aggregation::ByCrate => result
            .crates
            .iter()
            .filter(|c| locs_filtered_total(&c.stats, line_types) > 0)
            .map(|c| (c.name.clone(), c.stats.filter(*line_types)))
            .collect(),
        Aggregation::ByModule => result
            .modules
            .iter()
            .filter(|m| locs_filtered_total(&m.stats, line_types) > 0)
            .map(|m| {
                let label = if m.name.is_empty() {
                    "(root)".to_string()
                } else {
                    m.name.clone()
                };
                (label, m.stats.filter(*line_types))
            })
            .collect(),
        Aggregation::ByFile => result
            .files
            .iter()
            .filter(|f| locs_filtered_total(&f.stats, line_types) > 0)
            .map(|f| {
                (
                    relative_path_label(&f.path, &result.root),
                    f.stats.filter(*line_types),
                )
            })
            .collect(),
    };

    // Sort based on ordering
    match ordering.by {
        OrderBy::Label => {
            items.sort_by(|a, b| a.0.cmp(&b.0));
        }
        _ => {
            items.sort_by(|a, b| {
                let key_a = count_sort_key(&a.1, &ordering.by, line_types);
                let key_b = count_sort_key(&b.1, &ordering.by, line_types);
                key_a.cmp(&key_b)
            });
        }
    }

    // Reverse if descending
    if ordering.direction == OrderDirection::Descending {
        items.reverse();
    }

    // Map to QueryItems
    items
        .into_iter()
        .map(|(label, stats)| QueryItem { label, stats })
        .collect()
}

/// Compute filtered total for a LocsDiff struct.
fn locs_diff_filtered_total(diff: &LocsDiff, line_types: &LineTypes) -> (u64, u64) {
    let added = locs_filtered_total(&diff.added, line_types);
    let removed = locs_filtered_total(&diff.removed, line_types);
    (added, removed)
}

/// Check if a diff has any net change.
fn has_net_change(diff: &LocsDiff, line_types: &LineTypes) -> bool {
    let (added, removed) = locs_diff_filtered_total(diff, line_types);
    added != removed
}

/// Get sort key for LocsDiff based on OrderBy (uses net change).
fn diff_sort_key(diff: &LocsDiff, order_by: &OrderBy, line_types: &LineTypes) -> i64 {
    match order_by {
        OrderBy::Label => 0, // Label sorting handled separately
        OrderBy::Code => diff.net_code(),
        OrderBy::Tests => diff.net_tests(),
        OrderBy::Examples => diff.net_examples(),
        OrderBy::Docs => diff.net_docs(),
        OrderBy::Comments => diff.net_comments(),
        OrderBy::Blanks => diff.net_blanks(),
        OrderBy::Total => {
            let (a, r) = locs_diff_filtered_total(diff, line_types);
            a as i64 - r as i64
        }
    }
}

/// Build query items from DiffResult based on aggregation level.
fn build_diff_items(
    result: &DiffResult,
    aggregation: &Aggregation,
    line_types: &LineTypes,
    ordering: &Ordering,
) -> Vec<QueryItem<LocsDiff>> {
    let mut items: Vec<(String, LocsDiff)> = match aggregation {
        Aggregation::Total => return vec![],
        Aggregation::ByCrate => result
            .crates
            .iter()
            .filter(|c| has_net_change(&c.diff, line_types))
            .map(|c| (c.name.clone(), c.diff.filter(*line_types)))
            .collect(),
        Aggregation::ByModule => return vec![], // Diff doesn't support by-module currently
        Aggregation::ByFile => result
            .files
            .iter()
            .filter(|f| has_net_change(&f.diff, line_types))
            .map(|f| {
                (
                    f.path.to_string_lossy().to_string(),
                    f.diff.filter(*line_types),
                )
            })
            .collect(),
    };

    // Sort based on ordering
    match ordering.by {
        OrderBy::Label => {
            items.sort_by(|a, b| a.0.cmp(&b.0));
        }
        _ => {
            items.sort_by(|a, b| {
                let key_a = diff_sort_key(&a.1, &ordering.by, line_types);
                let key_b = diff_sort_key(&b.1, &ordering.by, line_types);
                key_a.cmp(&key_b)
            });
        }
    }

    // Reverse if descending
    if ordering.direction == OrderDirection::Descending {
        items.reverse();
    }

    // Map to QueryItems
    items
        .into_iter()
        .map(|(label, stats)| QueryItem { label, stats })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::stats::CrateStats;
    use std::path::PathBuf;

    fn sample_locs(code: u64, tests: u64) -> Locs {
        Locs {
            code,
            tests,
            examples: 0,
            docs: 0,
            comments: 0,
            blanks: 0,
            total: code + tests,
        }
    }

    fn sample_count_result() -> CountResult {
        CountResult {
            root: PathBuf::from("/workspace"),
            file_count: 4,
            total: sample_locs(200, 100),
            crates: vec![
                CrateStats {
                    name: "alpha".to_string(),
                    path: PathBuf::from("/alpha"),
                    stats: sample_locs(50, 25),
                    files: vec![],
                },
                CrateStats {
                    name: "beta".to_string(),
                    path: PathBuf::from("/beta"),
                    stats: sample_locs(150, 75),
                    files: vec![],
                },
            ],
            files: vec![],
            modules: vec![],
        }
    }

    #[test]
    fn test_count_queryset_by_crate() {
        let result = sample_count_result();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        );

        assert_eq!(qs.items.len(), 2);
        assert_eq!(qs.file_count, 4);
        // Default ordering is by label ascending
        assert_eq!(qs.items[0].label, "alpha");
        assert_eq!(qs.items[1].label, "beta");
    }

    #[test]
    fn test_count_queryset_ordering_by_code() {
        let result = sample_count_result();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::by_code(), // Descending by default
        );

        // beta has 150 code, alpha has 50
        assert_eq!(qs.items[0].label, "beta");
        assert_eq!(qs.items[0].stats.code, 150);
        assert_eq!(qs.items[1].label, "alpha");
        assert_eq!(qs.items[1].stats.code, 50);
    }

    #[test]
    fn test_count_queryset_total_aggregation() {
        let result = sample_count_result();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::Total,
            LineTypes::everything(),
            Ordering::default(),
        );

        // Total aggregation returns no items (just uses total)
        assert_eq!(qs.items.len(), 0);
        assert_eq!(qs.total.code, 200);
        assert_eq!(qs.total.tests, 100);
    }

    #[test]
    fn test_count_queryset_filtered_line_types() {
        let result = sample_count_result();
        let line_types = LineTypes::new().with_code();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            line_types,
            Ordering::default(),
        );

        // Stats should be filtered
        assert_eq!(qs.items[0].stats.code, 50);
        assert_eq!(qs.items[0].stats.tests, 0); // Filtered out
    }

    #[test]
    fn test_count_queryset_by_file_relative_paths() {
        use crate::data::stats::FileStats;

        let result = CountResult {
            root: PathBuf::from("/workspace"),
            file_count: 2,
            total: sample_locs(100, 50),
            crates: vec![],
            files: vec![
                FileStats::new(PathBuf::from("/workspace/src/main.rs"), sample_locs(50, 25)),
                FileStats::new(
                    PathBuf::from("/workspace/crate-a/src/lib.rs"),
                    sample_locs(50, 25),
                ),
            ],
            modules: vec![],
        };

        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByFile,
            LineTypes::everything(),
            Ordering::default(),
        );

        // Labels should be relative to workspace root
        assert_eq!(qs.items.len(), 2);
        assert!(qs.items.iter().any(|item| item.label == "src/main.rs"));
        assert!(qs
            .items
            .iter()
            .any(|item| item.label == "crate-a/src/lib.rs"));
    }
}
