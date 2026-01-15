//! Table-ready data structures for LOC output.
//!
//! This module provides `LOCTable`, a presentation-ready data structure
//! that can be directly consumed by templates or serialized to JSON.
//!
//! The data flow is:
//! 1. Raw collection (per-file stats)
//! 2. QuerySet (aggregation + category filters applied)
//! 3. LOCTable (table-ready: headers, rows, footer)

use serde::{Deserialize, Serialize};

use crate::{Aggregation, Contexts, CountResult, DiffResult};

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
    /// Column headers: [label_header, category1, category2, ...]
    pub headers: Vec<String>,
    /// Data rows
    pub rows: Vec<TableRow>,
    /// Summary/footer row
    pub footer: TableRow,
}

impl LOCTable {
    /// Create a LOCTable from a CountResult.
    ///
    /// Applies aggregation level and category filters to produce
    /// a table-ready structure.
    pub fn from_count(result: &CountResult, aggregation: Aggregation, contexts: Contexts) -> Self {
        let headers = build_headers(&aggregation, &contexts);
        let rows = build_count_rows(result, &aggregation, &contexts);
        let footer = build_count_footer(result, &contexts);

        LOCTable {
            title: None,
            headers,
            rows,
            footer,
        }
    }

    /// Create a LOCTable from a DiffResult.
    ///
    /// Applies aggregation level and category filters to produce
    /// a table-ready structure with diff formatting (+added/-removed/net).
    pub fn from_diff(result: &DiffResult, aggregation: Aggregation, contexts: Contexts) -> Self {
        let headers = build_headers(&aggregation, &contexts);
        let rows = build_diff_rows(result, &aggregation, &contexts);
        let footer = build_diff_footer(result, &contexts);
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

/// Build column headers based on aggregation level and enabled contexts.
fn build_headers(aggregation: &Aggregation, contexts: &Contexts) -> Vec<String> {
    let label_header = match aggregation {
        Aggregation::Total => "Name".to_string(),
        Aggregation::ByCrate => "Crate".to_string(),
        Aggregation::ByModule => "Module".to_string(),
        Aggregation::ByFile => "File".to_string(),
    };

    let mut headers = vec![label_header];

    if contexts.code {
        headers.push("Code".to_string());
    }
    if contexts.tests {
        headers.push("Tests".to_string());
    }
    if contexts.examples {
        headers.push("Examples".to_string());
    }
    headers.push("Total".to_string());

    headers
}

/// Compute total for a Locs struct.
fn locs_total(locs: &crate::Locs) -> u64 {
    locs.blank + locs.comments + locs.docs + locs.logic
}

/// Compute total for a LocStats struct.
fn loc_stats_total(stats: &crate::LocStats, contexts: &Contexts) -> u64 {
    let mut total = 0;
    if contexts.code {
        total += locs_total(&stats.code);
    }
    if contexts.tests {
        total += locs_total(&stats.tests);
    }
    if contexts.examples {
        total += locs_total(&stats.examples);
    }
    total
}

/// Build values for a count row from LocStats.
fn count_values(stats: &crate::LocStats, contexts: &Contexts) -> Vec<String> {
    let mut values = Vec::new();

    if contexts.code {
        values.push(locs_total(&stats.code).to_string());
    }
    if contexts.tests {
        values.push(locs_total(&stats.tests).to_string());
    }
    if contexts.examples {
        values.push(locs_total(&stats.examples).to_string());
    }
    values.push(loc_stats_total(stats, contexts).to_string());

    values
}

/// Build data rows from CountResult based on aggregation level.
fn build_count_rows(
    result: &CountResult,
    aggregation: &Aggregation,
    contexts: &Contexts,
) -> Vec<TableRow> {
    match aggregation {
        Aggregation::Total => vec![],
        Aggregation::ByCrate => result
            .crates
            .iter()
            .filter(|c| loc_stats_total(&c.stats, contexts) > 0)
            .map(|c| TableRow {
                label: c.name.clone(),
                values: count_values(&c.stats, contexts),
            })
            .collect(),
        Aggregation::ByModule => result
            .modules
            .iter()
            .filter(|m| loc_stats_total(&m.stats, contexts) > 0)
            .map(|m| TableRow {
                label: if m.name.is_empty() {
                    "(root)".to_string()
                } else {
                    m.name.clone()
                },
                values: count_values(&m.stats, contexts),
            })
            .collect(),
        Aggregation::ByFile => result
            .files
            .iter()
            .filter(|f| loc_stats_total(&f.stats, contexts) > 0)
            .map(|f| TableRow {
                label: f.path.to_string_lossy().to_string(),
                values: count_values(&f.stats, contexts),
            })
            .collect(),
    }
}

/// Build footer row from CountResult.
fn build_count_footer(result: &CountResult, contexts: &Contexts) -> TableRow {
    TableRow {
        label: format!("Total ({} files)", result.total.file_count),
        values: count_values(&result.total, contexts),
    }
}

/// Compute total for a LocsDiff struct.
fn locs_diff_total(diff: &crate::LocsDiff) -> (u64, u64) {
    (locs_total(&diff.added), locs_total(&diff.removed))
}

/// Compute total for a LocStatsDiff struct.
fn loc_stats_diff_total(stats: &crate::LocStatsDiff, contexts: &Contexts) -> (u64, u64) {
    let mut added = 0;
    let mut removed = 0;

    if contexts.code {
        let (a, r) = locs_diff_total(&stats.code);
        added += a;
        removed += r;
    }
    if contexts.tests {
        let (a, r) = locs_diff_total(&stats.tests);
        added += a;
        removed += r;
    }
    if contexts.examples {
        let (a, r) = locs_diff_total(&stats.examples);
        added += a;
        removed += r;
    }

    (added, removed)
}

/// Format a diff value as "+added/-removed/net".
fn format_diff(added: u64, removed: u64) -> String {
    let net = added as i64 - removed as i64;
    format!("+{}/-{}/{}", added, removed, net)
}

/// Build values for a diff row from LocStatsDiff.
fn diff_values(stats: &crate::LocStatsDiff, contexts: &Contexts) -> Vec<String> {
    let mut values = Vec::new();

    if contexts.code {
        let (a, r) = locs_diff_total(&stats.code);
        values.push(format_diff(a, r));
    }
    if contexts.tests {
        let (a, r) = locs_diff_total(&stats.tests);
        values.push(format_diff(a, r));
    }
    if contexts.examples {
        let (a, r) = locs_diff_total(&stats.examples);
        values.push(format_diff(a, r));
    }

    let (total_added, total_removed) = loc_stats_diff_total(stats, contexts);
    values.push(format_diff(total_added, total_removed));

    values
}

/// Check if a diff has any net change.
fn has_net_change(stats: &crate::LocStatsDiff, contexts: &Contexts) -> bool {
    let (added, removed) = loc_stats_diff_total(stats, contexts);
    added != removed
}

/// Build data rows from DiffResult based on aggregation level.
fn build_diff_rows(
    result: &DiffResult,
    aggregation: &Aggregation,
    contexts: &Contexts,
) -> Vec<TableRow> {
    match aggregation {
        Aggregation::Total => vec![],
        Aggregation::ByCrate => result
            .crates
            .iter()
            .filter(|c| has_net_change(&c.diff, contexts))
            .map(|c| TableRow {
                label: c.name.clone(),
                values: diff_values(&c.diff, contexts),
            })
            .collect(),
        Aggregation::ByModule => vec![], // Diff doesn't support by-module currently
        Aggregation::ByFile => result
            .files
            .iter()
            .filter(|f| has_net_change(&f.diff, contexts))
            .map(|f| TableRow {
                label: f.path.to_string_lossy().to_string(),
                values: diff_values(&f.diff, contexts),
            })
            .collect(),
    }
}

/// Build footer row from DiffResult.
fn build_diff_footer(result: &DiffResult, contexts: &Contexts) -> TableRow {
    TableRow {
        label: format!("Total ({} files)", result.total.file_count),
        values: diff_values(&result.total, contexts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CrateStats, LocStats, Locs};
    use std::path::PathBuf;

    fn sample_locs(logic: u64) -> Locs {
        Locs {
            blank: 0,
            comments: 0,
            docs: 0,
            logic,
        }
    }

    fn sample_stats(code: u64, tests: u64) -> LocStats {
        LocStats {
            file_count: 1,
            code: sample_locs(code),
            tests: sample_locs(tests),
            examples: sample_locs(0),
        }
    }

    #[test]
    fn test_headers_by_crate() {
        let headers = build_headers(&Aggregation::ByCrate, &Contexts::all());
        assert_eq!(headers[0], "Crate");
        assert_eq!(headers[1], "Code");
        assert_eq!(headers[2], "Tests");
        assert_eq!(headers[3], "Examples");
        assert_eq!(headers[4], "Total");
    }

    #[test]
    fn test_headers_filtered_contexts() {
        let contexts = Contexts::none().with_code(true);
        let headers = build_headers(&Aggregation::ByFile, &contexts);
        assert_eq!(headers.len(), 3); // File, Code, Total
        assert_eq!(headers[0], "File");
        assert_eq!(headers[1], "Code");
        assert_eq!(headers[2], "Total");
    }

    #[test]
    fn test_count_values() {
        let stats = sample_stats(100, 50);
        let values = count_values(&stats, &Contexts::all());
        assert_eq!(values[0], "100"); // Code
        assert_eq!(values[1], "50"); // Tests
        assert_eq!(values[2], "0"); // Examples
        assert_eq!(values[3], "150"); // Total
    }

    #[test]
    fn test_loc_table_from_count() {
        let result = CountResult {
            total: sample_stats(200, 100),
            crates: vec![
                CrateStats {
                    name: "foo".to_string(),
                    path: PathBuf::from("/foo"),
                    stats: sample_stats(150, 75),
                    files: vec![],
                },
                CrateStats {
                    name: "bar".to_string(),
                    path: PathBuf::from("/bar"),
                    stats: sample_stats(50, 25),
                    files: vec![],
                },
            ],
            files: vec![],
            modules: vec![],
        };

        let table = LOCTable::from_count(&result, Aggregation::ByCrate, Contexts::all());

        assert!(table.title.is_none());
        assert_eq!(table.headers[0], "Crate");
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0].label, "foo");
        assert_eq!(table.rows[0].values[0], "150");
        assert_eq!(table.footer.label, "Total (1 files)");
    }

    #[test]
    fn test_format_diff() {
        assert_eq!(format_diff(10, 5), "+10/-5/5");
        assert_eq!(format_diff(5, 10), "+5/-10/-5");
        assert_eq!(format_diff(0, 0), "+0/-0/0");
    }
}
