//! Template rendering for CLI output

use console::Style;
use outstanding::{render_with_output, Theme};
use rustloclib::{Contexts, StatsRow};
use serde::Serialize;

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
