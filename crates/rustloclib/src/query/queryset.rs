//! Query set: the canonical command response.
//!
//! [`CountQuerySet`] and [`DiffQuerySet`] are the single, typed, serializable
//! value each command produces. They are **independent of output mode**: the
//! same query set backs the human table, JSON/YAML/XML serialization, and CSV.
//! Adapting a query set to a particular presentation is the render layer's job,
//! never the handler's.
//!
//! A QuerySet is where this library's pipeline ends: it sits between raw
//! counting/diff results and whatever presentation the caller builds.
//! It represents data that has been:
//! - Aggregated to the requested level (crate, module, file)
//! - Sorted according to the ordering preference
//!
//! ## Line types are a *view descriptor*, not a data filter
//!
//! The `line_types` field records which line types the user asked to *see*; the
//! render layer uses it to choose columns. At this layer it zeroes nothing: a
//! query set never applies line-type filtering of its own, so ordering and
//! predicates evaluate against whatever counts the upstream result carried
//! rather than against values zeroed for display.
//!
//! Completeness itself is the *caller's* to guarantee, not this layer's.
//! [`CountOptions::line_types`] / [`DiffOptions::line_types`] **are** data
//! filters — they zero disabled types before returning — so a query set built
//! from a narrowed [`CountResult`] carries those zeros through. A caller who
//! wants one response to serve every output mode must count with
//! [`LineTypes::everything`] and narrow only at the view; that is what the CLI
//! does.
//!
//! [`CountOptions::line_types`]: crate::data::counter::CountOptions::line_types
//! [`DiffOptions::line_types`]: crate::data::diff::DiffOptions::line_types
//! [`LineTypes::everything`]: crate::query::options::LineTypes::everything
//!
//! After construction with `from_result`, two chainable methods further
//! reduce the row set:
//!
//! - [`CountQuerySet::filter`] / [`DiffQuerySet::filter`] — keep only rows
//!   satisfying every supplied [`Predicate`] (AND-combined). For diffs,
//!   each predicate is evaluated against the row's net change
//!   (added − removed).
//! - [`CountQuerySet::top`] / [`DiffQuerySet::top`] — truncate to the first
//!   N rows after sorting.
//!
//! Order of application is whatever the caller chains.
//! `.filter(...).top(N)` keeps the top N of the matching rows (the CLI's
//! convention); `.top(N).filter(...)` filters within an already-truncated
//! slice. `total` and `total_items` always describe the underlying data
//! set, not the post-filter/post-top slice — that lets the footer render
//! "top X of Y" honestly.
//!
//! The data pipeline is:
//! 1. Raw Data (CountResult, DiffResult)
//! 2. QuerySet (filtered, aggregated, sorted, optionally truncated) — the
//!    library's final output; presentation lives in the calling application
//!
//! [`Predicate`]: crate::query::options::Predicate

use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use crate::data::counter::{compute_module_name, CountResult};
use crate::data::diff::{DiffResult, LocsDiff};
use crate::data::stats::Locs;

use super::options::{Aggregation, Field, LineTypes, OrderBy, OrderDirection, Ordering, Predicate};

/// A single item in a query set (one row of data before string formatting).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryItem<T> {
    /// Row label (file path, crate name, module name, etc.)
    pub label: String,
    /// Statistics for this item
    pub stats: T,
}

/// Query set for count results — the canonical `count` response.
///
/// The same value is rendered as a table, serialized to JSON/YAML/XML, and
/// adapted to CSV rows. Construction never depends on the output mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CountQuerySet {
    /// Aggregation level used
    pub aggregation: Aggregation,
    /// Line types the caller asked to see. A *view descriptor* for the render
    /// layer — it never narrows `items`/`total` here. Whether those counts are
    /// complete depends on the [`CountResult`] they were built from; see the
    /// module docs.
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

