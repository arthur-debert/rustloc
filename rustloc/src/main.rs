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
//! - **Rust-aware**: Distinguishes code, tests, examples, comments, and blanks
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
//! ```
//!
//! ## Origins
//!
//! The parsing logic is adapted from [cargo-warloc](https://github.com/Maximkaaa/cargo-warloc)
//! by Maxim Gritsenko. This CLI wraps rustloclib to provide a user-friendly interface.

use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use rustloclib::{
    count_workspace, diff_commits, CountOptions, CountResult, DiffOptions, DiffResult,
    FilterConfig, LocStats, LocStatsDiff, Locs, LocsDiff,
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
    /// Commit range (e.g., HEAD~5..HEAD) or base commit
    #[arg(required = true)]
    from: String,

    /// Target commit (optional if using range syntax like HEAD~5..HEAD)
    to: Option<String>,

    /// Path to repository (defaults to current directory)
    #[arg(short, long, default_value = ".")]
    path: String,

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

    /// Filter which line types to show (comma-separated: code,blank,docs,comments)
    /// If not specified, all types are shown.
    #[arg(short = 't', long = "type", value_delimiter = ',')]
    types: Vec<LineType>,
}

/// Types of lines that can be filtered
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum LineType {
    Code,
    Blank,
    Docs,
    Comments,
}

/// Helper to check which line types should be displayed
#[derive(Debug, Clone)]
struct TypeFilter {
    code: bool,
    blank: bool,
    docs: bool,
    comments: bool,
}

impl TypeFilter {
    /// Create a filter from the CLI args. If empty, show all types.
    fn from_args(types: &[LineType]) -> Self {
        if types.is_empty() {
            // Show all types by default
            Self {
                code: true,
                blank: true,
                docs: true,
                comments: true,
            }
        } else {
            Self {
                code: types.contains(&LineType::Code),
                blank: types.contains(&LineType::Blank),
                docs: types.contains(&LineType::Docs),
                comments: types.contains(&LineType::Comments),
            }
        }
    }

    /// Build column headers based on filter
    fn headers(&self) -> Vec<&'static str> {
        let mut cols = Vec::new();
        if self.code {
            cols.push("Code");
        }
        if self.blank {
            cols.push("Blank");
        }
        if self.docs {
            cols.push("Docs");
        }
        if self.comments {
            cols.push("Comments");
        }
        cols.push("Total");
        cols
    }

    /// Format values based on filter
    fn values(&self, code: u64, blank: u64, docs: u64, comments: u64) -> Vec<u64> {
        let mut vals = Vec::new();
        if self.code {
            vals.push(code);
        }
        if self.blank {
            vals.push(blank);
        }
        if self.docs {
            vals.push(docs);
        }
        if self.comments {
            vals.push(comments);
        }
        // Calculate total based on selected types only
        let total = (if self.code { code } else { 0 })
            + (if self.blank { blank } else { 0 })
            + (if self.docs { docs } else { 0 })
            + (if self.comments { comments } else { 0 });
        vals.push(total);
        vals
    }

    /// Format diff values based on filter (returns strings like "+x/-y/z")
    fn diff_values(&self, added: &Locs, removed: &Locs) -> Vec<String> {
        let mut vals = Vec::new();
        if self.code {
            vals.push(format_diff_cell(added.code, removed.code));
        }
        if self.blank {
            vals.push(format_diff_cell(added.blanks, removed.blanks));
        }
        if self.docs {
            vals.push(format_diff_cell(added.docs, removed.docs));
        }
        if self.comments {
            vals.push(format_diff_cell(added.comments, removed.comments));
        }
        // Calculate totals based on selected types
        let added_total = (if self.code { added.code } else { 0 })
            + (if self.blank { added.blanks } else { 0 })
            + (if self.docs { added.docs } else { 0 })
            + (if self.comments { added.comments } else { 0 });
        let removed_total = (if self.code { removed.code } else { 0 })
            + (if self.blank { removed.blanks } else { 0 })
            + (if self.docs { removed.docs } else { 0 })
            + (if self.comments { removed.comments } else { 0 });
        vals.push(format_diff_cell(added_total, removed_total));
        vals
    }

    /// Get number of columns (including Total)
    fn column_count(&self) -> usize {
        let mut count = 1; // Total always included
        if self.code {
            count += 1;
        }
        if self.blank {
            count += 1;
        }
        if self.docs {
            count += 1;
        }
        if self.comments {
            count += 1;
        }
        count
    }
}

