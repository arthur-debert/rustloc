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

use std::collections::HashMap;

use crate::data::counter::{compute_module_name, CountResult};
use crate::data::diff::{DiffResult, LocsDiff};
use crate::data::stats::Locs;

use super::options::{Aggregation, Field, LineTypes, OrderBy, OrderDirection, Ordering, Predicate};

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
    /// Data rows (filtered and sorted; possibly truncated by `top`)
    pub items: Vec<QueryItem<Locs>>,
    /// Total across all items in the underlying data set (not affected by `top` or `filter`)
    pub total: Locs,
    /// Number of files analyzed
    pub file_count: usize,
    /// Count of rows before any user-driven reduction (`top` or `filter`).
    /// Equals `items.len()` unless one of those was applied.
    #[serde(default)]
    pub total_items: usize,
    /// True iff `top` was applied. Distinguishes top-truncation from
    /// filter-elimination so the footer can render "top X of Y" vs plain
    /// "X of Y" appropriately.
    #[serde(default)]
    pub top_applied: bool,
}

/// Query set for diff results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffQuerySet {
    /// Aggregation level used
    pub aggregation: Aggregation,
    /// Line types included
    pub line_types: LineTypes,
    /// Data rows (filtered and sorted; possibly truncated by `top`)
    pub items: Vec<QueryItem<LocsDiff>>,
    /// Total diff across all items in the underlying data set (not affected by `top` or `filter`)
    pub total: LocsDiff,
    /// Number of files changed
    pub file_count: usize,
    /// Source commit
    pub from_commit: String,
    /// Target commit
    pub to_commit: String,
    /// Lines added in non-Rust files
    #[serde(default)]
    pub non_rust_added: u64,
    /// Lines removed in non-Rust files
    #[serde(default)]
    pub non_rust_removed: u64,
    /// Count of rows before any user-driven reduction (`top` or `filter`).
    /// Equals `items.len()` unless one of those was applied.
    #[serde(default)]
    pub total_items: usize,
    /// True iff `top` was applied. See [`CountQuerySet::top_applied`].
    #[serde(default)]
    pub top_applied: bool,
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
        let total_items = items.len();

        CountQuerySet {
            aggregation,
            line_types,
            items,
            total,
            file_count: result.file_count,
            total_items,
            top_applied: false,
        }
    }

    /// Keep only the first `n` items after ordering.
    ///
    /// Applied after `from_result` so the truncation runs on already-sorted
    /// rows. With `n` larger than the current row count this is a no-op.
    /// `total` and `file_count` are intentionally not changed — the displayed
    /// rows are a slice, but the underlying counts still describe the full
    /// data set.
    #[must_use]
    pub fn top(mut self, n: usize) -> Self {
        self.items.truncate(n);
        self.top_applied = true;
        self
    }

    /// Keep only items satisfying every predicate (AND-combined).
    ///
    /// `Field::Total` honors the active `LineTypes` — i.e. the total used
    /// here is the same one that `Ordering` with `OrderBy::Total` uses, so
    /// `--type code,tests --total-gte 1000` filters on the sum of the two
    /// enabled types, not on the precomputed `Locs::total`.
    ///
    /// `total_items` is intentionally NOT updated — it tracks the row count
    /// before any *user-driven* truncation (`top` and `filter`), so the
    /// footer can render "top X of Y" honestly even when the visible slice
    /// is filtered down. The full data set is still summarized in `total`.
    #[must_use]
    pub fn filter(mut self, preds: &[Predicate]) -> Self {
        if preds.is_empty() {
            return self;
        }
        let line_types = self.line_types;
        self.items.retain(|item| {
            preds
                .iter()
                .all(|p| matches_locs(p, &item.stats, &line_types))
        });
        self
    }
}

