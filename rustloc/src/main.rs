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
//! - **Multiple output formats**: Table (default), JSON, CSV
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

mod render;

use std::path::Path;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use rustloclib::{
    count_workspace, diff_commits, diff_workdir, Aggregation, Contexts, CountOptions, CountResult,
    DiffOptions, DiffResult, FilterConfig, LocStats, LocStatsDiff, StatsRow, WorkdirDiffMode,
};

/// Rust-aware lines of code counter with test/code separation
#[derive(Parser, Debug)]
#[command(name = "rustloc")]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[command(flatten)]
    count_args: CountArgs,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Count lines of code (default command)
    Count(CountArgs),

    /// Show LOC differences between two git commits
    Diff(DiffArgs),
}

/// Arguments for the count command
#[derive(Args, Debug, Clone)]
struct CountArgs {
    /// Path to analyze (defaults to current directory)
    #[arg(default_value = ".")]
    path: String,

    #[command(flatten)]
    common: CommonArgs,

    /// Show breakdown by crate
    #[arg(long)]
    by_crate: bool,

    /// Show breakdown by file
    #[arg(short = 'f', long)]
    by_file: bool,

    /// Show breakdown by module (directory aggregate + sibling module file)
    #[arg(short = 'm', long)]
    by_module: bool,
}

/// Arguments for the diff command
#[derive(Args, Debug)]
struct DiffArgs {
    /// Commit range (e.g., HEAD~5..HEAD) or base commit.
    /// If omitted, shows working directory changes vs HEAD.
    from: Option<String>,

    /// Target commit (optional if using range syntax like HEAD~5..HEAD)
    to: Option<String>,

    /// Path to repository (defaults to current directory)
    #[arg(short, long, default_value = ".")]
    path: String,

    /// Show only staged changes (like git diff --cached).
    /// Only valid when comparing working directory (no commit args).
    #[arg(long, visible_alias = "cached")]
    staged: bool,

    #[command(flatten)]
    common: CommonArgs,

    /// Show breakdown by crate
    #[arg(long)]
    by_crate: bool,

    /// Show breakdown by file
    #[arg(short = 'f', long)]
    by_file: bool,
}

/// Common arguments shared between count and diff commands
#[derive(Args, Debug, Clone)]
struct CommonArgs {
    /// Filter by crate name (can be specified multiple times)
    #[arg(short, long = "crate")]
    crates: Vec<String>,

    /// Include files matching glob pattern (can be specified multiple times)
    #[arg(short, long)]
    include: Vec<String>,

    /// Exclude files matching glob pattern (can be specified multiple times)
    #[arg(short, long)]
    exclude: Vec<String>,

    /// Output format
    #[arg(short = 'o', long, value_enum, default_value = "table")]
    output: OutputFormat,

    /// Filter which code contexts to show (comma-separated: main,tests,examples)
    /// If not specified, all contexts are shown.
    #[arg(short = 't', long = "type", value_delimiter = ',')]
    types: Vec<ContextType>,
}

/// Code context types that can be filtered
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ContextType {
    /// Production code
    Code,
    /// Test code
    Tests,
    /// Example code
    Examples,
}

/// Convert CLI ContextType args to library's Contexts
fn to_contexts(types: &[ContextType]) -> Contexts {
    if types.is_empty() {
        Contexts::all()
    } else {
        Contexts::none()
            .with_code(types.contains(&ContextType::Code))
            .with_tests(types.contains(&ContextType::Tests))
            .with_examples(types.contains(&ContextType::Examples))
    }
}