/// Format a diff cell as "+added/-removed/net"
fn format_diff_cell(added: u64, removed: u64) -> String {
    let net = added as i64 - removed as i64;
    format!("+{}/-{}/{}", added, removed, net)
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
    let type_filter = TypeFilter::from_args(&args.common.types);

    let mut options = CountOptions::new()
        .crates(args.common.crates.clone())
        .filter(filter);

    if args.by_file {
        options = options.with_file_stats();
    }

    if args.by_module {
        options = options.with_module_stats();
    }

    let result = count_workspace(&args.path, options)?;

    match args.common.output {
        OutputFormat::Table => print_count_table(
            &result,
            args.by_crate,
            args.by_file,
            args.by_module,
            &type_filter,
        ),
        OutputFormat::Json => print_count_json(&result)?,
        OutputFormat::Csv => print_count_csv(
            &result,
            args.by_crate,
            args.by_file,
            args.by_module,
            &type_filter,
        ),
    }

    Ok(())
}

fn run_diff(args: DiffArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Parse commit range - support both "from..to" and "from to" syntax
    let (from, to) = parse_commit_range(&args.from, args.to.as_deref())?;

    let filter = build_filter(&args.common)?;
    let type_filter = TypeFilter::from_args(&args.common.types);

    let mut options = DiffOptions::new()
        .crates(args.common.crates.clone())
        .filter(filter);

    if args.by_file {
        options = options.with_file_stats();
    }

    let result = diff_commits(&args.path, &from, &to, options)?;

    match args.common.output {
        OutputFormat::Table => print_diff_table(&result, args.by_crate, args.by_file, &type_filter),
        OutputFormat::Json => print_diff_json(&result)?,
        OutputFormat::Csv => print_diff_csv(&result, args.by_crate, args.by_file, &type_filter),
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
    type_filter: &TypeFilter,
) {
    let headers = type_filter.headers();
    let col_count = type_filter.column_count();

    if by_module && !result.modules.is_empty() {
        println!("By-module breakdown:");
        // Build dynamic header
        print!("{:<40} {:>6}", "Module", "Files");
        for h in &headers {
            print!(" {:>8}", h);
        }
        println!();
        println!("{}", "-".repeat(48 + col_count * 9));

        for module in &result.modules {
            let name = if module.name.is_empty() {
                "(root)"
            } else {
                &module.name
            };
            let truncated = if name.len() > 38 {
                format!("..{}", &name[name.len() - 36..])
            } else {
                name.to_string()
            };

            print!("{:<40} {:>6}", truncated, module.files.len());
            let vals = type_filter.values(
                module.stats.code(),
                module.stats.blanks(),
                module.stats.docs(),
                module.stats.comments(),
            );
            for v in vals {
                print!(" {:>8}", v);
            }
            println!();
        }
        println!();
    }

    if by_file && !result.files.is_empty() {
        println!("By-file breakdown:");
        // Build dynamic header
        print!("{:<60}", "File");
        for h in &headers {
            print!(" {:>8}", h);
        }
        println!();
        println!("{}", "-".repeat(60 + col_count * 9));

        for file in &result.files {
            let path_str = file.path.to_string_lossy();
            let truncated = if path_str.len() > 58 {
                format!("..{}", &path_str[path_str.len() - 56..])
            } else {
                path_str.to_string()
            };

            print!("{:<60}", truncated);
            let vals = type_filter.values(
                file.stats.code(),
                file.stats.blanks(),
                file.stats.docs(),
                file.stats.comments(),
            );
            for v in vals {
                print!(" {:>8}", v);
            }
            println!();
        }
        println!();
    }

    if by_crate && !result.crates.is_empty() {
        println!("By-crate breakdown:");
        for crate_stats in &result.crates {
            println!(
                "\n{} ({} files):",
                crate_stats.name, crate_stats.stats.file_count
            );
            print_stats_table(&crate_stats.stats, type_filter);
        }
        println!();
    }

    if by_crate || by_file || by_module {
        println!("Total ({} files):", result.total.file_count);
    } else {
        println!("File count: {}", result.total.file_count);
    }
    print_stats_table(&result.total, type_filter);
}

fn print_stats_table(stats: &LocStats, type_filter: &TypeFilter) {
    let headers = type_filter.headers();

    // Build header
    print!("{:<12}", "Type");
    for h in &headers {
        print!(" | {:>12}", h);
    }
    println!();
    println!("{}", "-".repeat(15 + headers.len() * 15));

    print_locs_row("Main", &stats.main, type_filter);
    print_locs_row("Tests", &stats.tests, type_filter);
    print_locs_row("Examples", &stats.examples, type_filter);

    println!("{}", "-".repeat(15 + headers.len() * 15));

    // Total row
    print!("{:<12}", "");
    let vals = type_filter.values(stats.code(), stats.blanks(), stats.docs(), stats.comments());
    for v in vals {
        print!(" | {:>12}", v);
    }
    println!();
}

fn print_locs_row(name: &str, locs: &Locs, type_filter: &TypeFilter) {
    print!("{:<12}", name);
    let vals = type_filter.values(locs.code, locs.blanks, locs.docs, locs.comments);
    for v in vals {
        print!(" | {:>12}", v);
    }
    println!();
}

fn print_count_json(result: &CountResult) -> Result<(), serde_json::Error> {
    // Direct serialization of the library type - no transformation needed
    println!("{}", serde_json::to_string_pretty(result)?);
    Ok(())
}

fn print_count_csv(
    result: &CountResult,
    by_crate: bool,
    by_file: bool,
    by_module: bool,
    type_filter: &TypeFilter,
) {
    // Build header dynamically
    let mut header = "type,name".to_string();
    if type_filter.code {
        header.push_str(",code");
    }
    if type_filter.blank {
        header.push_str(",blanks");
    }
    if type_filter.docs {
        header.push_str(",docs");
    }
    if type_filter.comments {
        header.push_str(",comments");
    }
    header.push_str(",total");
    println!("{}", header);

    // Helper to format a row
    let format_row = |type_name: &str, name: &str, locs: &Locs, tf: &TypeFilter| {
        let vals = tf.values(locs.code, locs.blanks, locs.docs, locs.comments);
        let vals_str: Vec<String> = vals.iter().map(|v| v.to_string()).collect();
        format!("{},\"{}\",{}", type_name, name, vals_str.join(","))
    };

    let format_stats_row = |type_name: &str, name: &str, stats: &LocStats, tf: &TypeFilter| {
        let vals = tf.values(stats.code(), stats.blanks(), stats.docs(), stats.comments());
        let vals_str: Vec<String> = vals.iter().map(|v| v.to_string()).collect();
        format!("{},\"{}\",{}", type_name, name, vals_str.join(","))
    };

    if by_module {
        for module in &result.modules {
            let name = if module.name.is_empty() {
                "(root)"
            } else {
                &module.name
            };
            println!(
                "{}",
                format_stats_row("module", name, &module.stats, type_filter)
            );
        }
    }

    if by_file {
        for file in &result.files {
            println!(
                "{}",
                format_stats_row(
                    "file",
                    &file.path.to_string_lossy(),
                    &file.stats,
                    type_filter
                )
            );
        }
    }

    if by_crate {
        for crate_stats in &result.crates {
            println!(
                "{}",
                format_stats_row("crate", &crate_stats.name, &crate_stats.stats, type_filter)
            );
        }
    }

    let stats = &result.total;
    println!("{}", format_row("main", "total", &stats.main, type_filter));
    println!(
        "{}",
        format_row("tests", "total", &stats.tests, type_filter)
    );
    println!(
        "{}",
        format_row("examples", "total", &stats.examples, type_filter)
    );
    println!("{}", format_stats_row("total", "total", stats, type_filter));
}

// ============================================================================
// Diff output functions
// ============================================================================

fn print_diff_table(result: &DiffResult, by_crate: bool, by_file: bool, type_filter: &TypeFilter) {
    println!("Diff: {} → {}", result.from_commit, result.to_commit);
    println!();

    let headers = type_filter.headers();
    let col_count = type_filter.column_count();

    if by_file && !result.files.is_empty() {
        println!("By-file breakdown:");
        // Build dynamic header
        print!("{:<50} {:>6}", "File", "Change");
        for h in &headers {
            print!(" {:>14}", h);
        }
        println!();
        println!("{}", "-".repeat(58 + col_count * 15));

        for file in &result.files {
            let path_str = file.path.to_string_lossy();
            let truncated = if path_str.len() > 48 {
                format!("..{}", &path_str[path_str.len() - 46..])
            } else {
                path_str.to_string()
            };

            let change_type = match file.change_type {
                rustloclib::FileChangeType::Added => "+",
                rustloclib::FileChangeType::Deleted => "-",
                rustloclib::FileChangeType::Modified => "M",
            };

            // Sum up the diff across all contexts for this file
            let total_diff = sum_diff_contexts(&file.diff);

            print!("{:<50} {:>6}", truncated, change_type);
            let vals = type_filter.diff_values(&total_diff.added, &total_diff.removed);
            for v in vals {
                print!(" {:>14}", v);
            }
            println!();
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
            print_diff_stats_table(&crate_stats.diff, type_filter);
        }
        println!();
    }

    if by_crate || by_file {
        println!("Total ({} files changed):", result.total.file_count);
    } else {
        println!("Files changed: {}", result.total.file_count);
    }
    print_diff_stats_table(&result.total, type_filter);
}

fn print_diff_stats_table(diff: &LocStatsDiff, type_filter: &TypeFilter) {
    let headers = type_filter.headers();

    // Build header
    print!("{:<12}", "Type");
    for h in &headers {
        print!(" | {:>14}", h);
    }
    println!();
    println!("{}", "-".repeat(15 + headers.len() * 17));

    print_diff_locs_row("Main", &diff.main, type_filter);
    print_diff_locs_row("Tests", &diff.tests, type_filter);
    print_diff_locs_row("Examples", &diff.examples, type_filter);

    println!("{}", "-".repeat(15 + headers.len() * 17));

    // Calculate totals across contexts
    let total_added = diff.total_added();
    let total_removed = diff.total_removed();

    print!("{:<12}", "");
    let vals = type_filter.diff_values(&total_added, &total_removed);
    for v in vals {
        print!(" | {:>14}", v);
    }
    println!();
}

fn print_diff_locs_row(name: &str, diff: &LocsDiff, type_filter: &TypeFilter) {
    print!("{:<12}", name);
    let vals = type_filter.diff_values(&diff.added, &diff.removed);
    for v in vals {
        print!(" | {:>14}", v);
    }
    println!();
}

/// Sum up the diff across all contexts (main, tests, examples)
fn sum_diff_contexts(diff: &LocStatsDiff) -> LocsDiff {
    LocsDiff {
        added: diff.total_added(),
        removed: diff.total_removed(),
    }
}

fn print_diff_json(result: &DiffResult) -> Result<(), serde_json::Error> {
    // Direct serialization of the library type - no transformation needed
    println!("{}", serde_json::to_string_pretty(result)?);
    Ok(())
}

fn print_diff_csv(result: &DiffResult, by_crate: bool, by_file: bool, type_filter: &TypeFilter) {
    // Build header dynamically
    let mut header = "type,name,change".to_string();
    if type_filter.code {
        header.push_str(",code_added,code_removed,code_net");
    }
    if type_filter.blank {
        header.push_str(",blanks_added,blanks_removed,blanks_net");
    }
    if type_filter.docs {
        header.push_str(",docs_added,docs_removed,docs_net");
    }
    if type_filter.comments {
        header.push_str(",comments_added,comments_removed,comments_net");
    }
    header.push_str(",total_added,total_removed,total_net");
    println!("{}", header);

    // Helper to format a diff row
    let format_diff_row =
        |type_name: &str, name: &str, change: &str, diff: &LocsDiff, tf: &TypeFilter| {
            let mut vals = Vec::new();
            if tf.code {
                vals.push(format!(
                    "{},{},{}",
                    diff.added.code,
                    diff.removed.code,
                    diff.net_code()
                ));
            }
            if tf.blank {
                vals.push(format!(
                    "{},{},{}",
                    diff.added.blanks,
                    diff.removed.blanks,
                    diff.net_blanks()
                ));
            }
            if tf.docs {
                vals.push(format!(
                    "{},{},{}",
                    diff.added.docs,
                    diff.removed.docs,
                    diff.net_docs()
                ));
            }
            if tf.comments {
                vals.push(format!(
                    "{},{},{}",
                    diff.added.comments,
                    diff.removed.comments,
                    diff.net_comments()
                ));
            }
            // Calculate filtered total
            let added_total = (if tf.code { diff.added.code } else { 0 })
                + (if tf.blank { diff.added.blanks } else { 0 })
                + (if tf.docs { diff.added.docs } else { 0 })
                + (if tf.comments { diff.added.comments } else { 0 });
            let removed_total = (if tf.code { diff.removed.code } else { 0 })
                + (if tf.blank { diff.removed.blanks } else { 0 })
                + (if tf.docs { diff.removed.docs } else { 0 })
                + (if tf.comments {
                    diff.removed.comments
                } else {
                    0
                });
            let net_total = added_total as i64 - removed_total as i64;
            vals.push(format!("{},{},{}", added_total, removed_total, net_total));

            format!("{},\"{}\",{},{}", type_name, name, change, vals.join(","))
        };

    if by_file {
        for file in &result.files {
            let total_diff = sum_diff_contexts(&file.diff);
            let change_type_str = match file.change_type {
                rustloclib::FileChangeType::Added => "added",
                rustloclib::FileChangeType::Deleted => "deleted",
                rustloclib::FileChangeType::Modified => "modified",
            };
            println!(
                "{}",
                format_diff_row(
                    "file",
                    &file.path.to_string_lossy(),
                    change_type_str,
                    &total_diff,
                    type_filter
                )
            );
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
                format_diff_row("crate", &crate_stats.name, "-", &total_diff, type_filter)
            );
        }
    }

    // Category totals
    let d = &result.total;
    println!(
        "{}",
        format_diff_row("main", "total", "-", &d.main, type_filter)
    );
    println!(
        "{}",
        format_diff_row("tests", "total", "-", &d.tests, type_filter)
    );
    println!(
        "{}",
        format_diff_row("examples", "total", "-", &d.examples, type_filter)
    );

    // Grand total
    let total_diff = LocsDiff {
        added: d.total_added(),
        removed: d.total_removed(),
    };
    println!(
        "{}",
        format_diff_row("total", "total", "-", &total_diff, type_filter)
    );
}
