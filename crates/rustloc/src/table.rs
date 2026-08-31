//! The typed payload the table templates render.
//!
//! The data flow is:
//!
//! 1. Raw data (`CountResult`, `DiffResult`) — `rustloclib`
//! 2. QuerySet (aggregated, sorted, optionally filtered/truncated) — the
//!    canonical, output-mode-independent command response, and the end of
//!    `rustloclib`'s reusable pipeline
//! 3. [`CountView`] / [`DiffView`] — *this* module: the same numbers, narrowed
//!    to the requested columns and paired with the facts a table needs. Count
//!    tables may also carry ratio facts when the human table asks for them.
//!    Human tables can also carry locale-formatted display strings beside the
//!    raw numbers when `--number-fmt` or `number_fmt = true` asks for grouping.
//! 4. The rendered table — `templates/count_table.jinja`,
//!    `templates/diff_table.jinja`, and their shared `table_macros.jinja`
//!
//! ## This module is not a layout engine
//!
//! It computes no widths, picks no wording, and writes no style tags. Every one
//! of those is human rendering *policy*, and policy lives in MiniJinja. What
//! crosses this boundary is typed numbers, optional display text for those
//! numbers, and the handful of facts the wording depends on (how many rows were
//! displayed of how many, whether `--top` or a filter did the reducing, and the
//! optional file-level [`FileChangeType`] on `--by-file` diff rows) — never a
//! sentence built from them. The template maps Added/Deleted onto semantic tags;
//! this module names no style.
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

use rustloclib::{
    sat_sub_u64, Aggregation, CountQuerySet, DiffQuerySet, FileChangeType, LineTypes, Locs,
    LocsDiff,
};
use serde::Serialize;

use crate::number_format::NumberFormat;

/// One integer and the text a human table should display for it.
///
/// The raw number remains available for conditionals and tests; the template
/// reads `display` so locale punctuation never reaches structured output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DisplayNumber<T> {
    /// The typed value.
    pub raw: T,
    /// The human table's visible text.
    pub display: String,
}

impl DisplayNumber<u64> {
    fn u64(raw: u64, format: NumberFormat) -> Self {
        Self {
            raw,
            display: format.u64(raw),
        }
    }
}

impl DisplayNumber<i64> {
    fn i64(raw: i64, format: NumberFormat) -> Self {
        Self {
            raw,
            display: format.i64(raw),
        }
    }
}

/// A data row: its label, plus one value per enabled column in column order.
///
/// Generic over the cell type because count cells are a single number and diff
/// cells are a [`DiffValue`] triple — the row shape itself is identical.
///
/// [`Self::change_type`] is a typed fact for `--by-file` diff rows. The
/// template maps Added/Deleted onto semantic tags; this module names no style.
#[derive(Debug, Clone, Serialize)]
pub struct Row<V> {
    /// File path, crate name, module name — whatever the aggregation groups by.
    pub label: String,
    /// One value per enabled column, positionally matching `columns`.
    pub values: Vec<V>,
    /// Git file status. Present only on file-level diff rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_type: Option<FileChangeType>,
}

/// The facts the footer's wording is derived from.
///
/// Facts, not a sentence: whether the footer reads "Total (2 crates)",
/// "Total (top 1 of 2 crates)" or "Total (1 of 2 crates)" is wording, and the
/// template decides it. This struct only reports what happened.
#[derive(Debug, Clone, Serialize)]
pub struct Footer {
    /// Rows actually shown, after any user-driven reduction.
    pub displayed: usize,
    /// Formatted `displayed` value for human table labels.
    pub displayed_display: String,
    /// Rows before that reduction. Equals `displayed` when nothing reduced them.
    pub total_items: usize,
    /// Formatted `total_items` value for human table labels.
    pub total_items_display: String,
    /// Files analyzed — what total aggregation counts, having no rows to count.
    pub file_count: usize,
    /// Formatted `file_count` value for human table labels.
    pub file_count_display: String,
    /// True iff `--top` did the reducing (a sorted slice) rather than a filter
    /// (predicate-eliminated rows). The two mean different things to a reader,
    /// so the template needs to tell them apart.
    pub top_applied: bool,
}

