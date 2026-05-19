//! Table-ready data structures for LOC output.
//!
//! This module provides `LOCTable`, a presentation-ready data structure
//! that can be directly consumed by templates or serialized to JSON.
//!
//! The data flow is:
//! 1. Raw Data (CountResult, DiffResult)
//! 2. QuerySet (filtered, aggregated, sorted)
//! 3. LOCTable (formatted strings for display)
//!
//! LOCTable is a pure presentation layer - it only formats data, no filtering
//! or sorting logic. All computation happens in the QuerySet layer.

use serde::{Deserialize, Serialize};

use crate::data::diff::LocsDiff;
use crate::data::stats::Locs;
use crate::query::options::Aggregation;
use crate::query::queryset::{CountQuerySet, DiffQuerySet};

/// A single row in the table (data row or footer).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableRow {
    /// Row label (file path, crate name, "Total (N files)", etc.)
    pub label: String,
    /// Values for each category column (as strings, ready for display)
    pub values: Vec<String>,
}

/// Table-ready LOC data.
///
/// This is the final data structure before presentation. Templates
/// iterate over headers/rows/footer and apply formatting - no computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LOCTable {
    /// Optional title (e.g., "Diff: HEAD~5 → HEAD")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Column headers: [label_header, line_type1, line_type2, ..., Total]
    pub headers: Vec<String>,
    /// Data rows
    pub rows: Vec<TableRow>,
    /// Summary/footer row
    pub footer: TableRow,
    /// Per-value-column display widths (one per header[1..]). Each width is
    /// the max visible width across that column's header, all row values,
    /// and the footer value, so per-row items right-align to the widest
    /// value — usually the footer ("Total") — and the total never has
    /// fewer digits than any per-row entry.
    #[serde(default)]
    pub value_widths: Vec<usize>,
    /// Optional non-Rust changes summary (e.g., "Non-Rust changes: +10/-5/5 net")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub non_rust_summary: Option<String>,
    /// Optional legend text below the table (e.g., "+added / -removed / net")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legend: Option<String>,
}

impl LOCTable {
    /// Create a LOCTable from a CountQuerySet.
    ///
    /// The QuerySet already contains filtered, aggregated, and sorted data.
    /// This method just formats it into displayable strings.
    pub fn from_count_queryset(qs: &CountQuerySet) -> Self {
        let headers = build_headers(&qs.aggregation, &qs.line_types);
        let rows: Vec<TableRow> = qs
            .items
            .iter()
            .map(|item| TableRow {
                label: item.label.clone(),
                values: format_locs(&item.stats, &qs.line_types),
            })
            .collect();
        let footer = TableRow {
            label: build_footer_label(
                &qs.aggregation,
                rows.len(),
                qs.total_items,
                qs.file_count,
                qs.top_applied,
            ),
            values: format_locs(&qs.total, &qs.line_types),
        };
        let value_widths = compute_value_widths(&headers, &rows, &footer);

        LOCTable {
            title: None,
            headers,
            rows,
            footer,
            value_widths,
            non_rust_summary: None,
            legend: None,
        }
    }

    /// Create a LOCTable from a DiffQuerySet.
    ///
    /// The QuerySet already contains filtered, aggregated, and sorted data.
    /// This method just formats it into displayable strings with diff notation.
    pub fn from_diff_queryset(qs: &DiffQuerySet) -> Self {
        let headers = build_headers(&qs.aggregation, &qs.line_types);

        // Two-pass formatting so the `+added`, `-removed`, and `net` sub-fields
        // line up across every row in a column: gather raw pairs first,
        // compute per-column sub-widths from rows+footer, then render every
        // cell padded to those widths.
        let raw_rows: Vec<Vec<(u64, u64)>> = qs
            .items
            .iter()
            .map(|item| diff_pairs(&item.stats, &qs.line_types))
            .collect();
        let raw_footer = diff_pairs(&qs.total, &qs.line_types);
        let sub_widths = compute_diff_sub_widths(&raw_rows, &raw_footer);

        let rows: Vec<TableRow> = qs
            .items
            .iter()
            .zip(&raw_rows)
            .map(|(item, pairs)| TableRow {
                label: item.label.clone(),
                values: format_diff_row(pairs, &sub_widths),
            })
            .collect();
        let footer = TableRow {
            label: build_footer_label(
                &qs.aggregation,
                rows.len(),
                qs.total_items,
                qs.file_count,
                qs.top_applied,
            ),
            values: format_diff_row(&raw_footer, &sub_widths),
        };
        let value_widths = compute_value_widths(&headers, &rows, &footer);
        let title = Some(format!("Diff: {} → {}", qs.from_commit, qs.to_commit));

        let non_rust_summary = if qs.non_rust_added > 0 || qs.non_rust_removed > 0 {
            let nr_net = qs.non_rust_added as i64 - qs.non_rust_removed as i64;
            Some(format!(
                    "Non-Rust changes: [additions]+{}[/additions] / [deletions]-{}[/deletions] / {} net",
                    qs.non_rust_added, qs.non_rust_removed, nr_net
                ))
        } else {
            None
        };

        LOCTable {
            title,
            headers,
            rows,
            footer,
            value_widths,
            non_rust_summary,
            legend: Some("(+added / -removed / net)".to_string()),
        }
    }
}

