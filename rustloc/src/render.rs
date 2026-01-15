//! Template rendering for CLI output using outstanding

use console::Style;
use outstanding::{render_auto, render_with_output, Theme};
use rustloclib::{Contexts, CountResult, DiffResult, LocStats, LocStatsDiff, StatsRow};
use serde::Serialize;
use std::path::Path;

/// Include template at compile time
const STATS_TABLE_TEMPLATE: &str = include_str!("../templates/stats_table.jinja");

/// Re-export OutputMode for callers
pub use outstanding::OutputMode;

/// Column data for template rendering
#[derive(Debug, Serialize)]
struct TemplateColumn {
    /// Column name (e.g., "Code", "Tests", "Total")
    name: String,
    /// Pre-formatted with padding
    formatted: String,
}

/// Row data for template rendering (pre-formatted)
#[derive(Debug, Serialize)]
struct TemplateRow {
    /// Pre-padded name (left-aligned, padded to name_width)
    name: String,
    /// Pre-padded cells (right-aligned, padded to cell_width)
    cells: Vec<String>,
    /// Net value for filtering
    total_net: i64,
    /// Whether this is a count row (for skip logic)
    is_count: bool,
}

/// Data context for stats table template
#[derive(Debug, Serialize)]
struct StatsTableContext {
    /// Name column header (e.g., "File", "Crate")
    name_header: String,
    /// Pre-padded name column header
    name_header_formatted: String,
    /// Column definitions
    columns: Vec<TemplateColumn>,
    /// Separator line (dashes)
    separator: String,
    /// Data rows
    rows: Vec<TemplateRow>,
    /// Total row
    total: TemplateRow,
}

/// Truncate a name to fit within max_len, adding ".." prefix if needed
fn truncate_name(name: &str, max_len: usize) -> String {
    if name.len() > max_len {
        format!("..{}", &name[name.len() - max_len + 2..])
    } else {
        name.to_string()
    }
}

/// Convert a StatsRow to a TemplateRow with the given contexts, pre-formatting cells
fn to_template_row(
    row: &StatsRow,
    ctx: &Contexts,
    name_width: usize,
    cell_width: usize,
) -> TemplateRow {
    let mut cells = Vec::new();
    if ctx.code {
        cells.push(format!("{:>width$}", row.code, width = cell_width));
    }
    if ctx.tests {
        cells.push(format!("{:>width$}", row.tests, width = cell_width));
    }
    if ctx.examples {
        cells.push(format!("{:>width$}", row.examples, width = cell_width));
    }
    cells.push(format!("{:>width$}", row.total, width = cell_width));

    let truncated = truncate_name(&row.name, name_width - 2);

    TemplateRow {
        name: format!("{:<width$}", truncated, width = name_width),
        cells,
        total_net: row.total.net(),
        is_count: row.is_count(),
    }
}

/// Create the theme with styles
fn create_theme() -> Theme {
    Theme::new().add("category", Style::new().bold())
}

