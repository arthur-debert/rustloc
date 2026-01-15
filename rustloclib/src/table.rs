//! Table-ready data structures for LOC output.
//!
//! This module provides `LOCTable`, a presentation-ready data structure
//! that can be directly consumed by templates or serialized to JSON.
//!
//! The data flow is:
//! 1. Raw collection (per-file stats)
//! 2. Query processing (aggregation + line type filters + ordering)
//! 3. LOCTable (table-ready: headers, rows, footer)

use serde::{Deserialize, Serialize};

use crate::counter::CountResult;
use crate::diff::{DiffResult, LocsDiff};
use crate::options::{Aggregation, LineTypes, OrderBy, OrderDirection, Ordering};
use crate::stats::Locs;

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
}

impl LOCTable {
    /// Create a LOCTable from a CountResult.
    ///
    /// Applies aggregation level, line type filters, and ordering to produce
    /// a table-ready structure.
    pub fn from_count(
        result: &CountResult,
        aggregation: Aggregation,
        line_types: LineTypes,
        ordering: Ordering,
    ) -> Self {
        let headers = build_headers(&aggregation, &line_types);
        let rows = build_count_rows(result, &aggregation, &line_types, &ordering);
        let footer = build_count_footer(result, &line_types);

        LOCTable {
            title: None,
            headers,
            rows,
            footer,
        }
    }

    /// Create a LOCTable from a DiffResult.
    ///
    /// Applies aggregation level, line type filters, and ordering to produce
    /// a table-ready structure with diff formatting (+added/-removed/net).
    pub fn from_diff(
        result: &DiffResult,
        aggregation: Aggregation,
        line_types: LineTypes,
        ordering: Ordering,
    ) -> Self {
        let headers = build_headers(&aggregation, &line_types);
        let rows = build_diff_rows(result, &aggregation, &line_types, &ordering);
        let footer = build_diff_footer(result, &line_types);
        let title = Some(format!(
            "Diff: {} → {}",
            result.from_commit, result.to_commit
        ));

        LOCTable {
            title,
            headers,
            rows,
            footer,
        }
    }
}