/// Convert a path to a relative path from the base directory.
/// Falls back to the original path if it can't be made relative.
fn make_relative(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Table,
    Json,
    Csv,
    /// Debug output showing style tags as literals
    TermDebug,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Some(Commands::Count(args)) => run_count(args),
        Some(Commands::Diff(args)) => run_diff(args),
        None => run_count(cli.count_args),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn build_filter(common: &CommonArgs) -> Result<FilterConfig, Box<dyn std::error::Error>> {
    let mut filter = FilterConfig::new();
    for pattern in &common.include {
        filter = filter.include(pattern)?;
    }
    for pattern in &common.exclude {
        filter = filter.exclude(pattern)?;
    }
    Ok(filter)
}

/// Core handler logic for count command - returns data without rendering
fn handle_count(args: &CountArgs) -> Result<CountResult, Box<dyn std::error::Error>> {
    let filter = build_filter(&args.common)?;
    let contexts = to_contexts(&args.common.types);

    // Determine aggregation level from flags
    let aggregation = if args.by_file {
        Aggregation::ByFile
    } else if args.by_module {
        Aggregation::ByModule
    } else if args.by_crate {
        Aggregation::ByCrate
    } else {
        Aggregation::Total
    };

    let options = CountOptions::new()
        .crates(args.common.crates.clone())
        .filter(filter)
        .aggregation(aggregation)
        .contexts(contexts);

    Ok(count_workspace(&args.path, options)?)
}

fn run_count(args: CountArgs) -> Result<(), Box<dyn std::error::Error>> {
    let contexts = to_contexts(&args.common.types);
    let base_path = std::fs::canonicalize(&args.path)?;

    let result = handle_count(&args)?;

    match args.common.output {
        OutputFormat::Table => print_count_table(
            &result,
            args.by_crate,
            args.by_file,
            args.by_module,
            &contexts,
            &base_path,
            render::OutputMode::Term,
        ),
        OutputFormat::TermDebug => print_count_table(
            &result,
            args.by_crate,
            args.by_file,
            args.by_module,
            &contexts,
            &base_path,
            render::OutputMode::TermDebug,
        ),
        OutputFormat::Json => print_count_json(&result)?,
        OutputFormat::Csv => print_count_csv(
            &result,
            args.by_crate,
            args.by_file,
            args.by_module,
            &contexts,
            &base_path,
        ),
    }

    Ok(())
}

/// Core handler logic for diff command - returns data without rendering
fn handle_diff(args: &DiffArgs) -> Result<DiffResult, Box<dyn std::error::Error>> {
    let filter = build_filter(&args.common)?;
    let contexts = to_contexts(&args.common.types);

    // Determine aggregation level from flags
    let aggregation = if args.by_file {
        Aggregation::ByFile
    } else if args.by_crate {
        Aggregation::ByCrate
    } else {
        Aggregation::Total
    };

    let options = DiffOptions::new()
        .crates(args.common.crates.clone())
        .filter(filter)
        .aggregation(aggregation)
        .contexts(contexts);

    // Determine if this is a working directory diff or commit diff
    if args.from.is_none() {
        // No commit args - diff working directory
        let mode = if args.staged {
            WorkdirDiffMode::Staged
        } else {
            WorkdirDiffMode::All
        };
        Ok(diff_workdir(&args.path, mode, options)?)
    } else {
        // Commit args provided - diff between commits
        if args.staged {
            return Err("--staged/--cached can only be used without commit arguments".into());
        }
        let (from, to) = parse_commit_range(args.from.as_deref().unwrap(), args.to.as_deref())?;
        Ok(diff_commits(&args.path, &from, &to, options)?)
    }
}

fn run_diff(args: DiffArgs) -> Result<(), Box<dyn std::error::Error>> {
    let contexts = to_contexts(&args.common.types);
    let base_path = std::fs::canonicalize(&args.path)?;

    let result = handle_diff(&args)?;

    match args.common.output {
        OutputFormat::Table => print_diff_table(
            &result,
            args.by_crate,
            args.by_file,
            &contexts,
            &base_path,
            render::OutputMode::Term,
        ),
        OutputFormat::TermDebug => print_diff_table(
            &result,
            args.by_crate,
            args.by_file,
            &contexts,
            &base_path,
            render::OutputMode::TermDebug,
        ),
        OutputFormat::Json => print_diff_json(&result)?,
        OutputFormat::Csv => {
            print_diff_csv(&result, args.by_crate, args.by_file, &contexts, &base_path)
        }
    }

    Ok(())
}

fn parse_commit_range(
    from: &str,
    to: Option<&str>,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    if let Some(to_commit) = to {
        // Two separate arguments: from to
        Ok((from.to_string(), to_commit.to_string()))
    } else if from.contains("..") {
        // Range syntax: from..to
        let parts: Vec<&str> = from.split("..").collect();
        if parts.len() != 2 {
            return Err("Invalid commit range format. Use 'from..to' or 'from to'".into());
        }
        Ok((parts[0].to_string(), parts[1].to_string()))
    } else {
        // Single argument without range - assume comparing to HEAD
        Ok((from.to_string(), "HEAD".to_string()))
    }
}

// ============================================================================
// Unified output helpers
// ============================================================================

/// Print a stats table with rows of StatsRow using template rendering
fn print_stats_table(
    rows: &[StatsRow],
    total: &StatsRow,
    name_header: &str,
    name_width: usize,
    ctx: &Contexts,
    output_mode: render::OutputMode,
) {
    match render::render_stats_table(rows, total, name_header, name_width, ctx, output_mode) {
        Ok(output) => print!("{}", output),
        Err(e) => eprintln!("Template error: {}", e),
    }
}

// ============================================================================
// Count output functions
// ============================================================================

fn print_count_table(
    result: &CountResult,
    by_crate: bool,
    by_file: bool,
    by_module: bool,
    ctx: &Contexts,
    base_path: &Path,
    output_mode: render::OutputMode,
) {
    // Determine column header based on aggregation level
    let (name_header, name_width) = if by_file {
        ("File", 60)
    } else if by_module {
        ("Module", 40)
    } else if by_crate {
        ("Crate", 40)
    } else {
        ("", 40) // Total-only view still has name column
    };

    // Build rows from result based on aggregation level
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

    // Build total row
    let total = StatsRow::from_count(
        format!("Total ({} files)", result.total.file_count),
        &result.total,
    );

    print_stats_table(&rows, &total, name_header, name_width, ctx, output_mode);
}

fn print_count_json(result: &CountResult) -> Result<(), serde_json::Error> {
    println!("{}", serde_json::to_string_pretty(result)?);
    Ok(())
}

fn print_count_csv(
    result: &CountResult,
    by_crate: bool,
    by_file: bool,
    by_module: bool,
    ctx: &Contexts,
    base_path: &Path,
) {
    // Build dynamic header based on which contexts are enabled
    let mut header = String::from("name");
    if ctx.code {
        header.push_str(",code");
    }
    if ctx.tests {
        header.push_str(",tests");
    }
    if ctx.examples {
        header.push_str(",examples");
    }
    header.push_str(",total,files");
    println!("{}", header);

    let format_stats = |name: &str, stats: &LocStats| {
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
            println!("{}", format_stats(&path_str, &file.stats));
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
            println!("{}", format_stats(name, &module.stats));
        }
    } else if by_crate {
        for crate_stats in &result.crates {
            if crate_stats.stats.total() == 0 {
                continue;
            }
            println!("{}", format_stats(&crate_stats.name, &crate_stats.stats));
        }
    }

    // Always print total
    println!("{}", format_stats("total", &result.total));
}

