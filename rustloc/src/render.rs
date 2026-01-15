//! Template rendering for CLI output

use outstanding::{render, Theme};
use rustloclib::{Contexts, StatsRow};
use serde::Serialize;

/// Include template at compile time
const STATS_TABLE_TEMPLATE: &str = include_str!("../templates/stats_table.jinja");

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
    /// Pre-padded header
    header: String,
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

/// Render a stats table to string using outstanding
pub fn render_stats_table(
    rows: &[StatsRow],
    total: &StatsRow,
    name_header: &str,
    name_width: usize,
    ctx: &Contexts,
) -> Result<String, Box<dyn std::error::Error>> {
    // Create empty theme (no styling yet)
    let theme = Theme::new();

    // Build columns list based on enabled contexts
    let mut columns = Vec::new();
    if ctx.code {
        columns.push("Code");
    }
    if ctx.tests {
        columns.push("Tests");
    }
    if ctx.examples {
        columns.push("Examples");
    }
    columns.push("Total");

    // Determine cell width based on whether we're showing diffs or counts
    let is_diff = total.is_diff();
    let cell_width = if is_diff { 16 } else { 10 };

    // Build pre-formatted header line
    let mut header = format!("{:<width$}", name_header, width = name_width);
    for col in &columns {
        header.push_str(&format!(" {:>width$}", col, width = cell_width));
    }

    // Build separator line
    let separator = "-".repeat(name_width + (cell_width + 1) * columns.len());

    // Convert rows
    let template_rows: Vec<TemplateRow> = rows
        .iter()
        .map(|r| to_template_row(r, ctx, name_width, cell_width))
        .collect();

    let total_row = to_template_row(total, ctx, name_width, cell_width);

    let context = StatsTableContext {
        header,
        separator,
        rows: template_rows,
        total: total_row,
    };

    // Use outstanding's render function
    let rendered = render(STATS_TABLE_TEMPLATE, &context, &theme)?;

    Ok(rendered)
}
