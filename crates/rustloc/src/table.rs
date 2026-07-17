//! The typed payload the table templates render.
//!
//! The data flow is:
//!
//! 1. Raw data (`CountResult`, `DiffResult`) — `rustloclib`
//! 2. QuerySet (aggregated, sorted, optionally filtered/truncated) — the
//!    canonical, output-mode-independent command response, and the end of
//!    `rustloclib`'s reusable pipeline
//! 3. [`CountView`] / [`DiffView`] — *this* module: the same numbers, narrowed
//!    to the requested columns and paired with the facts a table needs
//! 4. The rendered table — `templates/count_table.jinja`,
//!    `templates/diff_table.jinja`, and their shared `table_macros.jinja`
//!
//! ## This module is not a formatter
//!
//! It builds no display strings, computes no widths, picks no wording, and
//! writes no style tags. Every one of those is human rendering *policy*, and
//! policy lives in MiniJinja. What crosses this boundary is typed numbers plus
//! the handful of facts the wording depends on (how many rows were displayed of
//! how many, and whether `--top` or a filter did the reducing) — never a
//! sentence built from them.
//!
//! That split is what makes the two readable in isolation: the templates are
//! the whole answer to "what does a user see?", and this module is the whole
//! answer to "which numbers do they see?". It also keeps `rustloclib` clean —
//! the library ends at typed data — and keeps the structured modes honest,
//! since `json`/`yaml`/`xml` serialize the query set directly and never reach
//! this module at all.
//!
//! ## What stays here, and why
//!
//! Two decisions are Rust-side on purpose:
//!
//! - **Column selection.** The query set's `line_types` is a *view descriptor*:
//!   the response carries complete counts and this layer picks which become
//!   columns. That is what lets one response render as a narrow table and still
//!   serialize a full, stable JSON/CSV schema.
//! - **The `total_items` clamp** in [`Footer::new`] — data repair, not wording.
//!   See the comment there.
//!
//! Everything else a reader sees is in the templates.

use rustloclib::{Aggregation, CountQuerySet, DiffQuerySet, LineTypes, Locs, LocsDiff};
use serde::Serialize;

/// A data row: its label, plus one value per enabled column in column order.
///
/// Generic over the cell type because count cells are a single number and diff
/// cells are a [`DiffValue`] triple — the row shape itself is identical.
#[derive(Debug, Clone, Serialize)]
pub struct Row<V> {
    /// File path, crate name, module name — whatever the aggregation groups by.
    pub label: String,
    /// One value per enabled column, positionally matching `columns`.
    pub values: Vec<V>,
}

/// The facts the footer's wording is derived from.
///
/// Facts, not a sentence: whether the footer reads "Total (2 crates)",
/// "Total (top 1 of 2 crates)" or "Total (1 of 2 crates)" is wording, and the
/// template decides it. This struct only reports what happened.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Footer {
    /// Rows actually shown, after any user-driven reduction.
    pub displayed: usize,
    /// Rows before that reduction. Equals `displayed` when nothing reduced them.
    pub total_items: usize,
    /// Files analyzed — what total aggregation counts, having no rows to count.
    pub file_count: usize,
    /// True iff `--top` did the reducing (a sorted slice) rather than a filter
    /// (predicate-eliminated rows). The two mean different things to a reader,
    /// so the template needs to tell them apart.
    pub top_applied: bool,
}

impl Footer {
    fn new(displayed: usize, total_items: usize, file_count: usize, top_applied: bool) -> Self {
        Footer {
            displayed,
            // `total_items < displayed` is logically impossible — reductions
            // only ever shrink the row set — but a query set deserialized from
            // a payload predating the `total_items` field arrives with 0.
            //
            // The clamp is here rather than in the template because it is data
            // repair, not presentation: it answers "which of these two numbers
            // is trustworthy?", which is a question about the payload. Without
            // it the template would faithfully render "Total (0 crates)" above
            // two visible rows.
            total_items: total_items.max(displayed),
            file_count,
            top_applied,
        }
    }
}

/// The count table's payload.
#[derive(Debug, Clone, Serialize)]
pub struct CountView {
    /// Aggregation key: `total`, `crate`, `module`, or `file`.
    pub aggregation: &'static str,
    /// Enabled column keys, in display order.
    pub columns: Vec<&'static str>,
    /// Data rows.
    pub rows: Vec<Row<u64>>,
    /// The totals row's values, positionally matching `columns`.
    pub total: Vec<u64>,
    /// Facts behind the footer's wording.
    pub footer: Footer,
}

impl CountView {
    /// Build the count table's payload from its canonical response.
    pub fn from_queryset(qs: &CountQuerySet) -> Self {
        let columns = enabled_columns(&qs.line_types);
        CountView {
            aggregation: aggregation_key(&qs.aggregation),
            rows: qs
                .items
                .iter()
                .map(|item| Row {
                    label: item.label.clone(),
                    values: columns.iter().map(|c| c.count(&item.stats)).collect(),
                })
                .collect(),
            total: columns.iter().map(|c| c.count(&qs.total)).collect(),
            footer: Footer::new(
                qs.items.len(),
                qs.total_items,
                qs.file_count,
                qs.top_applied,
            ),
            columns: columns.iter().map(|c| c.key()).collect(),
        }
    }
}