/// Visible width of a string, skipping BBCode-style `[tag]`/`[/tag]` markup
/// that the renderer strips before display. Mirrors what `standout`'s
/// `strip_tags` does for the simple tag forms our diff values produce —
/// the only markup we emit is `[additions]`, `[/additions]`, `[deletions]`,
/// `[/deletions]`, so a naive bracket-balanced skip is sufficient.
fn visible_width(s: &str) -> usize {
    let mut count = 0usize;
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '[' => in_tag = true,
            ']' if in_tag => in_tag = false,
            _ if !in_tag => count += 1,
            _ => {}
        }
    }
    count
}

/// For each value column, return the max visible width across its header,
/// every row value, and the footer value. The result has length
/// `headers.len() - 1` (the label header is not a value column).
fn compute_value_widths(headers: &[String], rows: &[TableRow], footer: &TableRow) -> Vec<usize> {
    if headers.len() <= 1 {
        return Vec::new();
    }
    (0..headers.len() - 1)
        .map(|i| {
            let header_w = visible_width(&headers[i + 1]);
            let row_w = rows
                .iter()
                .filter_map(|r| r.values.get(i))
                .map(|v| visible_width(v))
                .max()
                .unwrap_or(0);
            let footer_w = footer.values.get(i).map(|v| visible_width(v)).unwrap_or(0);
            header_w.max(row_w).max(footer_w)
        })
        .collect()
}

/// Line types configuration for header building.
/// This is a local copy of the fields we need to avoid circular imports.
struct LineTypesView {
    code: bool,
    tests: bool,
    examples: bool,
    docs: bool,
    comments: bool,
    blanks: bool,
    total: bool,
}

impl From<&crate::query::options::LineTypes> for LineTypesView {
    fn from(lt: &crate::query::options::LineTypes) -> Self {
        LineTypesView {
            code: lt.code,
            tests: lt.tests,
            examples: lt.examples,
            docs: lt.docs,
            comments: lt.comments,
            blanks: lt.blanks,
            total: lt.total,
        }
    }
}

/// Build footer label based on aggregation level.
///
/// `displayed` is the number of rows actually shown after any user-driven
/// reduction. `total` is the pre-reduction row count. When they differ the
/// label makes the gap explicit so the reader can see that the totals row
/// reflects more data than what's visible.
///
/// `top_applied` distinguishes the two reduction paths so the wording is
/// honest: "top X of Y" for `--top` (a sorted slice), plain "X of Y" for
/// filter-eliminated rows. Both can apply at once, in which case the
/// "top" wording dominates because the visible rows ARE the top of what
/// passed the filter.
///
/// `total < displayed` is logically impossible (reductions only shrink
/// the row set), but a deserialized queryset from a payload that pre-dates
/// the `total_items` field will arrive with `total = 0`. The `.max` clamp
/// keeps the footer correct for that case rather than rendering nonsense
/// like "Total (0 crates)" alongside two visible rows.
fn build_footer_label(
    aggregation: &Aggregation,
    displayed: usize,
    total: usize,
    file_count: usize,
    top_applied: bool,
) -> String {
    let unit = match aggregation {
        Aggregation::Total => return format!("Total ({} files)", file_count),
        Aggregation::ByCrate => "crates",
        Aggregation::ByModule => "modules",
        Aggregation::ByFile => "files",
    };
    let total = total.max(displayed);
    if displayed == total {
        format!("Total ({} {})", total, unit)
    } else if top_applied {
        format!("Total (top {} of {} {})", displayed, total, unit)
    } else {
        format!("Total ({} of {} {})", displayed, total, unit)
    }
}