/// Query set for diff results — the canonical `diff` response.
///
/// The same value is rendered as a table, serialized to JSON/YAML/XML, and
/// adapted to CSV rows. Construction never depends on the output mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffQuerySet {
    /// Aggregation level used
    pub aggregation: Aggregation,
    /// Line types the caller asked to see. A *view descriptor* for the render
    /// layer — it never narrows `items`/`total` here. Whether those counts are
    /// complete depends on the [`DiffResult`] they were built from; see the
    /// module docs.
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
    /// Lines added in files skipped by the active language selection.
    #[serde(default)]
    pub non_rust_added: u64,
    /// Lines removed in files skipped by the active language selection.
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
    /// Applies aggregation level and ordering. `line_types` is recorded as the
    /// requested *view* and does not filter the data here: `items` and `total`
    /// carry whatever `result` carried.
    ///
    /// So they are complete only if `result` was counted with
    /// [`LineTypes::everything`] — [`CountOptions::line_types`] zeroes disabled
    /// types before returning, and those zeros pass straight through. Count
    /// with everything and narrow at the view if one response must serve every
    /// output mode. See the module docs for why.
    ///
    /// [`LineTypes::everything`]: crate::query::options::LineTypes::everything
    /// [`CountOptions::line_types`]: crate::data::counter::CountOptions::line_types
    pub fn from_result(
        result: &CountResult,
        aggregation: Aggregation,
        line_types: LineTypes,
        ordering: Ordering,
    ) -> Self {
        let items = build_count_items(result, &aggregation, &ordering);
        let total = result.total;
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
    /// Every predicate evaluates against the item's real counts, never against
    /// a display-filtered view: `line_types` selects columns, not data. So
    /// `--blanks-gte 10` matches on real blank counts even when blanks aren't
    /// among the displayed line types.
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
        self.items
            .retain(|item| preds.iter().all(|p| matches_locs(p, &item.stats)));
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

/// Resolve the integer value a predicate's field refers to in a `Locs`.
///
/// `Field::Total` reads `Locs::total` directly — the same precomputed
/// all-types sum that the displayed `Total` column shows. Filtering on
/// `total` matches what the user sees, regardless of which line types
/// the active `LineTypes` happens to enable for column display.
fn locs_field_value(locs: &Locs, field: Field) -> i64 {
    let v: u64 = match field {
        Field::Code => locs.code,
        Field::Tests => locs.tests,
        Field::Examples => locs.examples,
        Field::Docs => locs.docs,
        Field::Comments => locs.comments,
        Field::Blanks => locs.blanks,
        Field::Total => locs.total,
    };
    u64_to_i64_sat(v)
}

fn matches_locs(pred: &Predicate, locs: &Locs) -> bool {
    let lhs = locs_field_value(locs, pred.field);
    pred.op.evaluate(lhs, u64_to_i64_sat(pred.value))
}

/// Resolve the (signed) net diff value a predicate's field refers to.
///
/// `Field::Total` uses `LocsDiff::net_total()` — the all-types net change.
/// Same WYSIWYF rationale as `locs_field_value`.
fn diff_field_value(diff: &LocsDiff, field: Field) -> i64 {
    match field {
        Field::Code => diff.net_code(),
        Field::Tests => diff.net_tests(),
        Field::Examples => diff.net_examples(),
        Field::Docs => diff.net_docs(),
        Field::Comments => diff.net_comments(),
        Field::Blanks => diff.net_blanks(),
        Field::Total => diff.net_total(),
    }
}

fn matches_diff(pred: &Predicate, diff: &LocsDiff) -> bool {
    let lhs = diff_field_value(diff, pred.field);
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
    /// Applies aggregation level and ordering. `line_types` is recorded as the
    /// requested *view* and does not filter the data here: `items` and `total`
    /// carry whatever `result` carried.
    ///
    /// So they are complete only if `result` was diffed with
    /// [`LineTypes::everything`] — [`DiffOptions::line_types`] zeroes disabled
    /// types before returning, and those zeros pass straight through. See the
    /// module docs for why.
    ///
    /// [`LineTypes::everything`]: crate::query::options::LineTypes::everything
    /// [`DiffOptions::line_types`]: crate::data::diff::DiffOptions::line_types
    pub fn from_result(
        result: &DiffResult,
        aggregation: Aggregation,
        line_types: LineTypes,
        ordering: Ordering,
    ) -> Self {
        let items = build_diff_items(result, &aggregation, &ordering);
        let total = result.total;
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
    /// As on the count side, predicates always see real counts rather than a
    /// display-filtered view. See [`CountQuerySet::filter`] for the rationale
    /// on why `total_items` isn't updated.
    #[must_use]
    pub fn filter(mut self, preds: &[Predicate]) -> Self {
        if preds.is_empty() {
            return self;
        }
        self.items
            .retain(|item| preds.iter().all(|p| matches_diff(p, &item.stats)));
        self
    }
}

/// Get sort key for Locs based on OrderBy.
///
/// `OrderBy::Total` reads `Locs::total` so sort order matches the displayed
/// `Total` column (which is also `Locs::total`). See `locs_field_value` for
/// the same rationale on the predicate side.
fn count_sort_key(locs: &Locs, order_by: &OrderBy) -> u64 {
    match order_by {
        OrderBy::Label => 0, // Label sorting handled separately
        OrderBy::Code => locs.code,
        OrderBy::Tests => locs.tests,
        OrderBy::Examples => locs.examples,
        OrderBy::Docs => locs.docs,
        OrderBy::Comments => locs.comments,
        OrderBy::Blanks => locs.blanks,
        OrderBy::Total => locs.total,
    }
}

/// Build query items from CountResult based on aggregation level.
///
/// Stats are carried through complete — line-type selection is a view concern
/// resolved at render time, so sorting here always sees real values.
fn build_count_items(
    result: &CountResult,
    aggregation: &Aggregation,
    ordering: &Ordering,
) -> Vec<QueryItem<Locs>> {
    let mut items: Vec<(String, Locs)> = match aggregation {
        Aggregation::Total => return vec![],
        Aggregation::ByCrate => result
            .crates
            .iter()
            .map(|c| (c.name.clone(), c.stats))
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
                (label, m.stats)
            })
            .collect(),
        Aggregation::ByFile => result
            .files
            .iter()
            .map(|f| (relative_path_label(&f.path, &result.root), f.stats))
            .collect(),
    };

    // Sort based on ordering
    match ordering.by {
        OrderBy::Label => {
            items.sort_by(|a, b| a.0.cmp(&b.0));
        }
        _ => {
            items.sort_by(|a, b| {
                let key_a = count_sort_key(&a.1, &ordering.by);
                let key_b = count_sort_key(&b.1, &ordering.by);
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

/// Get sort key for LocsDiff based on OrderBy (uses net change).
///
/// `OrderBy::Total` uses `LocsDiff::net_total()` for the same WYSIWYF
/// rationale as the count side — sort order matches the displayed Total.
fn diff_sort_key(diff: &LocsDiff, order_by: &OrderBy) -> i64 {
    match order_by {
        OrderBy::Label => 0, // Label sorting handled separately
        OrderBy::Code => diff.net_code(),
        OrderBy::Tests => diff.net_tests(),
        OrderBy::Examples => diff.net_examples(),
        OrderBy::Docs => diff.net_docs(),
        OrderBy::Comments => diff.net_comments(),
        OrderBy::Blanks => diff.net_blanks(),
        OrderBy::Total => diff.net_total(),
    }
}

/// Build query items from DiffResult based on aggregation level.
///
/// Stats are carried through complete — line-type selection is a view concern
/// resolved at render time, so sorting here always sees real values.
fn build_diff_items(
    result: &DiffResult,
    aggregation: &Aggregation,
    ordering: &Ordering,
) -> Vec<QueryItem<LocsDiff>> {
    let mut items: Vec<(String, LocsDiff)> = match aggregation {
        Aggregation::Total => return vec![],
        Aggregation::ByCrate => result
            .crates
            .iter()
            .map(|c| (c.name.clone(), c.diff))
            .collect(),
        Aggregation::ByModule => {
            let mut module_map: HashMap<String, LocsDiff> = HashMap::new();
            if result.crates.is_empty() {
                for file in &result.files {
                    let abs_path = if file.path.is_absolute() {
                        file.path.clone()
                    } else {
                        result.root.join(&file.path)
                    };
                    let module_name = compute_module_name(&abs_path, &result.root);
                    let full_name = if module_name.is_empty() {
                        "(root)".to_string()
                    } else {
                        module_name
                    };
                    let entry = module_map.entry(full_name).or_default();
                    *entry += file.diff;
                }
            } else {
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
                        *entry += file.diff;
                    }
                }
            }
            module_map.into_iter().collect()
        }
        Aggregation::ByFile => result
            .files
            .iter()
            .map(|f| (f.path.to_string_lossy().to_string(), f.diff))
            .collect(),
    };

    // Sort based on ordering
    match ordering.by {
        OrderBy::Label => {
            items.sort_by(|a, b| a.0.cmp(&b.0));
        }
        _ => {
            items.sort_by(|a, b| {
                let key_a = diff_sort_key(&a.1, &ordering.by);
                let key_b = diff_sort_key(&b.1, &ordering.by);
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
    fn test_line_types_is_a_view_descriptor_not_a_data_filter() {
        // `line_types` records what the caller wants to *see*; it must not
        // zero the underlying data. Keeping stats complete is what lets the
        // same response serve the table, JSON/YAML/XML, and CSV.
        let result = sample_count_result();
        let line_types = LineTypes::new().with_code();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            line_types,
            Ordering::default(),
        );

        assert_eq!(qs.items[0].stats.code, 50);
        // Not among the requested line types, but still carries its real count.
        assert_eq!(qs.items[0].stats.tests, 25);
        // The request itself is preserved for the render layer.
        assert_eq!(qs.line_types, line_types);
    }

    #[test]
    fn test_response_is_identical_regardless_of_requested_line_types() {
        // The canonical response's *data* must not vary with the view. Only
        // the `line_types` descriptor differs between these two.
        let result = sample_count_result();
        let full = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        );
        let narrow = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::new().with_code(),
            Ordering::default(),
        );

        assert_eq!(full.items, narrow.items);
        assert_eq!(full.total, narrow.total);
        assert_ne!(full.line_types, narrow.line_types);
    }

    #[test]
    fn test_filter_sees_real_counts_for_undisplayed_line_types() {
        // Regression: predicates used to run against display-filtered stats,
        // so `--tests-gte N` silently matched nothing whenever `tests` wasn't
        // among the requested line types — while the same query worked in
        // JSON mode. Data and view are now independent.
        let result = sample_count_result_three_crates();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::new().with_code(), // tests NOT displayed
            Ordering::default(),
        )
        .filter(&[Predicate::new(Field::Tests, Op::Gte, 75)]);

        // beta (75 tests) and gamma (200 tests) still match.
        let labels: Vec<_> = qs.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["beta", "gamma"]);
    }

    #[test]
    fn test_ordering_sees_real_counts_for_undisplayed_line_types() {
        // Same class of bug as above, on the ordering side.
        let result = sample_count_result_three_crates();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::new().with_code(), // tests NOT displayed
            Ordering {
                by: OrderBy::Tests,
                direction: OrderDirection::Descending,
            },
        );

        let labels: Vec<_> = qs.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["gamma", "beta", "alpha"]);
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
    fn test_filter_total_uses_full_total_regardless_of_line_types() {
        // `Field::Total` reads `Locs::total` (the precomputed all-types
        // sum, what the displayed `Total` column shows), independent of
        // which line types `LineTypes` enables for column display. This
        // is the WYSIWYF guarantee: filtering on `total` matches what
        // the user sees in the Total column.
        //
        // alpha total=75, beta total=225, gamma total=600.
        let result = sample_count_result_three_crates();

        // With everything enabled: --total-gte 200 keeps beta + gamma.
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        )
        .filter(&[Predicate::new(Field::Total, Op::Gte, 200)]);
        assert_eq!(qs.items.len(), 2);

        // With only `code` enabled, the predicate STILL evaluates against
        // `Locs::total`, so the same 2 rows survive. Before the WYSIWYF
        // fix this returned only `gamma` because `Field::Total` was
        // redefined as "sum of currently-enabled line types", which made
        // it diverge from the visible Total column.
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::new().with_code(),
            Ordering::default(),
        )
        .filter(&[Predicate::new(Field::Total, Op::Gte, 200)]);
        assert_eq!(qs.items.len(), 2);
        let labels: Vec<_> = qs.items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"beta"));
        assert!(labels.contains(&"gamma"));
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
