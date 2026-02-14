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
            label: build_footer_label(&qs.aggregation, rows.len(), qs.file_count),
            values: format_locs(&qs.total, &qs.line_types),
        };

        LOCTable {
            title: None,
            headers,
            rows,
            footer,
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
        let rows: Vec<TableRow> = qs
            .items
            .iter()
            .map(|item| TableRow {
                label: item.label.clone(),
                values: format_locs_diff(&item.stats, &qs.line_types),
            })
            .collect();
        let footer = TableRow {
            label: build_footer_label(&qs.aggregation, rows.len(), qs.file_count),
            values: format_locs_diff(&qs.total, &qs.line_types),
        };
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
            non_rust_summary,
            legend: Some("(+added / -removed / net)".to_string()),
        }
    }
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
/// For Total aggregation (no item rows), reports the total file count.
/// For other aggregations, reports the number of displayed items with the matching unit.
fn build_footer_label(aggregation: &Aggregation, items_count: usize, file_count: usize) -> String {
    match aggregation {
        Aggregation::Total => format!("Total ({} files)", file_count),
        Aggregation::ByCrate => format!("Total ({} crates)", items_count),
        Aggregation::ByModule => format!("Total ({} modules)", items_count),
        Aggregation::ByFile => format!("Total ({} files)", items_count),
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

/// Format a diff value as "+added/-removed/net" with standout style tags.
fn format_diff_value(added: u64, removed: u64) -> String {
    let net = added as i64 - removed as i64;
    format!(
        "[additions]+{}[/additions]/[deletions]-{}[/deletions]/{}",
        added, removed, net
    )
}

/// Format LocsDiff values as strings for display.
fn format_locs_diff(diff: &LocsDiff, line_types: &crate::query::options::LineTypes) -> Vec<String> {
    let lt = LineTypesView::from(line_types);
    let mut values = Vec::new();

    if lt.code {
        values.push(format_diff_value(diff.added.code, diff.removed.code));
    }
    if lt.tests {
        values.push(format_diff_value(diff.added.tests, diff.removed.tests));
    }
    if lt.examples {
        values.push(format_diff_value(
            diff.added.examples,
            diff.removed.examples,
        ));
    }
    if lt.docs {
        values.push(format_diff_value(diff.added.docs, diff.removed.docs));
    }
    if lt.comments {
        values.push(format_diff_value(
            diff.added.comments,
            diff.removed.comments,
        ));
    }
    if lt.blanks {
        values.push(format_diff_value(diff.added.blanks, diff.removed.blanks));
    }
    if lt.total {
        // Use the precomputed all fields
        values.push(format_diff_value(diff.added.total, diff.removed.total));
    }

    values
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
    fn test_format_diff_value() {
        assert_eq!(
            format_diff_value(10, 5),
            "[additions]+10[/additions]/[deletions]-5[/deletions]/5"
        );
        assert_eq!(
            format_diff_value(5, 10),
            "[additions]+5[/additions]/[deletions]-10[/deletions]/-5"
        );
        assert_eq!(
            format_diff_value(0, 0),
            "[additions]+0[/additions]/[deletions]-0[/deletions]/0"
        );
    }
}
