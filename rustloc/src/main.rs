//! # rustloc
//!
//! A CLI tool for counting lines of code in Rust projects with test/code separation.
//!
//! ## Overview
//!
//! rustloc is built on top of rustloclib and provides a command-line interface for
//! analyzing Rust codebases. It understands Rust's test structure and can separate
//! production code from test code, even when they're in the same file.
//!
//! ## Features
//!
//! - **Rust-aware**: Distinguishes code, tests, examples, comments, and blank lines
//! - **Cargo workspace support**: Filter by crate with `--crate` or `-c`
//! - **Glob filtering**: Include/exclude files with glob patterns
//! - **Multiple output formats**: Table (default), JSON
//! - **Git diff analysis**: Compare LOC between commits
//!
//! ## Usage
//!
//! ```bash
//! # Count LOC in current directory
//! rustloc .
//!
//! # Count specific crates in a workspace
//! rustloc . --crate my-lib --crate my-cli
//!
//! # Output as JSON
//! rustloc . --output json
//!
//! # Filter files with glob patterns
//! rustloc . --include "src/**/*.rs" --exclude "**/generated/**"
//!
//! # Diff between commits
//! rustloc diff HEAD~5..HEAD
//! rustloc diff main feature-branch
//!
//! # Diff working directory changes (like git diff)
//! rustloc diff
//!
//! # Diff only staged changes (like git diff --cached)
//! rustloc diff --staged
//! ```
//!
//! ## Origins
//!
//! The parsing logic is adapted from [cargo-warloc](https://github.com/Maximkaaa/cargo-warloc)
//! by Maxim Gritsenko. This CLI wraps rustloclib to provide a user-friendly interface.

use std::process::ExitCode;

use clap::{Arg, ArgAction, ArgMatches, Command};
use console::Style;
use outstanding::cli::{App, CommandContext, HandlerResult, Output, RunResult};
use outstanding::Theme;
use rustloclib::{
    count_workspace, diff_commits, diff_workdir, Aggregation, Contexts, CountOptions, DiffOptions,
    FilterConfig, StatsRow, WorkdirDiffMode,
};
use serde::Serialize;

/// Include template at compile time
const STATS_TABLE_TEMPLATE: &str = include_str!("../templates/stats_table.jinja");

/// Column data for template rendering
#[derive(Debug, Serialize)]
struct TemplateColumn {
    name: String,
    formatted: String,
}

/// Row data for template rendering
#[derive(Debug, Serialize)]
struct TemplateRow {
    name: String,
    cells: Vec<String>,
    total_net: i64,
    is_count: bool,
}

/// Data context for stats table template
#[derive(Debug, Serialize)]
struct StatsTableContext {
    name_header: String,
    name_header_formatted: String,
    columns: Vec<TemplateColumn>,
    separator: String,
    rows: Vec<TemplateRow>,
    total: TemplateRow,
}

/// Truncate a name to fit within max_len
fn truncate_name(name: &str, max_len: usize) -> String {
    if name.len() > max_len {
        format!("..{}", &name[name.len() - max_len + 2..])
    } else {
        name.to_string()
    }
}

/// Convert a StatsRow to a TemplateRow
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