/// Saturating `u64 -> i64`. Values past `i64::MAX` clamp to `i64::MAX`
/// rather than wrapping into the negative range. The cap is ~9.2 × 10^18 —
/// well past any realistic LOC count, but better to clamp than to silently
/// produce wrong filter results if a caller passes a huge value.
fn u64_to_i64_sat(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// Saturating signed subtraction `u64 - u64 -> i64`. Used for the diff
/// `Total` case where each side is a u64 sum and the difference can exceed
/// `i64` magnitude in pathological inputs.
fn u64_sub_i64_sat(a: u64, b: u64) -> i64 {
    let a = u64_to_i64_sat(a);
    let b = u64_to_i64_sat(b);
    a.saturating_sub(b)
}

/// Resolve the integer value a predicate's field refers to in a `Locs`.
///
/// `Field::Total` follows `OrderBy::Total` semantics: the sum of currently-
/// enabled line types, not `Locs::total`.
fn locs_field_value(locs: &Locs, field: Field, line_types: &LineTypes) -> i64 {
    let v: u64 = match field {
        Field::Code => locs.code,
        Field::Tests => locs.tests,
        Field::Examples => locs.examples,
        Field::Docs => locs.docs,
        Field::Comments => locs.comments,
        Field::Blanks => locs.blanks,
        Field::Total => locs_filtered_total(locs, line_types),
    };
    u64_to_i64_sat(v)
}

fn matches_locs(pred: &Predicate, locs: &Locs, line_types: &LineTypes) -> bool {
    let lhs = locs_field_value(locs, pred.field, line_types);
    pred.op.evaluate(lhs, u64_to_i64_sat(pred.value))
}

/// Resolve the (signed) net diff value a predicate's field refers to.
fn diff_field_value(diff: &LocsDiff, field: Field, line_types: &LineTypes) -> i64 {
    match field {
        Field::Code => diff.net_code(),
        Field::Tests => diff.net_tests(),
        Field::Examples => diff.net_examples(),
        Field::Docs => diff.net_docs(),
        Field::Comments => diff.net_comments(),
        Field::Blanks => diff.net_blanks(),
        Field::Total => {
            let (a, r) = locs_diff_filtered_total(diff, line_types);
            u64_sub_i64_sat(a, r)
        }
    }
}

fn matches_diff(pred: &Predicate, diff: &LocsDiff, line_types: &LineTypes) -> bool {
    let lhs = diff_field_value(diff, pred.field, line_types);
    pred.op.evaluate(lhs, u64_to_i64_sat(pred.value))
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
        let total_items = items.len();

        DiffQuerySet {
            aggregation,
            line_types,
            items,
            total,
            file_count: result.files.len(),
            from_commit: result.from_commit.clone(),
            to_commit: result.to_commit.clone(),
            non_rust_added: result.non_rust_added,
            non_rust_removed: result.non_rust_removed,
            total_items,
            top_applied: false,
        }
    }

    /// Keep only the first `n` items after ordering.
    ///
    /// See [`CountQuerySet::top`] for semantics — `total` and `file_count`
    /// describe the full data set, not the truncated slice.
    #[must_use]
    pub fn top(mut self, n: usize) -> Self {
        self.items.truncate(n);
        self.top_applied = true;
        self
    }

    /// Keep only items satisfying every predicate (AND-combined).
    ///
    /// Comparisons are made against the **net** change for each field
    /// (added − removed), so `--code-lt 0` matches files with net code
    /// removed, and `--code-gte 100` matches files where added − removed ≥ 100.
    /// `Field::Total` honors the active `LineTypes`. See [`CountQuerySet::filter`]
    /// for the rationale on why `total_items` isn't updated.
    #[must_use]
    pub fn filter(mut self, preds: &[Predicate]) -> Self {
        if preds.is_empty() {
            return self;
        }
        let line_types = self.line_types;
        self.items.retain(|item| {
            preds
                .iter()
                .all(|p| matches_diff(p, &item.stats, &line_types))
        });
        self
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
            .map(|c| (c.name.clone(), c.stats.filter(*line_types)))
            .collect(),
        Aggregation::ByModule => result
            .modules
            .iter()
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
            .map(|c| (c.name.clone(), c.diff.filter(*line_types)))
            .collect(),
        Aggregation::ByModule => {
            let mut module_map: HashMap<String, LocsDiff> = HashMap::new();
            for crate_diff in &result.crates {
                let src_root = crate_diff.path.join("src");
                let effective_root = if src_root.exists() {
                    src_root
                } else {
                    crate_diff.path.clone()
                };
                for file in &crate_diff.files {
                    let abs_path = if file.path.is_absolute() {
                        file.path.clone()
                    } else {
                        result.root.join(&file.path)
                    };
                    let local_module = compute_module_name(&abs_path, &effective_root);
                    let full_name = if local_module.is_empty() {
                        crate_diff.name.clone()
                    } else {
                        format!("{}::{}", crate_diff.name, local_module)
                    };
                    let entry = module_map.entry(full_name).or_default();
                    *entry += file.diff.filter(*line_types);
                }
            }
            module_map.into_iter().collect()
        }
        Aggregation::ByFile => result
            .files
            .iter()
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
    use crate::query::options::{Field, Op, Predicate};
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

    fn sample_count_result_three_crates() -> CountResult {
        CountResult {
            root: PathBuf::from("/workspace"),
            file_count: 6,
            total: sample_locs(600, 300),
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
                CrateStats {
                    name: "gamma".to_string(),
                    path: PathBuf::from("/gamma"),
                    stats: sample_locs(400, 200),
                    files: vec![],
                },
            ],
            files: vec![],
            modules: vec![],
        }
    }

    #[test]
    fn test_count_queryset_top_truncates_after_sort() {
        let result = sample_count_result_three_crates();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::by_code(), // descending: gamma(400), beta(150), alpha(50)
        )
        .top(2);

        assert_eq!(qs.items.len(), 2);
        assert_eq!(qs.items[0].label, "gamma");
        assert_eq!(qs.items[1].label, "beta");
        // total reflects the full data set, not the truncated slice
        assert_eq!(qs.total.code, 600);
        assert_eq!(qs.file_count, 6);
    }

    #[test]
    fn test_count_queryset_top_larger_than_len_is_noop() {
        let result = sample_count_result_three_crates();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::by_code(),
        )
        .top(99);

        assert_eq!(qs.items.len(), 3);
    }

    #[test]
    fn test_filter_gte_drops_items_below_threshold() {
        // alpha=50 code, beta=150, gamma=400. Filter --code-gte 100 keeps
        // beta and gamma.
        let result = sample_count_result_three_crates();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        )
        .filter(&[Predicate::new(Field::Code, Op::Gte, 100)]);

        let labels: Vec<_> = qs.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["beta", "gamma"]);
    }

    #[test]
    fn test_filter_eq_and_ne() {
        let result = sample_count_result_three_crates();
        let eq = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        )
        .filter(&[Predicate::new(Field::Code, Op::Eq, 150)]);
        assert_eq!(eq.items.len(), 1);
        assert_eq!(eq.items[0].label, "beta");

        let ne = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        )
        .filter(&[Predicate::new(Field::Code, Op::Ne, 150)]);
        assert_eq!(ne.items.len(), 2);
    }

    #[test]
    fn test_filter_combines_predicates_with_and() {
        let result = sample_count_result_three_crates();
        // alpha=50/25, beta=150/75, gamma=400/200
        // --code-gt 100 AND --tests-lt 100 -> only beta (150 code, 75 tests)
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        )
        .filter(&[
            Predicate::new(Field::Code, Op::Gt, 100),
            Predicate::new(Field::Tests, Op::Lt, 100),
        ]);

        assert_eq!(qs.items.len(), 1);
        assert_eq!(qs.items[0].label, "beta");
    }

    #[test]
    fn test_filter_total_honors_line_types() {
        // alpha total=75 (50+25), beta total=225, gamma total=600.
        // With everything enabled, --total-gte 200 keeps beta and gamma.
        let result = sample_count_result_three_crates();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        )
        .filter(&[Predicate::new(Field::Total, Op::Gte, 200)]);
        assert_eq!(qs.items.len(), 2);

        // With only `code` enabled, "total" becomes just code values.
        // alpha=50, beta=150, gamma=400. --total-gte 200 keeps gamma alone.
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::new().with_code(),
            Ordering::default(),
        )
        .filter(&[Predicate::new(Field::Total, Op::Gte, 200)]);
        assert_eq!(qs.items.len(), 1);
        assert_eq!(qs.items[0].label, "gamma");
    }

    #[test]
    fn test_filter_empty_predicates_is_noop() {
        let result = sample_count_result_three_crates();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        )
        .filter(&[]);
        assert_eq!(qs.items.len(), 3);
    }

    #[test]
    fn test_filter_preserves_total_and_total_items() {
        // total_items should still reflect the pre-filter row count, so the
        // table footer can render "top X of Y" honestly.
        let result = sample_count_result_three_crates();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        )
        .filter(&[Predicate::new(Field::Code, Op::Gte, 200)]);

        assert_eq!(qs.items.len(), 1); // only gamma
        assert_eq!(qs.total_items, 3); // pre-filter
        assert_eq!(qs.total.code, 600); // pre-filter
    }

    #[test]
    fn test_filter_chains_with_top() {
        // Filter then top. Filter keeps beta+gamma, top 1 (with default
        // label-asc ordering) keeps beta.
        let result = sample_count_result_three_crates();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        )
        .filter(&[Predicate::new(Field::Code, Op::Gte, 100)])
        .top(1);

        assert_eq!(qs.items.len(), 1);
        assert_eq!(qs.items[0].label, "beta");
        assert_eq!(qs.total_items, 3);
    }

    #[test]
    fn test_count_queryset_top_zero_empties_items() {
        let result = sample_count_result_three_crates();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::by_code(),
        )
        .top(0);

        assert_eq!(qs.items.len(), 0);
        // Total preserved.
        assert_eq!(qs.total.code, 600);
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

    fn sample_diff_result_two_files() -> crate::data::diff::DiffResult {
        use crate::data::diff::{
            CrateDiffStats, DiffResult, FileChangeType, FileDiffStats, LocsDiff,
        };
        use crate::data::stats::Locs;

        let big = LocsDiff {
            added: Locs {
                code: 200,
                tests: 0,
                examples: 0,
                docs: 0,
                comments: 0,
                blanks: 0,
                total: 200,
            },
            removed: Locs {
                code: 50,
                tests: 0,
                examples: 0,
                docs: 0,
                comments: 0,
                blanks: 0,
                total: 50,
            },
        };
        let small = LocsDiff {
            added: Locs {
                code: 10,
                tests: 0,
                examples: 0,
                docs: 0,
                comments: 0,
                blanks: 0,
                total: 10,
            },
            removed: Locs {
                code: 30,
                tests: 0,
                examples: 0,
                docs: 0,
                comments: 0,
                blanks: 0,
                total: 30,
            },
        };

        let big_file = FileDiffStats {
            path: PathBuf::from("big.rs"),
            change_type: FileChangeType::Modified,
            diff: big,
        };
        let small_file = FileDiffStats {
            path: PathBuf::from("small.rs"),
            change_type: FileChangeType::Modified,
            diff: small,
        };

        DiffResult {
            root: PathBuf::from("/workspace"),
            from_commit: "HEAD~1".to_string(),
            to_commit: "HEAD".to_string(),
            total: big + small,
            crates: vec![CrateDiffStats {
                name: "x".to_string(),
                path: PathBuf::from("/workspace"),
                diff: big + small,
                files: vec![big_file.clone(), small_file.clone()],
            }],
            files: vec![big_file, small_file],
            non_rust_added: 0,
            non_rust_removed: 0,
        }
    }

    #[test]
    fn test_diff_filter_uses_net_value() {
        // big.rs net code = +150, small.rs net code = -20.
        // --code-gt 0 keeps only big.rs.
        let result = sample_diff_result_two_files();
        let qs = DiffQuerySet::from_result(
            &result,
            Aggregation::ByFile,
            LineTypes::everything(),
            Ordering::default(),
        )
        .filter(&[Predicate::new(Field::Code, Op::Gt, 0)]);

        assert_eq!(qs.items.len(), 1);
        assert_eq!(qs.items[0].label, "big.rs");
    }

    #[test]
    fn test_diff_filter_negative_net_via_lt_zero() {
        // big.rs has net code = +150, small.rs has net code = -20.
        // The threshold here is 0 (a valid u64), but the LHS — the net
        // diff value — is signed and can be negative. So `--code-lt 0`
        // matches files whose net code change is below zero, i.e. those
        // with more code removed than added (small.rs only).
        //
        // Predicate.value is u64, so the threshold itself can't be
        // negative; we don't need it to be — the signed net LHS is what
        // makes "less than zero" meaningful.
        let result = sample_diff_result_two_files();
        let qs = DiffQuerySet::from_result(
            &result,
            Aggregation::ByFile,
            LineTypes::everything(),
            Ordering::default(),
        )
        .filter(&[Predicate::new(Field::Code, Op::Lt, 0)]);

        assert_eq!(qs.items.len(), 1);
        assert_eq!(qs.items[0].label, "small.rs");
    }

    #[test]
    fn test_diff_filter_preserves_total_items() {
        let result = sample_diff_result_two_files();
        let qs = DiffQuerySet::from_result(
            &result,
            Aggregation::ByFile,
            LineTypes::everything(),
            Ordering::default(),
        )
        .filter(&[Predicate::new(Field::Code, Op::Gte, 100)]);

        assert_eq!(qs.items.len(), 1);
        assert_eq!(qs.total_items, 2); // pre-filter
    }
}