/// Build column headers based on aggregation level and enabled line types.
fn build_headers(aggregation: &Aggregation, line_types: &LineTypes) -> Vec<String> {
    let label_header = match aggregation {
        Aggregation::Total => "Name".to_string(),
        Aggregation::ByCrate => "Crate".to_string(),
        Aggregation::ByModule => "Module".to_string(),
        Aggregation::ByFile => "File".to_string(),
    };

    let mut headers = vec![label_header];

    if line_types.code {
        headers.push("Code".to_string());
    }
    if line_types.tests {
        headers.push("Tests".to_string());
    }
    if line_types.examples {
        headers.push("Examples".to_string());
    }
    if line_types.docs {
        headers.push("Docs".to_string());
    }
    if line_types.comments {
        headers.push("Comments".to_string());
    }
    if line_types.blanks {
        headers.push("Blanks".to_string());
    }
    headers.push("Total".to_string());

    headers
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

/// Build values for a count row from Locs.
fn count_values(locs: &Locs, line_types: &LineTypes) -> Vec<String> {
    let mut values = Vec::new();

    if line_types.code {
        values.push(locs.code.to_string());
    }
    if line_types.tests {
        values.push(locs.tests.to_string());
    }
    if line_types.examples {
        values.push(locs.examples.to_string());
    }
    if line_types.docs {
        values.push(locs.docs.to_string());
    }
    if line_types.comments {
        values.push(locs.comments.to_string());
    }
    if line_types.blanks {
        values.push(locs.blanks.to_string());
    }
    values.push(locs_filtered_total(locs, line_types).to_string());

    values
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

/// Build data rows from CountResult based on aggregation level.
fn build_count_rows(
    result: &CountResult,
    aggregation: &Aggregation,
    line_types: &LineTypes,
    ordering: &Ordering,
) -> Vec<TableRow> {
    // Collect items with their labels and stats for sorting
    let mut items: Vec<(String, Locs)> = match aggregation {
        Aggregation::Total => return vec![],
        Aggregation::ByCrate => result
            .crates
            .iter()
            .filter(|c| locs_filtered_total(&c.stats, line_types) > 0)
            .map(|c| (c.name.clone(), c.stats))
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
                (label, m.stats)
            })
            .collect(),
        Aggregation::ByFile => result
            .files
            .iter()
            .filter(|f| locs_filtered_total(&f.stats, line_types) > 0)
            .map(|f| (f.path.to_string_lossy().to_string(), f.stats))
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

    // Map to TableRows
    items
        .into_iter()
        .map(|(label, locs)| TableRow {
            label,
            values: count_values(&locs, line_types),
        })
        .collect()
}

/// Build footer row from CountResult.
fn build_count_footer(result: &CountResult, line_types: &LineTypes) -> TableRow {
    TableRow {
        label: format!("Total ({} files)", result.file_count),
        values: count_values(&result.total, line_types),
    }
}

/// Compute filtered total for a LocsDiff struct.
fn locs_diff_filtered_total(diff: &LocsDiff, line_types: &LineTypes) -> (u64, u64) {
    let added = locs_filtered_total(&diff.added, line_types);
    let removed = locs_filtered_total(&diff.removed, line_types);
    (added, removed)
}

/// Format a diff value as "+added/-removed/net".
fn format_diff(added: u64, removed: u64) -> String {
    let net = added as i64 - removed as i64;
    format!("+{}/-{}/{}", added, removed, net)
}

/// Build values for a diff row from LocsDiff.
fn diff_values(diff: &LocsDiff, line_types: &LineTypes) -> Vec<String> {
    let mut values = Vec::new();

    if line_types.code {
        values.push(format_diff(diff.added.code, diff.removed.code));
    }
    if line_types.tests {
        values.push(format_diff(diff.added.tests, diff.removed.tests));
    }
    if line_types.examples {
        values.push(format_diff(diff.added.examples, diff.removed.examples));
    }
    if line_types.docs {
        values.push(format_diff(diff.added.docs, diff.removed.docs));
    }
    if line_types.comments {
        values.push(format_diff(diff.added.comments, diff.removed.comments));
    }
    if line_types.blanks {
        values.push(format_diff(diff.added.blanks, diff.removed.blanks));
    }

    let (total_added, total_removed) = locs_diff_filtered_total(diff, line_types);
    values.push(format_diff(total_added, total_removed));

    values
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

/// Build data rows from DiffResult based on aggregation level.
fn build_diff_rows(
    result: &DiffResult,
    aggregation: &Aggregation,
    line_types: &LineTypes,
    ordering: &Ordering,
) -> Vec<TableRow> {
    // Collect items with their labels and diff stats for sorting
    let mut items: Vec<(String, LocsDiff)> = match aggregation {
        Aggregation::Total => return vec![],
        Aggregation::ByCrate => result
            .crates
            .iter()
            .filter(|c| has_net_change(&c.diff, line_types))
            .map(|c| (c.name.clone(), c.diff))
            .collect(),
        Aggregation::ByModule => return vec![], // Diff doesn't support by-module currently
        Aggregation::ByFile => result
            .files
            .iter()
            .filter(|f| has_net_change(&f.diff, line_types))
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

    // Map to TableRows
    items
        .into_iter()
        .map(|(label, diff)| TableRow {
            label,
            values: diff_values(&diff, line_types),
        })
        .collect()
}

/// Build footer row from DiffResult.
fn build_diff_footer(result: &DiffResult, line_types: &LineTypes) -> TableRow {
    let file_count = result.files.len();
    TableRow {
        label: format!("Total ({} files)", file_count),
        values: diff_values(&result.total, line_types),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::CrateStats;
    use std::path::PathBuf;

    fn sample_locs(code: u64, tests: u64) -> Locs {
        Locs {
            code,
            tests,
            examples: 0,
            docs: 0,
            comments: 0,
            blanks: 0,
        }
    }

    fn sample_count_result() -> CountResult {
        CountResult {
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
        let headers = build_headers(&Aggregation::ByCrate, &LineTypes::all());
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
        assert_eq!(headers.len(), 3); // File, Code, Total
        assert_eq!(headers[0], "File");
        assert_eq!(headers[1], "Code");
        assert_eq!(headers[2], "Total");
    }

    #[test]
    fn test_count_values() {
        let locs = sample_locs(100, 50);
        let values = count_values(&locs, &LineTypes::all());
        assert_eq!(values[0], "100"); // Code
        assert_eq!(values[1], "50"); // Tests
        assert_eq!(values[2], "0"); // Examples
        assert_eq!(values[3], "0"); // Docs
        assert_eq!(values[4], "0"); // Comments
        assert_eq!(values[5], "0"); // Blanks
        assert_eq!(values[6], "150"); // Total
    }

    #[test]
    fn test_loc_table_from_count() {
        let result = sample_count_result();
        let table = LOCTable::from_count(
            &result,
            Aggregation::ByCrate,
            LineTypes::all(),
            Ordering::default(),
        );

        assert!(table.title.is_none());
        assert_eq!(table.headers[0], "Crate");
        assert_eq!(table.rows.len(), 2);
        // Default ordering is by label ascending: alpha before beta
        assert_eq!(table.rows[0].label, "alpha");
        assert_eq!(table.rows[1].label, "beta");
        assert_eq!(table.footer.label, "Total (4 files)");
    }

    #[test]
    fn test_ordering_by_label_ascending() {
        let result = sample_count_result();
        let table = LOCTable::from_count(
            &result,
            Aggregation::ByCrate,
            LineTypes::all(),
            Ordering::by_label(),
        );

        assert_eq!(table.rows[0].label, "alpha");
        assert_eq!(table.rows[1].label, "beta");
    }

    #[test]
    fn test_ordering_by_label_descending() {
        let result = sample_count_result();
        let table = LOCTable::from_count(
            &result,
            Aggregation::ByCrate,
            LineTypes::all(),
            Ordering::by_label().descending(),
        );

        assert_eq!(table.rows[0].label, "beta");
        assert_eq!(table.rows[1].label, "alpha");
    }

    #[test]
    fn test_ordering_by_code_descending() {
        let result = sample_count_result();
        let table = LOCTable::from_count(
            &result,
            Aggregation::ByCrate,
            LineTypes::all(),
            Ordering::by_code(), // Descending by default
        );

        // beta has 150 code, alpha has 50
        assert_eq!(table.rows[0].label, "beta");
        assert_eq!(table.rows[0].values[0], "150");
        assert_eq!(table.rows[1].label, "alpha");
        assert_eq!(table.rows[1].values[0], "50");
    }

    #[test]
    fn test_ordering_by_code_ascending() {
        let result = sample_count_result();
        let table = LOCTable::from_count(
            &result,
            Aggregation::ByCrate,
            LineTypes::all(),
            Ordering::by_code().ascending(),
        );

        // alpha has 50 code, beta has 150
        assert_eq!(table.rows[0].label, "alpha");
        assert_eq!(table.rows[1].label, "beta");
    }

    #[test]
    fn test_ordering_by_total_descending() {
        let result = sample_count_result();
        let table = LOCTable::from_count(
            &result,
            Aggregation::ByCrate,
            LineTypes::all(),
            Ordering::by_total(),
        );

        // beta has 225 total, alpha has 75
        assert_eq!(table.rows[0].label, "beta");
        assert_eq!(table.rows[1].label, "alpha");
    }

    #[test]
    fn test_format_diff() {
        assert_eq!(format_diff(10, 5), "+10/-5/5");
        assert_eq!(format_diff(5, 10), "+5/-10/-5");
        assert_eq!(format_diff(0, 0), "+0/-0/0");
    }
}