/// Convert a path to a relative path from the base directory
fn make_relative(path: &std::path::Path, base: &std::path::Path) -> String {
    path.strip_prefix(base)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

/// Build the clap Command structure
fn build_command() -> Command {
    Command::new("rustloc")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Arthur Debert")
        .about("Rust-aware lines of code counter with test/code separation")
        .arg(
            Arg::new("path")
                .help("Path to analyze (defaults to current directory)")
                .default_value("."),
        )
        .arg(
            Arg::new("crate")
                .short('c')
                .long("crate")
                .action(ArgAction::Append)
                .help("Filter by crate name (can be specified multiple times)"),
        )
        .arg(
            Arg::new("include")
                .short('i')
                .long("include")
                .action(ArgAction::Append)
                .help("Include files matching glob pattern"),
        )
        .arg(
            Arg::new("exclude")
                .short('e')
                .long("exclude")
                .action(ArgAction::Append)
                .help("Exclude files matching glob pattern"),
        )
        .arg(
            Arg::new("type")
                .short('t')
                .long("type")
                .value_delimiter(',')
                .value_parser(["code", "tests", "examples"])
                .help("Filter code contexts (comma-separated: code,tests,examples)"),
        )
        .arg(
            Arg::new("by-crate")
                .long("by-crate")
                .action(ArgAction::SetTrue)
                .help("Show breakdown by crate"),
        )
        .arg(
            Arg::new("by-file")
                .short('f')
                .long("by-file")
                .action(ArgAction::SetTrue)
                .help("Show breakdown by file"),
        )
        .arg(
            Arg::new("by-module")
                .short('m')
                .long("by-module")
                .action(ArgAction::SetTrue)
                .help("Show breakdown by module"),
        )
        .subcommand(
            Command::new("count")
                .about("Count lines of code (default command)")
                .arg(Arg::new("path").help("Path to analyze").default_value("."))
                .arg(
                    Arg::new("crate")
                        .short('c')
                        .long("crate")
                        .action(ArgAction::Append)
                        .help("Filter by crate name"),
                )
                .arg(
                    Arg::new("include")
                        .short('i')
                        .long("include")
                        .action(ArgAction::Append)
                        .help("Include files matching glob pattern"),
                )
                .arg(
                    Arg::new("exclude")
                        .short('e')
                        .long("exclude")
                        .action(ArgAction::Append)
                        .help("Exclude files matching glob pattern"),
                )
                .arg(
                    Arg::new("type")
                        .short('t')
                        .long("type")
                        .value_delimiter(',')
                        .value_parser(["code", "tests", "examples"])
                        .help("Filter code contexts"),
                )
                .arg(
                    Arg::new("by-crate")
                        .long("by-crate")
                        .action(ArgAction::SetTrue)
                        .help("Show breakdown by crate"),
                )
                .arg(
                    Arg::new("by-file")
                        .short('f')
                        .long("by-file")
                        .action(ArgAction::SetTrue)
                        .help("Show breakdown by file"),
                )
                .arg(
                    Arg::new("by-module")
                        .short('m')
                        .long("by-module")
                        .action(ArgAction::SetTrue)
                        .help("Show breakdown by module"),
                ),
        )
        .subcommand(
            Command::new("diff")
                .about("Show LOC differences between git commits")
                .arg(Arg::new("from").help("Commit range (e.g., HEAD~5..HEAD) or base commit"))
                .arg(Arg::new("to").help("Target commit (optional if using range syntax)"))
                .arg(
                    Arg::new("path")
                        .short('p')
                        .long("path")
                        .default_value(".")
                        .help("Path to repository"),
                )
                .arg(
                    Arg::new("staged")
                        .long("staged")
                        .visible_alias("cached")
                        .action(ArgAction::SetTrue)
                        .help("Show only staged changes"),
                )
                .arg(
                    Arg::new("crate")
                        .short('c')
                        .long("crate")
                        .action(ArgAction::Append)
                        .help("Filter by crate name"),
                )
                .arg(
                    Arg::new("include")
                        .short('i')
                        .long("include")
                        .action(ArgAction::Append)
                        .help("Include files matching glob pattern"),
                )
                .arg(
                    Arg::new("exclude")
                        .short('e')
                        .long("exclude")
                        .action(ArgAction::Append)
                        .help("Exclude files matching glob pattern"),
                )
                .arg(
                    Arg::new("type")
                        .short('t')
                        .long("type")
                        .value_delimiter(',')
                        .value_parser(["code", "tests", "examples"])
                        .help("Filter code contexts"),
                )
                .arg(
                    Arg::new("by-crate")
                        .long("by-crate")
                        .action(ArgAction::SetTrue)
                        .help("Show breakdown by crate"),
                )
                .arg(
                    Arg::new("by-file")
                        .short('f')
                        .long("by-file")
                        .action(ArgAction::SetTrue)
                        .help("Show breakdown by file"),
                ),
        )
}

/// Extract contexts from matches
fn extract_contexts(matches: &ArgMatches) -> Contexts {
    let types: Vec<&str> = matches
        .get_many::<String>("type")
        .map(|v| v.map(|s| s.as_str()).collect())
        .unwrap_or_default();

    if types.is_empty() {
        Contexts::all()
    } else {
        Contexts::none()
            .with_code(types.contains(&"code"))
            .with_tests(types.contains(&"tests"))
            .with_examples(types.contains(&"examples"))
    }
}

/// Build filter config from matches
fn build_filter(matches: &ArgMatches) -> Result<FilterConfig, anyhow::Error> {
    let mut filter = FilterConfig::new();

    if let Some(includes) = matches.get_many::<String>("include") {
        for pattern in includes {
            filter = filter.include(pattern)?;
        }
    }

    if let Some(excludes) = matches.get_many::<String>("exclude") {
        for pattern in excludes {
            filter = filter.exclude(pattern)?;
        }
    }

    Ok(filter)
}

/// Extract crates list from matches
fn extract_crates(matches: &ArgMatches) -> Vec<String> {
    matches
        .get_many::<String>("crate")
        .map(|v| v.cloned().collect())
        .unwrap_or_default()
}

/// Handler for count command
fn count_handler(matches: &ArgMatches, ctx: &CommandContext) -> HandlerResult<serde_json::Value> {
    let path = matches
        .get_one::<String>("path")
        .map(|s| s.as_str())
        .unwrap_or(".");
    let filter = build_filter(matches)?;
    let contexts = extract_contexts(matches);
    let crates = extract_crates(matches);

    let by_file = matches.get_flag("by-file");
    let by_module = matches.get_flag("by-module");
    let by_crate = matches.get_flag("by-crate");

    let aggregation = if by_file {
        Aggregation::ByFile
    } else if by_module {
        Aggregation::ByModule
    } else if by_crate {
        Aggregation::ByCrate
    } else {
        Aggregation::Total
    };

    let options = CountOptions::new()
        .crates(crates)
        .filter(filter)
        .aggregation(aggregation)
        .contexts(contexts);

    let result = count_workspace(path, options)?;

    // For JSON mode, return raw data
    if ctx.output_mode.is_structured() {
        return Ok(Output::Render(serde_json::to_value(&result)?));
    }

    // For table mode, build template context
    let base_path = std::fs::canonicalize(path).unwrap_or_else(|_| std::path::PathBuf::from(path));

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
                let path_str = make_relative(&f.path, &base_path);
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

    let context = build_stats_context(&rows, &total, name_header, name_width, &contexts);
    Ok(Output::Render(serde_json::to_value(&context)?))
}

/// Build StatsTableContext from rows and configuration
fn build_stats_context(
    rows: &[StatsRow],
    total: &StatsRow,
    name_header: &str,
    name_width: usize,
    ctx: &Contexts,
) -> StatsTableContext {
    // Build columns based on enabled contexts
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

    let columns: Vec<TemplateColumn> = column_names
        .iter()
        .map(|name| TemplateColumn {
            name: name.to_string(),
            formatted: format!("{:>width$}", name, width = cell_width),
        })
        .collect();

    let separator = "-".repeat(name_width + (cell_width + 1) * columns.len());

    let template_rows: Vec<TemplateRow> = rows
        .iter()
        .map(|r| to_template_row(r, ctx, name_width, cell_width))
        .collect();

    let total_row = to_template_row(total, ctx, name_width, cell_width);

    StatsTableContext {
        name_header: name_header.to_string(),
        name_header_formatted: format!("{:<width$}", name_header, width = name_width),
        columns,
        separator,
        rows: template_rows,
        total: total_row,
    }
}

/// Handler for diff command
fn diff_handler(matches: &ArgMatches, ctx: &CommandContext) -> HandlerResult<serde_json::Value> {
    let path = matches
        .get_one::<String>("path")
        .map(|s| s.as_str())
        .unwrap_or(".");
    let filter = build_filter(matches)?;
    let contexts = extract_contexts(matches);
    let crates = extract_crates(matches);
    let staged = matches.get_flag("staged");

    let by_file = matches.get_flag("by-file");
    let by_crate = matches.get_flag("by-crate");

    let aggregation = if by_file {
        Aggregation::ByFile
    } else if by_crate {
        Aggregation::ByCrate
    } else {
        Aggregation::Total
    };

    let options = DiffOptions::new()
        .crates(crates)
        .filter(filter)
        .aggregation(aggregation)
        .contexts(contexts);

    let from = matches.get_one::<String>("from");
    let to = matches.get_one::<String>("to");

    let result = if from.is_none() {
        // Working directory diff
        let mode = if staged {
            WorkdirDiffMode::Staged
        } else {
            WorkdirDiffMode::All
        };
        diff_workdir(path, mode, options)?
    } else {
        // Commit diff
        if staged {
            return Err(anyhow::anyhow!(
                "--staged/--cached can only be used without commit arguments"
            ));
        }
        let (from_commit, to_commit) = parse_commit_range(from.unwrap(), to.map(|s| s.as_str()))?;
        diff_commits(path, &from_commit, &to_commit, options)?
    };

    // For JSON mode, return raw data
    if ctx.output_mode.is_structured() {
        return Ok(Output::Render(serde_json::to_value(&result)?));
    }

    // For table mode, build header and template context
    let base_path = std::fs::canonicalize(path).unwrap_or_else(|_| std::path::PathBuf::from(path));

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
                let path_str = make_relative(&f.path, &base_path);
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

    let context = build_stats_context(&rows, &total, name_header, name_width, &contexts);

    // Add diff header info to context
    let mut value = serde_json::to_value(&context)?;
    value["diff_header"] = serde_json::json!(format!(
        "Diff: {} \u{2192} {}",
        result.from_commit, result.to_commit
    ));

    Ok(Output::Render(value))
}

fn parse_commit_range(from: &str, to: Option<&str>) -> Result<(String, String), anyhow::Error> {
    if let Some(to_commit) = to {
        Ok((from.to_string(), to_commit.to_string()))
    } else if from.contains("..") {
        let parts: Vec<&str> = from.split("..").collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Invalid commit range format. Use 'from..to' or 'from to'"
            ));
        }
        Ok((parts[0].to_string(), parts[1].to_string()))
    } else {
        Ok((from.to_string(), "HEAD".to_string()))
    }
}