/// One diff cell's numbers.
///
/// `net` is `i64` so a net removal stays negative rather than underflowing into
/// a very large positive number.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct DiffValue {
    /// Lines added.
    pub added: u64,
    /// Lines removed.
    pub removed: u64,
    /// `added - removed`.
    pub net: i64,
}

impl DiffValue {
    fn new(added: u64, removed: u64) -> Self {
        DiffValue {
            added,
            removed,
            net: added as i64 - removed as i64,
        }
    }
}

/// The diff table's payload.
#[derive(Debug, Clone, Serialize)]
pub struct DiffView {
    /// Aggregation key: `total`, `crate`, `module`, or `file`.
    pub aggregation: &'static str,
    /// Enabled column keys, in display order.
    pub columns: Vec<&'static str>,
    /// Data rows.
    pub rows: Vec<Row<DiffValue>>,
    /// The totals row's values, positionally matching `columns`.
    pub total: Vec<DiffValue>,
    /// Facts behind the footer's wording.
    pub footer: Footer,
    /// The revision compared from.
    pub from_commit: String,
    /// The revision compared to.
    pub to_commit: String,
    /// Changes in files the active language selection skipped.
    ///
    /// Always present, even when zero: whether a zero summary is worth showing
    /// a reader is the template's call, not this module's.
    pub non_rust: DiffValue,
}

impl DiffView {
    /// Build the diff table's payload from its canonical response.
    pub fn from_queryset(qs: &DiffQuerySet) -> Self {
        let columns = enabled_columns(&qs.line_types);
        DiffView {
            aggregation: aggregation_key(&qs.aggregation),
            rows: qs
                .items
                .iter()
                .map(|item| Row {
                    label: item.label.clone(),
                    values: columns.iter().map(|c| c.diff_value(&item.stats)).collect(),
                })
                .collect(),
            total: columns.iter().map(|c| c.diff_value(&qs.total)).collect(),
            footer: Footer::new(
                qs.items.len(),
                qs.total_items,
                qs.file_count,
                qs.top_applied,
            ),
            from_commit: qs.from_commit.clone(),
            to_commit: qs.to_commit.clone(),
            non_rust: DiffValue::new(qs.non_rust_added, qs.non_rust_removed),
            columns: columns.iter().map(|c| c.key()).collect(),
        }
    }
}

/// One value column of the table.
///
/// The query set's `line_types` selects which of these appear; this enum is the
/// single source of truth for their order and per-column accessors, so the
/// column keys and the cells beneath them can never drift out of alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    Code,
    Tests,
    Examples,
    Docs,
    Comments,
    Blanks,
    Total,
}

impl Column {
    /// This column's key.
    ///
    /// A *data* name — it matches the `Locs` field and the JSON/CSV schemas, and
    /// is deliberately not a display word. The templates map keys to the words a
    /// reader sees, which is why nothing in Rust needs to know that `code` is
    /// shown as "Code".
    fn key(self) -> &'static str {
        match self {
            Column::Code => "code",
            Column::Tests => "tests",
            Column::Examples => "examples",
            Column::Docs => "docs",
            Column::Comments => "comments",
            Column::Blanks => "blanks",
            Column::Total => "total",
        }
    }

    /// This column's count out of a `Locs`.
    fn count(self, locs: &Locs) -> u64 {
        match self {
            Column::Code => locs.code,
            Column::Tests => locs.tests,
            Column::Examples => locs.examples,
            Column::Docs => locs.docs,
            Column::Comments => locs.comments,
            Column::Blanks => locs.blanks,
            // Precomputed by the library, not summed here.
            Column::Total => locs.total,
        }
    }

    /// This column's cell out of a `LocsDiff`.
    fn diff_value(self, diff: &LocsDiff) -> DiffValue {
        DiffValue::new(self.count(&diff.added), self.count(&diff.removed))
    }
}

/// The enabled columns, in display order.
fn enabled_columns(line_types: &LineTypes) -> Vec<Column> {
    [
        (line_types.code, Column::Code),
        (line_types.tests, Column::Tests),
        (line_types.examples, Column::Examples),
        (line_types.docs, Column::Docs),
        (line_types.comments, Column::Comments),
        (line_types.blanks, Column::Blanks),
        (line_types.total, Column::Total),
    ]
    .into_iter()
    .filter_map(|(enabled, column)| enabled.then_some(column))
    .collect()
}