/// Build column headers based on aggregation level and enabled line types.
fn build_headers(
    aggregation: &Aggregation,
    line_types: &crate::query::options::LineTypes,
) -> Vec<String> {
    let label_header = match aggregation {
        Aggregation::Total => "Name".to_string(),
        Aggregation::ByCrate => "Crate".to_string(),
        Aggregation::ByModule => "Module".to_string(),
        Aggregation::ByFile => "File".to_string(),
    };

    let lt = LineTypesView::from(line_types);
    let mut headers = vec![label_header];

    if lt.code {
        headers.push("Code".to_string());
    }
    if lt.tests {
        headers.push("Tests".to_string());
    }
    if lt.examples {
        headers.push("Examples".to_string());
    }
    if lt.docs {
        headers.push("Docs".to_string());
    }
    if lt.comments {
        headers.push("Comments".to_string());
    }
    if lt.blanks {
        headers.push("Blanks".to_string());
    }
    if lt.total {
        headers.push("Total".to_string());
    }

    headers
}

/// Format Locs values as strings for display.
fn format_locs(locs: &Locs, line_types: &crate::query::options::LineTypes) -> Vec<String> {
    let lt = LineTypesView::from(line_types);
    let mut values = Vec::new();

    if lt.code {
        values.push(locs.code.to_string());
    }
    if lt.tests {
        values.push(locs.tests.to_string());
    }
    if lt.examples {
        values.push(locs.examples.to_string());
    }
    if lt.docs {
        values.push(locs.docs.to_string());
    }
    if lt.comments {
        values.push(locs.comments.to_string());
    }
    if lt.blanks {
        values.push(locs.blanks.to_string());
    }
    if lt.total {
        // Use the precomputed all field
        values.push(locs.total.to_string());
    }

    values
}

/// Per-column max widths of the three sub-fields inside a diff value
/// (`+added/-removed/net`). Used to right-align each sub-field across
/// every row in the same column so the `/` separators line up.
#[derive(Debug, Clone, Default)]
struct DiffSubWidths {
    added: usize,
    removed: usize,
    net: usize,
}

/// Format a diff value as "+added/-removed/net" with standout style tags,
/// right-padding each sub-field to the given widths. Padding spaces sit
/// *outside* the style tags so coloring stays on the digits only. Passing
/// zero widths produces a tight unpadded value.
fn format_diff_value_padded(added: u64, removed: u64, w: &DiffSubWidths) -> String {
    let net = added as i64 - removed as i64;
    let added_str = format!("+{}", added);
    let removed_str = format!("-{}", removed);
    let net_str = net.to_string();
    let apad = " ".repeat(w.added.saturating_sub(added_str.len()));
    let rpad = " ".repeat(w.removed.saturating_sub(removed_str.len()));
    let npad = " ".repeat(w.net.saturating_sub(net_str.len()));
    format!(
        "{}[additions]{}[/additions]/{}[deletions]{}[/deletions]/{}{}",
        apad, added_str, rpad, removed_str, npad, net_str,
    )
}

/// Return raw `(added, removed)` pairs in the order of enabled line types.
/// The shape matches what `build_headers` / `format_locs_diff` produce, so
/// callers can pair widths with cells positionally.
fn diff_pairs(diff: &LocsDiff, line_types: &crate::query::options::LineTypes) -> Vec<(u64, u64)> {
    let lt = LineTypesView::from(line_types);
    let mut pairs = Vec::new();
    if lt.code {
        pairs.push((diff.added.code, diff.removed.code));
    }
    if lt.tests {
        pairs.push((diff.added.tests, diff.removed.tests));
    }
    if lt.examples {
        pairs.push((diff.added.examples, diff.removed.examples));
    }
    if lt.docs {
        pairs.push((diff.added.docs, diff.removed.docs));
    }
    if lt.comments {
        pairs.push((diff.added.comments, diff.removed.comments));
    }
    if lt.blanks {
        pairs.push((diff.added.blanks, diff.removed.blanks));
    }
    if lt.total {
        pairs.push((diff.added.total, diff.removed.total));
    }
    pairs
}