// ============================================================================
// Diff output functions
// ============================================================================

fn print_diff_table(
    result: &DiffResult,
    by_crate: bool,
    by_file: bool,
    ctx: &Contexts,
    base_path: &Path,
    output_mode: render::OutputMode,
) {
    println!("Diff: {} → {}", result.from_commit, result.to_commit);
    println!();

    // Determine column header based on aggregation level (same as counts)
    let (name_header, name_width) = if by_file {
        ("File", 60)
    } else if by_crate {
        ("Crate", 40)
    } else {
        ("", 40) // Total-only view still has name column
    };

    // Build rows from result based on aggregation level
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

    // Build total row
    let total = result
        .total
        .to_stats_row(format!("Total ({} files)", result.total.file_count));

    print_stats_table(&rows, &total, name_header, name_width, ctx, output_mode);
}

fn print_diff_json(result: &DiffResult) -> Result<(), serde_json::Error> {
    println!("{}", serde_json::to_string_pretty(result)?);
    Ok(())
}

fn print_diff_csv(
    result: &DiffResult,
    by_crate: bool,
    by_file: bool,
    ctx: &Contexts,
    base_path: &Path,
) {
    // Build dynamic header matching count CSV format but with diff columns
    let mut header = String::from("name");
    if ctx.code {
        header.push_str(",code_added,code_removed,code_net");
    }
    if ctx.tests {
        header.push_str(",tests_added,tests_removed,tests_net");
    }
    if ctx.examples {
        header.push_str(",examples_added,examples_removed,examples_net");
    }
    header.push_str(",total_added,total_removed,total_net,files");
    println!("{}", header);

    // Helper to format a row
    let format_stats = |name: &str, diff: &LocStatsDiff| {
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
            println!("{}", format_stats(&path_str, &file.diff));
        }
    } else if by_crate {
        for crate_stats in &result.crates {
            println!("{}", format_stats(&crate_stats.name, &crate_stats.diff));
        }
    }

    // Always print total
    println!("{}", format_stats("total", &result.total));
}