/// Convert a path to a relative path from the base directory.
fn make_relative(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

/// Render a stats table to string using outstanding
pub fn render_stats_table(
    rows: &[StatsRow],
    total: &StatsRow,
    name_header: &str,
    name_width: usize,
    ctx: &Contexts,
    output_mode: OutputMode,
) -> Result<String, Box<dyn std::error::Error>> {
    let theme = create_theme();

    // Build columns list based on enabled contexts
    let mut column_names = Vec::new();
    if ctx.code {
        column_names.push("Code");
    }
    if ctx.tests {
        column_names.push("Tests");
    }
    if ctx.examples {
        column_names.push("Examples");
    }
    column_names.push("Total");

    // Determine cell width based on whether we're showing diffs or counts
    let is_diff = total.is_diff();
    let cell_width = if is_diff { 16 } else { 10 };

    // Build column objects
    let columns: Vec<TemplateColumn> = column_names
        .iter()
        .map(|name| TemplateColumn {
            name: name.to_string(),
            formatted: format!("{:>width$}", name, width = cell_width),
        })
        .collect();

    // Build separator line
    let separator = "-".repeat(name_width + (cell_width + 1) * columns.len());

    // Convert rows
    let template_rows: Vec<TemplateRow> = rows
        .iter()
        .map(|r| to_template_row(r, ctx, name_width, cell_width))
        .collect();

    let total_row = to_template_row(total, ctx, name_width, cell_width);

    let context = StatsTableContext {
        name_header: name_header.to_string(),
        name_header_formatted: format!("{:<width$}", name_header, width = name_width),
        columns,
        separator,
        rows: template_rows,
        total: total_row,
    };

    // Use outstanding's render_with_output function
    let rendered = render_with_output(STATS_TABLE_TEMPLATE, &context, &theme, output_mode)?;

    Ok(rendered)
}

// ============================================================================
// Count output - unified rendering
// ============================================================================

/// Render count result using outstanding's auto dispatch
pub fn render_count(
    result: &CountResult,
    by_crate: bool,
    by_file: bool,
    by_module: bool,
    ctx: &Contexts,
    base_path: &Path,
    output_mode: OutputMode,
) -> Result<String, Box<dyn std::error::Error>> {
    // For JSON mode, use outstanding's render_auto for serialization
    if matches!(output_mode, OutputMode::Json) {
        let theme = create_theme();
        return Ok(render_auto(
            STATS_TABLE_TEMPLATE,
            result,
            &theme,
            output_mode,
        )?);
    }

    // For table modes, build rows and render the table
    let (name_header, name_width) = if by_file {
        ("File", 60)
    } else if by_module {
        ("Module", 40)
    } else if by_crate {
        ("Crate", 40)
    } else {
        ("", 40)
    };

    let rows: Vec<StatsRow> = if by_file && !result.files.is_empty() {
        result
            .files
            .iter()
            .map(|f| {
                let path_str = make_relative(&f.path, base_path);
                StatsRow::from_count(path_str, &f.stats)
            })
            .collect()
    } else if by_module && !result.modules.is_empty() {
        result
            .modules
            .iter()
            .map(|m| {
                let name = if m.name.is_empty() {
                    "(root)".to_string()
                } else {
                    m.name.clone()
                };
                StatsRow::from_count(name, &m.stats)
            })
            .collect()
    } else if by_crate && !result.crates.is_empty() {
        result
            .crates
            .iter()
            .map(|c| StatsRow::from_count(&c.name, &c.stats))
            .collect()
    } else {
        Vec::new()
    };

    let total = StatsRow::from_count(
        format!("Total ({} files)", result.total.file_count),
        &result.total,
    );

    render_stats_table(&rows, &total, name_header, name_width, ctx, output_mode)
}

/// Render count result as CSV
pub fn render_count_csv(
    result: &CountResult,
    by_crate: bool,
    by_file: bool,
    by_module: bool,
    ctx: &Contexts,
    base_path: &Path,
) -> String {
    let mut output = String::new();

    // Build header
    output.push_str("name");
    if ctx.code {
        output.push_str(",code");
    }
    if ctx.tests {
        output.push_str(",tests");
    }
    if ctx.examples {
        output.push_str(",examples");
    }
    output.push_str(",total,files\n");

    let format_stats = |name: &str, stats: &LocStats| -> String {
        let mut row = format!("\"{}\"", name);
        if ctx.code {
            row.push_str(&format!(",{}", stats.code.total()));
        }
        if ctx.tests {
            row.push_str(&format!(",{}", stats.tests.total()));
        }
        if ctx.examples {
            row.push_str(&format!(",{}", stats.examples.total()));
        }
        row.push_str(&format!(",{},{}", stats.total(), stats.file_count));
        row
    };

    if by_file {
        for file in &result.files {
            if file.stats.total() == 0 {
                continue;
            }
            let path_str = make_relative(&file.path, base_path);
            output.push_str(&format_stats(&path_str, &file.stats));
            output.push('\n');
        }
    } else if by_module {
        for module in &result.modules {
            if module.stats.total() == 0 {
                continue;
            }
            let name = if module.name.is_empty() {
                "(root)"
            } else {
                &module.name
            };
            output.push_str(&format_stats(name, &module.stats));
            output.push('\n');
        }
    } else if by_crate {
        for crate_stats in &result.crates {
            if crate_stats.stats.total() == 0 {
                continue;
            }
            output.push_str(&format_stats(&crate_stats.name, &crate_stats.stats));
            output.push('\n');
        }
    }

    output.push_str(&format_stats("total", &result.total));
    output.push('\n');
    output
}

// ============================================================================
// Diff output - unified rendering
// ============================================================================

/// Render diff result using outstanding's auto dispatch
pub fn render_diff(
    result: &DiffResult,
    by_crate: bool,
    by_file: bool,
    ctx: &Contexts,
    base_path: &Path,
    output_mode: OutputMode,
) -> Result<String, Box<dyn std::error::Error>> {
    // For JSON mode, use outstanding's render_auto for serialization
    if matches!(output_mode, OutputMode::Json) {
        let theme = create_theme();
        return Ok(render_auto(
            STATS_TABLE_TEMPLATE,
            result,
            &theme,
            output_mode,
        )?);
    }

    // Build header for diff
    let mut output = format!("Diff: {} → {}\n\n", result.from_commit, result.to_commit);

    // For table modes, build rows and render the table
    let (name_header, name_width) = if by_file {
        ("File", 60)
    } else if by_crate {
        ("Crate", 40)
    } else {
        ("", 40)
    };

    let rows: Vec<StatsRow> = if by_file && !result.files.is_empty() {
        result
            .files
            .iter()
            .map(|f| {
                let path_str = make_relative(&f.path, base_path);
                f.diff.to_stats_row(path_str)
            })
            .collect()
    } else if by_crate && !result.crates.is_empty() {
        result
            .crates
            .iter()
            .map(|c| c.diff.to_stats_row(&c.name))
            .collect()
    } else {
        Vec::new()
    };

    let total = result
        .total
        .to_stats_row(format!("Total ({} files)", result.total.file_count));

    let table = render_stats_table(&rows, &total, name_header, name_width, ctx, output_mode)?;
    output.push_str(&table);
    Ok(output)
}

/// Render diff result as CSV
pub fn render_diff_csv(
    result: &DiffResult,
    by_crate: bool,
    by_file: bool,
    ctx: &Contexts,
    base_path: &Path,
) -> String {
    let mut output = String::new();

    // Build header
    output.push_str("name");
    if ctx.code {
        output.push_str(",code_added,code_removed,code_net");
    }
    if ctx.tests {
        output.push_str(",tests_added,tests_removed,tests_net");
    }
    if ctx.examples {
        output.push_str(",examples_added,examples_removed,examples_net");
    }
    output.push_str(",total_added,total_removed,total_net,files\n");

    let format_stats = |name: &str, diff: &LocStatsDiff| -> String {
        let mut row = format!("\"{}\"", name);
        if ctx.code {
            let net = diff.code.added.total() as i64 - diff.code.removed.total() as i64;
            row.push_str(&format!(
                ",{},{},{}",
                diff.code.added.total(),
                diff.code.removed.total(),
                net
            ));
        }
        if ctx.tests {
            let net = diff.tests.added.total() as i64 - diff.tests.removed.total() as i64;
            row.push_str(&format!(
                ",{},{},{}",
                diff.tests.added.total(),
                diff.tests.removed.total(),
                net
            ));
        }
        if ctx.examples {
            let net = diff.examples.added.total() as i64 - diff.examples.removed.total() as i64;
            row.push_str(&format!(
                ",{},{},{}",
                diff.examples.added.total(),
                diff.examples.removed.total(),
                net
            ));
        }
        let total_added = diff.total_added().total();
        let total_removed = diff.total_removed().total();
        let total_net = total_added as i64 - total_removed as i64;
        row.push_str(&format!(
            ",{},{},{},{}",
            total_added, total_removed, total_net, diff.file_count
        ));
        row
    };

    if by_file {
        for file in &result.files {
            let path_str = make_relative(&file.path, base_path);
            output.push_str(&format_stats(&path_str, &file.diff));
            output.push('\n');
        }
    } else if by_crate {
        for crate_stats in &result.crates {
            output.push_str(&format_stats(&crate_stats.name, &crate_stats.diff));
            output.push('\n');
        }
    }

    output.push_str(&format_stats("total", &result.total));
    output.push('\n');
    output
}