/// For each value column, compute the max widths of the `+added`, `-removed`,
/// and `net` sub-fields across every row and the footer. With these widths,
/// the three `/`-separated sub-fields line up across every row in the column
/// — the usual case is the footer wins each sub-width because it sums the
/// rows.
fn compute_diff_sub_widths(rows: &[Vec<(u64, u64)>], footer: &[(u64, u64)]) -> Vec<DiffSubWidths> {
    (0..footer.len())
        .map(|i| {
            let mut w = DiffSubWidths::default();
            let update = |a: u64, r: u64, w: &mut DiffSubWidths| {
                let aw = format!("+{}", a).len();
                let rw = format!("-{}", r).len();
                let nw = (a as i64 - r as i64).to_string().len();
                w.added = w.added.max(aw);
                w.removed = w.removed.max(rw);
                w.net = w.net.max(nw);
            };
            for row in rows {
                if let Some(&(a, r)) = row.get(i) {
                    update(a, r, &mut w);
                }
            }
            update(footer[i].0, footer[i].1, &mut w);
            w
        })
        .collect()
}

/// Format every `(added, removed)` pair against its column's sub-widths.
fn format_diff_row(pairs: &[(u64, u64)], widths: &[DiffSubWidths]) -> Vec<String> {
    pairs
        .iter()
        .zip(widths)
        .map(|(&(a, r), w)| format_diff_value_padded(a, r, w))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::counter::CountResult;
    use crate::data::stats::CrateStats;
    use crate::query::options::{LineTypes, Ordering};
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
    fn test_headers_by_crate() {
        let headers = build_headers(&Aggregation::ByCrate, &LineTypes::everything());
        assert_eq!(headers[0], "Crate");
        assert_eq!(headers[1], "Code");
        assert_eq!(headers[2], "Tests");
        assert_eq!(headers[3], "Examples");
        assert_eq!(headers[4], "Docs");
        assert_eq!(headers[5], "Comments");
        assert_eq!(headers[6], "Blanks");
        assert_eq!(headers[7], "Total");
    }

    #[test]
    fn test_headers_filtered_line_types() {
        let line_types = LineTypes::new().with_code();
        let headers = build_headers(&Aggregation::ByFile, &line_types);
        assert_eq!(headers.len(), 3); // File, Code, All
        assert_eq!(headers[0], "File");
        assert_eq!(headers[1], "Code");
        assert_eq!(headers[2], "Total");
    }

    #[test]
    fn test_format_locs() {
        let locs = sample_locs(100, 50);
        let values = format_locs(&locs, &LineTypes::everything());
        assert_eq!(values[0], "100"); // Code
        assert_eq!(values[1], "50"); // Tests
        assert_eq!(values[2], "0"); // Examples
        assert_eq!(values[3], "0"); // Docs
        assert_eq!(values[4], "0"); // Comments
        assert_eq!(values[5], "0"); // Blanks
        assert_eq!(values[6], "150"); // All
    }

    #[test]
    fn test_loc_table_from_queryset() {
        let result = sample_count_result();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        );
        let table = LOCTable::from_count_queryset(&qs);

        assert!(table.title.is_none());
        assert_eq!(table.headers[0], "Crate");
        assert_eq!(table.rows.len(), 2);
        // Default ordering is by label ascending: alpha before beta
        assert_eq!(table.rows[0].label, "alpha");
        assert_eq!(table.rows[1].label, "beta");
        assert_eq!(table.footer.label, "Total (2 crates)");
    }

    #[test]
    fn test_footer_label_marks_truncation_when_top_applied() {
        let result = sample_count_result();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        )
        .top(1);
        let table = LOCTable::from_count_queryset(&qs);

        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.footer.label, "Total (top 1 of 2 crates)");
    }

    #[test]
    fn test_footer_label_filter_only_uses_plain_x_of_y() {
        // When rows are reduced by a filter (not by --top), the footer
        // says "X of Y" without the "top" qualifier, because the visible
        // rows aren't sorted-and-sliced — they're just the ones that
        // passed the predicate.
        use crate::query::options::{Field, Op, Predicate};

        let result = sample_count_result();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        )
        .filter(&[Predicate::new(Field::Code, Op::Gte, 100)]);
        let table = LOCTable::from_count_queryset(&qs);

        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.footer.label, "Total (1 of 2 crates)");
    }

    #[test]
    fn test_footer_label_filter_then_top_uses_top_wording() {
        use crate::query::options::{Field, Op, Predicate};

        let result = sample_count_result();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        )
        .filter(&[Predicate::new(Field::Code, Op::Gte, 50)])
        .top(1);
        let table = LOCTable::from_count_queryset(&qs);

        // Both filter (kept 2) and top (kept 1) ran; "top" wording wins
        // because the visible row IS the top of what passed the filter.
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.footer.label, "Total (top 1 of 2 crates)");
    }

    #[test]
    fn test_footer_label_clamps_when_total_items_missing() {
        // A queryset deserialized from a payload that predates `total_items`
        // arrives with `total_items = 0`. The footer must still show the
        // correct row count rather than "Total (0 crates)".
        let result = sample_count_result();
        let mut qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        );
        qs.total_items = 0; // simulate stale payload
        let table = LOCTable::from_count_queryset(&qs);

        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.footer.label, "Total (2 crates)");
    }

    #[test]
    fn test_ordering_by_label_ascending() {
        let result = sample_count_result();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::by_label(),
        );
        let table = LOCTable::from_count_queryset(&qs);

        assert_eq!(table.rows[0].label, "alpha");
        assert_eq!(table.rows[1].label, "beta");
    }

    #[test]
    fn test_ordering_by_label_descending() {
        let result = sample_count_result();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::by_label().descending(),
        );
        let table = LOCTable::from_count_queryset(&qs);

        assert_eq!(table.rows[0].label, "beta");
        assert_eq!(table.rows[1].label, "alpha");
    }

    #[test]
    fn test_ordering_by_code_descending() {
        let result = sample_count_result();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::by_code(), // Descending by default
        );
        let table = LOCTable::from_count_queryset(&qs);

        // beta has 150 code, alpha has 50
        assert_eq!(table.rows[0].label, "beta");
        assert_eq!(table.rows[0].values[0], "150");
        assert_eq!(table.rows[1].label, "alpha");
        assert_eq!(table.rows[1].values[0], "50");
    }

    #[test]
    fn test_ordering_by_code_ascending() {
        let result = sample_count_result();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::by_code().ascending(),
        );
        let table = LOCTable::from_count_queryset(&qs);

        // alpha has 50 code, beta has 150
        assert_eq!(table.rows[0].label, "alpha");
        assert_eq!(table.rows[1].label, "beta");
    }

    #[test]
    fn test_ordering_by_total_descending() {
        let result = sample_count_result();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::by_total(),
        );
        let table = LOCTable::from_count_queryset(&qs);

        // beta has 225 total, alpha has 75
        assert_eq!(table.rows[0].label, "beta");
        assert_eq!(table.rows[1].label, "alpha");
    }

    #[test]
    fn test_format_diff_value_unpadded() {
        let zero = DiffSubWidths::default();
        assert_eq!(
            format_diff_value_padded(10, 5, &zero),
            "[additions]+10[/additions]/[deletions]-5[/deletions]/5"
        );
        assert_eq!(
            format_diff_value_padded(5, 10, &zero),
            "[additions]+5[/additions]/[deletions]-10[/deletions]/-5"
        );
        assert_eq!(
            format_diff_value_padded(0, 0, &zero),
            "[additions]+0[/additions]/[deletions]-0[/deletions]/0"
        );
    }

    #[test]
    fn test_format_diff_value_pads_sub_fields_outside_tags() {
        // Padding goes outside the style tags so coloring stays on digits.
        let w = DiffSubWidths {
            added: 4,
            removed: 2,
            net: 3,
        };
        assert_eq!(
            format_diff_value_padded(0, 0, &w),
            "  [additions]+0[/additions]/[deletions]-0[/deletions]/  0"
        );
        // Already-wide values keep zero padding.
        assert_eq!(
            format_diff_value_padded(111, 0, &w),
            "[additions]+111[/additions]/[deletions]-0[/deletions]/111"
        );
    }

    #[test]
    fn test_diff_sub_widths_align_separators_across_rows() {
        // For a column with rows (a=0, r=0) and (a=111, r=0), the footer
        // (a=111, r=0) determines the max widths. After padding, every
        // cell in the column has the same visible width, so the `/`
        // separators sit at the same offset in every row.
        let rows = vec![vec![(0u64, 0u64)], vec![(9u64, 0u64)]];
        let footer = vec![(111u64, 0u64)];
        let widths = compute_diff_sub_widths(&rows, &footer);
        assert_eq!(widths.len(), 1);
        assert_eq!(widths[0].added, "+111".len());
        assert_eq!(widths[0].removed, "-0".len());
        assert_eq!(widths[0].net, "111".len());

        let cell_a = format_diff_value_padded(0, 0, &widths[0]);
        let cell_b = format_diff_value_padded(9, 0, &widths[0]);
        let cell_c = format_diff_value_padded(111, 0, &widths[0]);
        // All cells render to the same visible width.
        let wa = visible_width(&cell_a);
        let wb = visible_width(&cell_b);
        let wc = visible_width(&cell_c);
        assert_eq!(wa, wb);
        assert_eq!(wb, wc);
        // And the `/` separators sit at the same visible positions.
        let slash_positions = |s: &str| -> Vec<usize> {
            let stripped: String = {
                let mut out = String::new();
                let mut in_tag = false;
                for c in s.chars() {
                    match c {
                        '[' => in_tag = true,
                        ']' if in_tag => in_tag = false,
                        _ if !in_tag => out.push(c),
                        _ => {}
                    }
                }
                out
            };
            stripped
                .char_indices()
                .filter(|&(_, c)| c == '/')
                .map(|(i, _)| i)
                .collect()
        };
        assert_eq!(slash_positions(&cell_a), slash_positions(&cell_c));
        assert_eq!(slash_positions(&cell_b), slash_positions(&cell_c));
    }

    #[test]
    fn test_visible_width_strips_bbcode_tags() {
        assert_eq!(visible_width("plain"), 5);
        assert_eq!(
            visible_width("[additions]+10[/additions]/[deletions]-5[/deletions]/5"),
            "+10/-5/5".len()
        );
        assert_eq!(visible_width("[header]Code[/header]"), 4);
    }

    #[test]
    fn test_compute_value_widths_picks_widest_cell_per_column() {
        // Each column's width = max(header, row values, footer value).
        // The footer here has the longest values, so widths match it.
        let zero = DiffSubWidths::default();
        let make = |a: u64, r: u64| format_diff_value_padded(a, r, &zero);
        let headers = vec!["File".to_string(), "Code".to_string(), "Total".to_string()];
        let rows = vec![
            TableRow {
                label: "a.rs".to_string(),
                values: vec![make(1, 0), make(1, 0)],
            },
            TableRow {
                label: "b.rs".to_string(),
                values: vec![make(9, 0), make(9, 0)],
            },
        ];
        let footer = TableRow {
            label: "Total (2 files)".to_string(),
            values: vec![make(100, 0), make(100, 0)],
        };

        let widths = compute_value_widths(&headers, &rows, &footer);
        assert_eq!(widths.len(), 2);
        // Footer wins both columns: "+100/-0/100" is 11 visible chars.
        assert_eq!(widths[0], visible_width("+100/-0/100"));
        assert_eq!(widths[1], visible_width("+100/-0/100"));
        // Every per-row value fits within its column — the total never
        // has fewer digits than any row entry.
        for row in &rows {
            for (i, v) in row.values.iter().enumerate() {
                assert!(visible_width(v) <= widths[i]);
            }
        }
    }

    #[test]
    fn test_value_widths_at_least_header_width() {
        // If every value is short ("0"), each column must still be wide
        // enough to fit its header word.
        let result = sample_count_result();
        let qs = CountQuerySet::from_result(
            &result,
            Aggregation::ByCrate,
            LineTypes::everything(),
            Ordering::default(),
        );
        let table = LOCTable::from_count_queryset(&qs);

        // headers[1..] = Code, Tests, Examples, Docs, Comments, Blanks, Total
        for (i, header) in table.headers[1..].iter().enumerate() {
            assert!(
                table.value_widths[i] >= header.len(),
                "column {} ({}) width {} < header {}",
                i,
                header,
                table.value_widths[i],
                header.len()
            );
        }
    }
}