impl Footer {
    fn new(
        displayed: usize,
        total_items: usize,
        file_count: usize,
        top_applied: bool,
        number_format: NumberFormat,
    ) -> Self {
        let total_items = total_items.max(displayed);
        Footer {
            displayed,
            displayed_display: number_format.u64(displayed as u64),
            // `total_items < displayed` is logically impossible — reductions
            // only ever shrink the row set — but a query set deserialized from
            // a payload predating the `total_items` field arrives with 0.
            //
            // The clamp is here rather than in the template because it is data
            // repair, not presentation: it answers "which of these two numbers
            // is trustworthy?", which is a question about the payload. Without
            // it the template would faithfully render "Total (0 crates)" above
            // two visible rows.
            total_items,
            total_items_display: number_format.u64(total_items as u64),
            file_count,
            file_count_display: number_format.u64(file_count as u64),
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
    pub rows: Vec<Row<DisplayNumber<u64>>>,
    /// The totals row's values, positionally matching `columns`.
    pub total: Vec<DisplayNumber<u64>>,
    /// Facts behind the footer's wording.
    pub footer: Footer,
    /// Optional percentage row values, positionally matching `columns`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ratios: Option<Vec<RatioValue>>,
}

impl CountView {
    /// Build the count table's payload from its canonical response.
    pub fn from_queryset(
        qs: &CountQuerySet,
        shows_ratios: bool,
        number_format: NumberFormat,
    ) -> Self {
        let columns = enabled_columns(&qs.line_types);
        let total = columns
            .iter()
            .map(|c| DisplayNumber::u64(c.count(&qs.total), number_format))
            .collect();
        let ratios = shows_ratios.then(|| {
            columns
                .iter()
                .map(|column| RatioValue::new(column.count(&qs.total), qs.total.total))
                .collect()
        });
        CountView {
            aggregation: aggregation_key(&qs.aggregation),
            rows: qs
                .items
                .iter()
                .map(|item| Row {
                    label: item.label.clone(),
                    values: columns
                        .iter()
                        .map(|c| DisplayNumber::u64(c.count(&item.stats), number_format))
                        .collect(),
                    change_type: None,
                })
                .collect(),
            total,
            footer: Footer::new(
                qs.items.len(),
                qs.total_items,
                qs.file_count,
                qs.top_applied,
                number_format,
            ),
            columns: columns.iter().map(|c| c.key()).collect(),
            ratios,
        }
    }
}

/// One ratio value, scaled to one decimal place.
///
/// The template owns the visible `%` suffix and style tag. This type carries
/// only the rounded numeric parts it needs to render exactly one decimal digit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RatioValue {
    /// Whole percentage points.
    pub whole: u64,
    /// The single fractional digit after the decimal point.
    pub tenth: u8,
}

impl RatioValue {
    fn new(value: u64, denominator: u64) -> Self {
        let scaled = if denominator == 0 {
            0
        } else {
            ((value as u128 * 1000) + (denominator as u128 / 2)) / denominator as u128
        };

        Self {
            whole: (scaled / 10) as u64,
            tenth: (scaled % 10) as u8,
        }
    }
}

/// One diff cell's numbers.
///
/// `net` is `i64` so a net removal stays negative rather than underflowing into
/// a very large positive number.
#[derive(Debug, Clone, Serialize)]
pub struct DiffValue {
    /// Lines added.
    pub added: DisplayNumber<u64>,
    /// Lines removed.
    pub removed: DisplayNumber<u64>,
    /// `added - removed`, saturating — see [`sat_sub_u64`].
    pub net: DisplayNumber<i64>,
}

impl DiffValue {
    /// Build a cell, taking the net from the library's saturating rule.
    ///
    /// Not a plain `added as i64 - removed as i64`: those casts wrap above
    /// `i64::MAX` and can hand a template a sign-flipped net, which would read
    /// as an addition where a removal happened. [`sat_sub_u64`] is the same
    /// rule `LocsDiff`'s `net_*` accessors use, so a net renders identically
    /// whether it reaches the reader through this view or through the library.
    fn new(added: u64, removed: u64, number_format: NumberFormat) -> Self {
        DiffValue {
            added: DisplayNumber::u64(added, number_format),
            removed: DisplayNumber::u64(removed, number_format),
            net: DisplayNumber::i64(sat_sub_u64(added, removed), number_format),
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
    pub fn from_queryset(qs: &DiffQuerySet, number_format: NumberFormat) -> Self {
        let columns = enabled_columns(&qs.line_types);
        DiffView {
            aggregation: aggregation_key(&qs.aggregation),
            rows: qs
                .items
                .iter()
                .map(|item| Row {
                    label: item.label.clone(),
                    values: columns
                        .iter()
                        .map(|c| c.diff_value(&item.stats, number_format))
                        .collect(),
                    change_type: item.change_type,
                })
                .collect(),
            total: columns
                .iter()
                .map(|c| c.diff_value(&qs.total, number_format))
                .collect(),
            footer: Footer::new(
                qs.items.len(),
                qs.total_items,
                qs.file_count,
                qs.top_applied,
                number_format,
            ),
            from_commit: qs.metadata.from_commit.clone(),
            to_commit: qs.metadata.to_commit.clone(),
            non_rust: DiffValue::new(
                qs.metadata.non_rust_added,
                qs.metadata.non_rust_removed,
                number_format,
            ),
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
    fn diff_value(self, diff: &LocsDiff, number_format: NumberFormat) -> DiffValue {
        DiffValue::new(
            self.count(&diff.added),
            self.count(&diff.removed),
            number_format,
        )
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
    use rustloclib::{
        CountReportMetadata, CountResult, CrateStats, DiffReportMetadata, Ordering, QueryItem,
    };
    use standout::tabular::{CellValue, Col, SubCol, SubColumns, TabularFormatter, TabularSpec};
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

    fn raw_u64(values: &[DisplayNumber<u64>]) -> Vec<u64> {
        values.iter().map(|value| value.raw).collect()
    }

    fn disabled_format() -> NumberFormat {
        NumberFormat::disabled()
    }

    #[test]
    fn columns_are_data_keys_not_display_words() {
        let view = CountView::from_queryset(
            &queryset(LineTypes::everything(), Ordering::default()),
            false,
            disabled_format(),
        );
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
        let view = CountView::from_queryset(
            &queryset(LineTypes::new().with_code(), Ordering::default()),
            false,
            disabled_format(),
        );
        assert_eq!(view.columns, vec!["code", "total"]);
        // ...and the cells narrow with them, positionally.
        assert_eq!(view.rows[0].values.len(), 2);
        assert_eq!(view.total.len(), 2);
    }

    #[test]
    fn line_types_can_drop_the_total_column_too() {
        // `--type code` (no `total`) is the shape that proves the narrowing is
        // driven by the descriptor rather than by a Total column special case.
        let view = CountView::from_queryset(
            &queryset(
                LineTypes::new().with_code().without_total(),
                Ordering::default(),
            ),
            false,
            disabled_format(),
        );
        assert_eq!(view.columns, vec!["code"]);
        assert_eq!(raw_u64(&view.rows[0].values), vec![50]);
        assert_eq!(raw_u64(&view.total), vec![200]);
    }

    #[test]
    fn values_are_typed_numbers_in_column_order() {
        let view = CountView::from_queryset(
            &queryset(LineTypes::everything(), Ordering::default()),
            false,
            disabled_format(),
        );
        // Default ordering is by label ascending: alpha before beta.
        assert_eq!(view.rows[0].label, "alpha");
        assert_eq!(raw_u64(&view.rows[0].values), vec![50, 25, 0, 0, 0, 0, 75]);
        assert_eq!(raw_u64(&view.total), vec![200, 100, 0, 0, 0, 0, 300]);
    }

    #[test]
    fn count_view_carries_locale_formatted_display_values() {
        let qs = CountQuerySet {
            aggregation: Aggregation::Total,
            line_types: LineTypes::new().with_code(),
            items: vec![],
            total: sample_locs(3805, 1200),
            file_count: 1200,
            metadata: CountReportMetadata::default(),
            total_items: 0,
            top_applied: false,
        };
        let view = CountView::from_queryset(
            &qs,
            false,
            NumberFormat::from_locale_name(Some("en-US")).unwrap(),
        );

        assert_eq!(view.total[0].raw, 3805);
        assert_eq!(view.total[0].display, "3,805");
        assert_eq!(view.total[1].raw, 5005);
        assert_eq!(view.total[1].display, "5,005");
        assert_eq!(view.footer.file_count_display, "1,200");
    }

    #[test]
    fn footer_carries_locale_formatted_summary_numbers() {
        let footer = Footer::new(
            1200,
            3805,
            5005,
            true,
            NumberFormat::from_locale_name(Some("de-DE")).unwrap(),
        );

        assert_eq!(footer.displayed, 1200);
        assert_eq!(footer.displayed_display, "1.200");
        assert_eq!(footer.total_items, 3805);
        assert_eq!(footer.total_items_display, "3.805");
        assert_eq!(footer.file_count, 5005);
        assert_eq!(footer.file_count_display, "5.005");
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
            false,
            disabled_format(),
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
            false,
            disabled_format(),
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
        let view = CountView::from_queryset(&qs, false, disabled_format());

        assert_eq!(view.footer.displayed, 2);
        assert_eq!(view.footer.total_items, 2);
    }

    #[test]
    fn count_view_omits_ratios_by_default() {
        let view = CountView::from_queryset(
            &queryset(LineTypes::new().with_code(), Ordering::default()),
            false,
            disabled_format(),
        );

        assert!(view.ratios.is_none());
    }

    #[test]
    fn count_view_reports_one_decimal_ratio_parts() {
        let view = CountView::from_queryset(
            &queryset(LineTypes::new().with_code(), Ordering::default()),
            true,
            disabled_format(),
        );
        let ratios = view.ratios.unwrap();

        assert_eq!(
            ratios,
            vec![
                RatioValue {
                    whole: 66,
                    tenth: 7
                },
                RatioValue {
                    whole: 100,
                    tenth: 0
                },
            ]
        );
    }

    #[test]
    fn count_view_reports_zero_ratios_for_zero_totals() {
        let qs = CountQuerySet {
            aggregation: Aggregation::Total,
            line_types: LineTypes::everything(),
            items: vec![],
            total: Locs::default(),
            file_count: 0,
            metadata: CountReportMetadata::default(),
            total_items: 0,
            top_applied: false,
        };
        let view = CountView::from_queryset(&qs, true, disabled_format());

        assert!(view
            .ratios
            .unwrap()
            .iter()
            .all(|ratio| *ratio == RatioValue { whole: 0, tenth: 0 }));
    }

    #[test]
    fn diff_values_carry_added_removed_and_a_signed_net() {
        assert_eq!(DiffValue::new(10, 5, disabled_format()).net.raw, 5);
        // A net removal stays negative rather than underflowing.
        assert_eq!(DiffValue::new(5, 10, disabled_format()).net.raw, -5);
        assert_eq!(DiffValue::new(0, 0, disabled_format()).net.raw, 0);
    }

    #[test]
    fn diff_value_net_saturates_instead_of_wrapping() {
        // Counts past `i64::MAX` are unreachable from real files, but a plain
        // `as i64` cast would wrap them into a *sign-flipped* net — rendering a
        // removal as an addition. Pinning the saturating rule keeps any
        // pathological payload clamped and correctly signed.
        let huge = u64::MAX;

        assert_eq!(DiffValue::new(huge, 0, disabled_format()).net.raw, i64::MAX);
        assert_eq!(
            DiffValue::new(0, huge, disabled_format()).net.raw,
            i64::MIN + 1
        );
        // Both sides clamp to i64::MAX, so an all-huge cell nets to zero
        // rather than to the -1 an unchecked cast would produce.
        assert_eq!(DiffValue::new(huge, huge, disabled_format()).net.raw, 0);
    }

    #[test]
    fn diff_value_carries_locale_formatted_display_values() {
        let value = DiffValue::new(
            3805,
            1200,
            NumberFormat::from_locale_name(Some("de-DE")).unwrap(),
        );

        assert_eq!(value.added.raw, 3805);
        assert_eq!(value.added.display, "3.805");
        assert_eq!(value.removed.raw, 1200);
        assert_eq!(value.removed.display, "1.200");
        assert_eq!(value.net.raw, 2605);
        assert_eq!(value.net.display, "2.605");
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
            metadata: DiffReportMetadata {
                from_commit: "HEAD".to_string(),
                to_commit: "working tree".to_string(),
                non_rust_added: 0,
                non_rust_removed: 0,
            },
            total_items: 0,
            top_applied: false,
        };
        let view = DiffView::from_queryset(&qs, disabled_format());

        assert_eq!(view.non_rust.added.raw, 0);
        assert_eq!(view.non_rust.removed.raw, 0);
        assert_eq!(view.non_rust.net.raw, 0);
    }

    #[test]
    fn diff_view_passes_file_change_type_as_a_typed_fact() {
        let qs = DiffQuerySet {
            aggregation: Aggregation::ByFile,
            line_types: LineTypes::everything(),
            items: vec![
                QueryItem {
                    label: "added.rs".to_string(),
                    change_type: Some(FileChangeType::Added),
                    stats: LocsDiff::default(),
                },
                QueryItem {
                    label: "gone.rs".to_string(),
                    change_type: Some(FileChangeType::Deleted),
                    stats: LocsDiff::default(),
                },
                QueryItem {
                    label: "kept.rs".to_string(),
                    change_type: Some(FileChangeType::Modified),
                    stats: LocsDiff::default(),
                },
            ],
            total: LocsDiff::default(),
            file_count: 3,
            metadata: DiffReportMetadata {
                from_commit: "HEAD~1".to_string(),
                to_commit: "HEAD".to_string(),
                non_rust_added: 0,
                non_rust_removed: 0,
            },
            total_items: 3,
            top_applied: false,
        };
        let view = DiffView::from_queryset(&qs, disabled_format());

        assert_eq!(view.rows[0].change_type, Some(FileChangeType::Added));
        assert_eq!(view.rows[1].change_type, Some(FileChangeType::Deleted));
        assert_eq!(view.rows[2].change_type, Some(FileChangeType::Modified));
    }

    #[test]
    fn native_tabular_count_prototype_matches_the_approved_row() {
        // Count needs no display strings in its typed presentation data. The
        // native formatter can consume the label and number directly and
        // reproduce start truncation plus numeric alignment byte-for-byte.
        let row = Row {
            label: "src/this_is_a_deliberately_long_ascii_filename_for_the_parity_gate.rs"
                .to_string(),
            values: vec![DisplayNumber::u64(61_u64, disabled_format())],
            change_type: None,
        };
        let spec = TabularSpec::builder()
            .column(Col::fixed(40).truncate_start())
            .column(Col::fixed(4).right())
            .separator(" ")
            .build();
        let formatter = TabularFormatter::new(&spec, 45);
        let value = row.values[0].display.as_str();

        assert_eq!(
            formatter.format_row(&[row.label.as_str(), value]),
            "…g_ascii_filename_for_the_parity_gate.rs   61"
        );
    }

    #[test]
    fn native_tabular_diff_prototype_exposes_style_padding_gap() {
        // This is the smallest 7.6.2 reproducer for the parity blocker. The
        // typed value is split into native subcolumns; the spec, not the data,
        // owns alignment and semantic styles. A wider peer in the same logical
        // column makes this row need one leading space in +added and -removed.
        let value = DiffValue::new(1, 0, disabled_format());
        let subcolumns = SubColumns::new(
            vec![
                SubCol::fixed(3).right().style("additions"),
                SubCol::fixed(3).right().style("deletions"),
                SubCol::fill().right(),
            ],
            "/",
        )
        .unwrap();
        let spec = TabularSpec::builder()
            .column(Col::fixed(10).sub_columns(subcolumns))
            .build();
        let formatter = TabularFormatter::new(&spec, 10);
        let added = format!("+{}", value.added.display);
        let removed = format!("-{}", value.removed.display);
        let net = value.net.display.as_str();

        let native = formatter.format_row_cells(&[CellValue::Sub(vec![
            added.as_str(),
            removed.as_str(),
            net,
        ])]);
        let approved = " [additions]+1[/additions]/ [deletions]-0[/deletions]/ 1";

        assert_eq!(
            native,
            "[additions] +1[/additions]/[deletions] -0[/deletions]/ 1"
        );
        assert_ne!(
            native, approved,
            "Standout 7.6.2 unexpectedly gained padding-outside-style parity; reassess migration"
        );
    }
}