/// The aggregation's key.
///
/// Like [`Column::key`], a data name: the templates turn `crate` into the
/// "Crate" header and the "crates" footer unit.
fn aggregation_key(aggregation: &Aggregation) -> &'static str {
    match aggregation {
        Aggregation::Total => "total",
        Aggregation::ByCrate => "crate",
        Aggregation::ByModule => "module",
        Aggregation::ByFile => "file",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustloclib::{CountResult, CrateStats, Ordering};
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

    fn queryset(line_types: LineTypes, ordering: Ordering) -> CountQuerySet {
        CountQuerySet::from_result(
            &sample_count_result(),
            Aggregation::ByCrate,
            line_types,
            ordering,
        )
    }

    #[test]
    fn columns_are_data_keys_not_display_words() {
        let view =
            CountView::from_queryset(&queryset(LineTypes::everything(), Ordering::default()));
        // Keys, lowercase, matching the JSON/CSV field names. The header words
        // ("Code", "Tests", ...) belong to the template and must not appear.
        assert_eq!(
            view.columns,
            vec!["code", "tests", "examples", "docs", "comments", "blanks", "total"]
        );
    }

    #[test]
    fn line_types_narrow_the_columns() {
        // `with_code` keeps the Total column, which `LineTypes::new` enables.
        let view =
            CountView::from_queryset(&queryset(LineTypes::new().with_code(), Ordering::default()));
        assert_eq!(view.columns, vec!["code", "total"]);
        // ...and the cells narrow with them, positionally.
        assert_eq!(view.rows[0].values.len(), 2);
        assert_eq!(view.total.len(), 2);
    }

    #[test]
    fn line_types_can_drop_the_total_column_too() {
        // `--type code` (no `total`) is the shape that proves the narrowing is
        // driven by the descriptor rather than by a Total column special case.
        let view = CountView::from_queryset(&queryset(
            LineTypes::new().with_code().without_total(),
            Ordering::default(),
        ));
        assert_eq!(view.columns, vec!["code"]);
        assert_eq!(view.rows[0].values, vec![50]);
        assert_eq!(view.total, vec![200]);
    }

    #[test]
    fn values_are_typed_numbers_in_column_order() {
        let view =
            CountView::from_queryset(&queryset(LineTypes::everything(), Ordering::default()));
        // Default ordering is by label ascending: alpha before beta.
        assert_eq!(view.rows[0].label, "alpha");
        assert_eq!(view.rows[0].values, vec![50, 25, 0, 0, 0, 0, 75]);
        assert_eq!(view.total, vec![200, 100, 0, 0, 0, 0, 300]);
    }

    #[test]
    fn aggregation_is_a_key_not_a_header_word() {
        for (aggregation, expected) in [
            (Aggregation::Total, "total"),
            (Aggregation::ByCrate, "crate"),
            (Aggregation::ByModule, "module"),
            (Aggregation::ByFile, "file"),
        ] {
            assert_eq!(aggregation_key(&aggregation), expected);
        }
    }

    #[test]
    fn footer_reports_reduction_facts_rather_than_wording() {
        let view = CountView::from_queryset(
            &queryset(LineTypes::everything(), Ordering::default()).top(1),
        );
        assert_eq!(view.footer.displayed, 1);
        assert_eq!(view.footer.total_items, 2);
        assert!(view.footer.top_applied);
        assert_eq!(view.footer.file_count, 4);
    }

    #[test]
    fn footer_distinguishes_filtering_from_top() {
        use rustloclib::{Field, Op, Predicate};

        let view = CountView::from_queryset(
            &queryset(LineTypes::everything(), Ordering::default()).filter(&[Predicate::new(
                Field::Code,
                Op::Gte,
                100,
            )]),
        );
        // Rows were reduced, but not by --top: the template needs both facts to
        // pick "1 of 2" over "top 1 of 2".
        assert_eq!(view.footer.displayed, 1);
        assert_eq!(view.footer.total_items, 2);
        assert!(!view.footer.top_applied);
    }

    #[test]
    fn footer_clamps_total_items_when_the_payload_predates_the_field() {
        // A query set deserialized from a stale payload arrives with
        // `total_items = 0`. Clamping here keeps the template from rendering
        // "Total (0 crates)" above two visible rows.
        let mut qs = queryset(LineTypes::everything(), Ordering::default());
        qs.total_items = 0;
        let view = CountView::from_queryset(&qs);

        assert_eq!(view.footer.displayed, 2);
        assert_eq!(view.footer.total_items, 2);
    }

    #[test]
    fn diff_values_carry_added_removed_and_a_signed_net() {
        assert_eq!(DiffValue::new(10, 5).net, 5);
        // A net removal stays negative rather than underflowing.
        assert_eq!(DiffValue::new(5, 10).net, -5);
        assert_eq!(DiffValue::new(0, 0).net, 0);
    }

    #[test]
    fn diff_view_exposes_skipped_changes_even_when_zero() {
        // Zero is still reported: whether to *show* it is the template's call.
        let qs = DiffQuerySet {
            aggregation: Aggregation::Total,
            line_types: LineTypes::everything(),
            items: vec![],
            total: LocsDiff::default(),
            file_count: 0,
            from_commit: "HEAD".to_string(),
            to_commit: "working tree".to_string(),
            non_rust_added: 0,
            non_rust_removed: 0,
            total_items: 0,
            top_applied: false,
        };
        let view = DiffView::from_queryset(&qs);

        assert_eq!(view.non_rust.added, 0);
        assert_eq!(view.non_rust.removed, 0);
        assert_eq!(view.non_rust.net, 0);
    }
}