/// Create the theme with styles
fn create_theme() -> Theme {
    Theme::new().add("category", Style::new().bold())
}

fn main() -> ExitCode {
    let cmd = build_command();
    let theme = create_theme();

    // Build the outstanding app with command handlers and run
    let result = App::builder()
        .theme(theme)
        .command("count", count_handler, STATS_TABLE_TEMPLATE)
        .command("diff", diff_handler, STATS_TABLE_TEMPLATE)
        .run_to_string(cmd, std::env::args());

    match result {
        RunResult::Handled(output) => {
            if !output.is_empty() {
                // Check if it's an error message from handler
                if output.starts_with("Error:") {
                    eprintln!("{}", output);
                    return ExitCode::FAILURE;
                }
                print!("{}", output);
            }
            ExitCode::SUCCESS
        }
        RunResult::Binary(_, _) => {
            // Not used in rustloc
            ExitCode::SUCCESS
        }
        RunResult::NoMatch(matches) => {
            // Handle root command (no subcommand) - treat as count
            // Extract output mode from args (outstanding adds _output_mode)
            let output_mode = matches
                .get_one::<String>("_output_mode")
                .map(|s| match s.as_str() {
                    "json" => outstanding::OutputMode::Json,
                    "text" => outstanding::OutputMode::Text,
                    "term-debug" => outstanding::OutputMode::TermDebug,
                    "term" => outstanding::OutputMode::Term,
                    _ => outstanding::OutputMode::Auto,
                })
                .unwrap_or(outstanding::OutputMode::Auto);

            let ctx = CommandContext {
                output_mode,
                command_path: vec![],
            };

            match count_handler(&matches, &ctx) {
                Ok(Output::Render(value)) => {
                    if output_mode.is_structured() {
                        // JSON mode - print raw JSON
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&value).unwrap_or_default()
                        );
                    } else {
                        // Table mode - render using outstanding
                        let theme = create_theme();
                        match outstanding::render(STATS_TABLE_TEMPLATE, &value, &theme) {
                            Ok(output) => {
                                print!("{}", output);
                            }
                            Err(e) => {
                                eprintln!("Error: {e}");
                                return ExitCode::FAILURE;
                            }
                        }
                    }
                    ExitCode::SUCCESS
                }
                Ok(Output::Silent) => ExitCode::SUCCESS,
                Ok(Output::Binary { .. }) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("Error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
