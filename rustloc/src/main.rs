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

use std::path::Path;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use rustloclib::{
    count_workspace, diff_commits, diff_workdir, Aggregation, Contexts, CountOptions, CountResult,
    DiffOptions, DiffResult, FilterConfig, LocStats, LocStatsDiff, LocsDiff, WorkdirDiffMode,
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

/// Format a diff cell as "+added/-removed/net"
fn format_diff_cell(added: u64, removed: u64) -> String {
    let net = added as i64 - removed as i64;
    format!("+{}/-{}/{}", added, removed, net)
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

fn run_count(args: CountArgs) -> Result<(), Box<dyn std::error::Error>> {
    let filter = build_filter(&args.common)?;
    let contexts = to_contexts(&args.common.types);
    let base_path = std::fs::canonicalize(&args.path)?;

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

    let result = count_workspace(&args.path, options)?;

    match args.common.output {
        OutputFormat::Table => print_count_table(
            &result,
            args.by_crate,
            args.by_file,
            args.by_module,
            &contexts,
            &base_path,
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

fn run_diff(args: DiffArgs) -> Result<(), Box<dyn std::error::Error>> {
    let filter = build_filter(&args.common)?;
    let contexts = to_contexts(&args.common.types);
    let base_path = std::fs::canonicalize(&args.path)?;

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
    let result = if args.from.is_none() {
        // No commit args - diff working directory
        let mode = if args.staged {
            WorkdirDiffMode::Staged
        } else {
            WorkdirDiffMode::All
        };
        diff_workdir(&args.path, mode, options)?
    } else {
        // Commit args provided - diff between commits
        if args.staged {
            return Err("--staged/--cached can only be used without commit arguments".into());
        }
        let (from, to) = parse_commit_range(args.from.as_deref().unwrap(), args.to.as_deref())?;
        diff_commits(&args.path, &from, &to, options)?
    };

    match args.common.output {
        OutputFormat::Table => {
            print_diff_table(&result, args.by_crate, args.by_file, &contexts, &base_path)
        }
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
// Count output functions
// ============================================================================

fn print_count_table(
    result: &CountResult,
    by_crate: bool,
    by_file: bool,
    by_module: bool,
    ctx: &Contexts,
    base_path: &Path,
) {
    // Determine column header based on aggregation level
    let (name_header, name_width) = if by_file {
        ("File", 60)
    } else if by_module {
        ("Module", 40)
    } else if by_crate {
        ("Crate", 40)
    } else {
        ("", 0) // No name column for total-only view
    };

    // Build dynamic header based on which contexts are enabled
    let mut header_parts = Vec::new();
    if ctx.code {
        header_parts.push("Code");
    }
    if ctx.tests {
        header_parts.push("Tests");
    }
    if ctx.examples {
        header_parts.push("Examples");
    }
    header_parts.push("Total");

    // Print header
    if name_width > 0 {
        print!("{:<width$}", name_header, width = name_width);
        for part in &header_parts {
            print!(" {:>10}", part);
        }
        println!();
        println!("{}", "-".repeat(name_width + 11 * header_parts.len()));
    }

    // Helper to print a stats row
    let print_row = |name: &str, stats: &LocStats| {
        print!("{:<width$}", name, width = name_width);
        if ctx.code {
            print!(" {:>10}", stats.code.total());
        }
        if ctx.tests {
            print!(" {:>10}", stats.tests.total());
        }
        if ctx.examples {
            print!(" {:>10}", stats.examples.total());
        }
        println!(" {:>10}", stats.total());
    };

    // Print rows based on aggregation level (skip rows with 0 total)
    if by_file && !result.files.is_empty() {
        for file in &result.files {
            let s = &file.stats;
            if s.total() == 0 {
                continue;
            }
            let path_str = make_relative(&file.path, base_path);
            let truncated = truncate_name(&path_str, name_width - 2);
            print_row(&truncated, s);
        }
    } else if by_module && !result.modules.is_empty() {
        for module in &result.modules {
            let s = &module.stats;
            if s.total() == 0 {
                continue;
            }
            let name = if module.name.is_empty() {
                "(root)"
            } else {
                &module.name
            };
            let truncated = truncate_name(name, name_width - 2);
            print_row(&truncated, s);
        }
    } else if by_crate && !result.crates.is_empty() {
        for crate_stats in &result.crates {
            let s = &crate_stats.stats;
            if s.total() == 0 {
                continue;
            }
            let truncated = truncate_name(&crate_stats.name, name_width - 2);
            print_row(&truncated, s);
        }
    }

    // Print totals
    let s = &result.total;
    if name_width > 0 {
        println!("{}", "-".repeat(name_width + 11 * header_parts.len()));
        print_row(&format!("Total ({} files)", s.file_count), s);
    } else {
        // Total-only view (no breakdown)
        for part in &header_parts {
            print!("{:>12}", part);
        }
        println!("{:>10}", "Files");
        println!("{}", "-".repeat(12 * header_parts.len() + 10));
        if ctx.code {
            print!("{:>12}", s.code.total());
        }
        if ctx.tests {
            print!("{:>12}", s.tests.total());
        }
        if ctx.examples {
            print!("{:>12}", s.examples.total());
        }
        print!("{:>12}", s.total());
        println!("{:>10}", s.file_count);
    }
}

/// Truncate a name to fit within max_len, adding ".." prefix if needed
fn truncate_name(name: &str, max_len: usize) -> String {
    if name.len() > max_len {
        format!("..{}", &name[name.len() - max_len + 2..])
    } else {
        name.to_string()
    }
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
) {
    println!("Diff: {} → {}", result.from_commit, result.to_commit);
    println!();

    if by_file && !result.files.is_empty() {
        println!("By-file breakdown:");
        println!(
            "{:<50} {:>6} {:>16} {:>16}",
            "File", "Change", "Code", "Total"
        );
        println!("{}", "-".repeat(92));

        for file in &result.files {
            let path_str = make_relative(&file.path, base_path);
            let truncated = if path_str.len() > 48 {
                format!("..{}", &path_str[path_str.len() - 46..])
            } else {
                path_str
            };

            let change_type = match file.change_type {
                rustloclib::FileChangeType::Added => "+",
                rustloclib::FileChangeType::Deleted => "-",
                rustloclib::FileChangeType::Modified => "M",
            };

            let total_diff = sum_diff_contexts(&file.diff);
            println!(
                "{:<50} {:>6} {:>16} {:>16}",
                truncated,
                change_type,
                format_diff_cell(total_diff.added.logic, total_diff.removed.logic),
                format_diff_cell(total_diff.added.total(), total_diff.removed.total())
            );
        }
        println!();
    }

    if by_crate && !result.crates.is_empty() {
        println!("By-crate breakdown:");
        for crate_stats in &result.crates {
            println!(
                "\n{} ({} files changed):",
                crate_stats.name, crate_stats.diff.file_count
            );
            print_diff_stats_table(&crate_stats.diff, ctx);
        }
        println!();
    }

    if by_crate || by_file {
        println!("Total ({} files changed):", result.total.file_count);
    } else {
        println!("Files changed: {}", result.total.file_count);
    }
    print_diff_stats_table(&result.total, ctx);
}

fn print_diff_stats_table(diff: &LocStatsDiff, ctx: &Contexts) {
    println!(
        "{:<12} | {:>16} | {:>16} | {:>16} | {:>16} | {:>16}",
        "Context", "Logic", "Blank", "Docs", "Comments", "Total"
    );
    println!("{}", "-".repeat(104));

    if ctx.code {
        print_diff_locs_row("Code", &diff.code);
    }
    if ctx.tests {
        print_diff_locs_row("Tests", &diff.tests);
    }
    if ctx.examples {
        print_diff_locs_row("Examples", &diff.examples);
    }

    println!("{}", "-".repeat(104));

    let total_added = diff.total_added();
    let total_removed = diff.total_removed();
    println!(
        "{:<12} | {:>16} | {:>16} | {:>16} | {:>16} | {:>16}",
        "",
        format_diff_cell(total_added.logic, total_removed.logic),
        format_diff_cell(total_added.blank, total_removed.blank),
        format_diff_cell(total_added.docs, total_removed.docs),
        format_diff_cell(total_added.comments, total_removed.comments),
        format_diff_cell(total_added.total(), total_removed.total())
    );
}

fn print_diff_locs_row(name: &str, diff: &LocsDiff) {
    println!(
        "{:<12} | {:>16} | {:>16} | {:>16} | {:>16} | {:>16}",
        name,
        format_diff_cell(diff.added.logic, diff.removed.logic),
        format_diff_cell(diff.added.blank, diff.removed.blank),
        format_diff_cell(diff.added.docs, diff.removed.docs),
        format_diff_cell(diff.added.comments, diff.removed.comments),
        format_diff_cell(diff.added.total(), diff.removed.total())
    );
}

fn sum_diff_contexts(diff: &LocStatsDiff) -> LocsDiff {
    LocsDiff {
        added: diff.total_added(),
        removed: diff.total_removed(),
    }
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
    println!(
        "type,name,change,logic_added,logic_removed,logic_net,total_added,total_removed,total_net"
    );

    let format_diff = |type_name: &str, name: &str, change: &str, diff: &LocsDiff| {
        let net_logic = diff.added.logic as i64 - diff.removed.logic as i64;
        let net_total = diff.added.total() as i64 - diff.removed.total() as i64;
        format!(
            "{},\"{}\",{},{},{},{},{},{},{}",
            type_name,
            name,
            change,
            diff.added.logic,
            diff.removed.logic,
            net_logic,
            diff.added.total(),
            diff.removed.total(),
            net_total
        )
    };

    if by_file {
        for file in &result.files {
            let total_diff = sum_diff_contexts(&file.diff);
            let change = match file.change_type {
                rustloclib::FileChangeType::Added => "added",
                rustloclib::FileChangeType::Deleted => "deleted",
                rustloclib::FileChangeType::Modified => "modified",
            };
            let path_str = make_relative(&file.path, base_path);
            println!("{}", format_diff("file", &path_str, change, &total_diff));
        }
    }

    if by_crate {
        for crate_stats in &result.crates {
            let total_diff = LocsDiff {
                added: crate_stats.diff.total_added(),
                removed: crate_stats.diff.total_removed(),
            };
            println!(
                "{}",
                format_diff("crate", &crate_stats.name, "-", &total_diff)
            );
        }
    }

    let d = &result.total;
    if ctx.code {
        println!("{}", format_diff("code", "total", "-", &d.code));
    }
    if ctx.tests {
        println!("{}", format_diff("tests", "total", "-", &d.tests));
    }
    if ctx.examples {
        println!("{}", format_diff("examples", "total", "-", &d.examples));
    }

    let total_diff = LocsDiff {
        added: d.total_added(),
        removed: d.total_removed(),
    };
    println!("{}", format_diff("total", "total", "-", &total_diff));
}
