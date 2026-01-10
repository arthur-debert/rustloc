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
        OutputFormat::Table => {
            print_count_table(&result, args.by_crate, args.by_file, args.by_module)
        }
        OutputFormat::Json => {
            print_count_json(&result, args.by_crate, args.by_file, args.by_module)?
        }
        OutputFormat::Csv => print_count_csv(&result, args.by_crate, args.by_file, args.by_module),
    }

    Ok(())
}

fn run_diff(args: DiffArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Parse commit range - support both "from..to" and "from to" syntax
    let (from, to) = parse_commit_range(&args.from, args.to.as_deref())?;

    let filter = build_filter(&args.common)?;

    let mut options = DiffOptions::new()
        .crates(args.common.crates.clone())
        .filter(filter);

    if args.by_file {
        options = options.with_file_stats();
    }

    let result = diff_commits(&args.path, &from, &to, options)?;

    match args.common.output {
        OutputFormat::Table => print_diff_table(&result, args.by_crate, args.by_file),
        OutputFormat::Json => print_diff_json(&result, args.by_crate, args.by_file)?,
        OutputFormat::Csv => print_diff_csv(&result, args.by_crate, args.by_file),
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

fn print_count_table(result: &CountResult, by_crate: bool, by_file: bool, by_module: bool) {
    if by_module && !result.modules.is_empty() {
        println!("By-module breakdown:");
        println!(
            "{:<40} {:>6} {:>8} {:>8} {:>8} {:>8} {:>8}",
            "Module", "Files", "Code", "Blank", "Docs", "Comments", "Total"
        );
        println!("{}", "-".repeat(86));

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

            println!(
                "{:<40} {:>6} {:>8} {:>8} {:>8} {:>8} {:>8}",
                truncated,
                module.files.len(),
                module.stats.code(),
                module.stats.blanks(),
                module.stats.docs(),
                module.stats.comments(),
                module.stats.total()
            );
        }
        println!();
    }

    if by_file && !result.files.is_empty() {
        println!("By-file breakdown:");
        println!(
            "{:<60} {:>8} {:>8} {:>8} {:>8} {:>8}",
            "File", "Code", "Blank", "Docs", "Comments", "Total"
        );
        println!("{}", "-".repeat(100));

        for file in &result.files {
            let path_str = file.path.to_string_lossy();
            let truncated = if path_str.len() > 58 {
                format!("..{}", &path_str[path_str.len() - 56..])
            } else {
                path_str.to_string()
            };

            let total = &file.stats;
            println!(
                "{:<60} {:>8} {:>8} {:>8} {:>8} {:>8}",
                truncated,
                total.code(),
                total.blanks(),
                total.docs(),
                total.comments(),
                total.total()
            );
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
            print_stats_table(&crate_stats.stats);
        }
        println!();
    }

    if by_crate || by_file || by_module {
        println!("Total ({} files):", result.total.file_count);
    } else {
        println!("File count: {}", result.total.file_count);
    }
    print_stats_table(&result.total);
}

fn print_stats_table(stats: &LocStats) {
    println!(
        "{:<12} | {:>12} | {:>12} | {:>12} | {:>12} | {:>12}",
        "Type", "Code", "Blank", "Doc comments", "Comments", "Total"
    );
    println!("{}", "-".repeat(79));

    print_locs_row("Main", &stats.main);
    print_locs_row("Tests", &stats.tests);
    print_locs_row("Examples", &stats.examples);

    println!("{}", "-".repeat(79));
    println!(
        "{:<12} | {:>12} | {:>12} | {:>12} | {:>12} | {:>12}",
        "",
        stats.code(),
        stats.blanks(),
        stats.docs(),
        stats.comments(),
        stats.total()
    );
}

fn print_locs_row(name: &str, locs: &Locs) {
    println!(
        "{:<12} | {:>12} | {:>12} | {:>12} | {:>12} | {:>12}",
        name,
        locs.code,
        locs.blanks,
        locs.docs,
        locs.comments,
        locs.total()
    );
}

fn print_count_json(
    result: &CountResult,
    by_crate: bool,
    by_file: bool,
    by_module: bool,
) -> Result<(), serde_json::Error> {
    #[derive(serde::Serialize)]
    struct JsonOutput<'a> {
        file_count: u64,
        totals: JsonStats<'a>,
        #[serde(skip_serializing_if = "Option::is_none")]
        crates: Option<Vec<JsonCrate<'a>>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        modules: Option<Vec<JsonModule<'a>>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        files: Option<Vec<JsonFile<'a>>>,
    }

    #[derive(serde::Serialize)]
    struct JsonStats<'a> {
        main: &'a Locs,
        tests: &'a Locs,
        examples: &'a Locs,
        total: JsonTotals,
    }

    #[derive(serde::Serialize)]
    struct JsonTotals {
        code: u64,
        blanks: u64,
        docs: u64,
        comments: u64,
        total: u64,
    }

    #[derive(serde::Serialize)]
    struct JsonCrate<'a> {
        name: &'a str,
        file_count: u64,
        stats: JsonStats<'a>,
    }

    #[derive(serde::Serialize)]
    struct JsonModule<'a> {
        name: &'a str,
        file_count: usize,
        stats: JsonStats<'a>,
    }

    #[derive(serde::Serialize)]
    struct JsonFile<'a> {
        path: &'a str,
        stats: JsonStats<'a>,
    }

    fn make_json_stats(stats: &LocStats) -> JsonStats<'_> {
        JsonStats {
            main: &stats.main,
            tests: &stats.tests,
            examples: &stats.examples,
            total: JsonTotals {
                code: stats.code(),
                blanks: stats.blanks(),
                docs: stats.docs(),
                comments: stats.comments(),
                total: stats.total(),
            },
        }
    }

    let output = JsonOutput {
        file_count: result.total.file_count,
        totals: make_json_stats(&result.total),
        crates: if by_crate {
            Some(
                result
                    .crates
                    .iter()
                    .map(|c| JsonCrate {
                        name: &c.name,
                        file_count: c.stats.file_count,
                        stats: make_json_stats(&c.stats),
                    })
                    .collect(),
            )
        } else {
            None
        },
        modules: if by_module {
            Some(
                result
                    .modules
                    .iter()
                    .map(|m| JsonModule {
                        name: if m.name.is_empty() { "(root)" } else { &m.name },
                        file_count: m.files.len(),
                        stats: make_json_stats(&m.stats),
                    })
                    .collect(),
            )
        } else {
            None
        },
        files: if by_file {
            Some(
                result
                    .files
                    .iter()
                    .map(|f| JsonFile {
                        path: f.path.to_str().unwrap_or("<invalid utf8>"),
                        stats: make_json_stats(&f.stats),
                    })
                    .collect(),
            )
        } else {
            None
        },
    };

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_count_csv(result: &CountResult, by_crate: bool, by_file: bool, by_module: bool) {
    println!("type,name,code,blanks,docs,comments,total");

    if by_module {
        for module in &result.modules {
            let name = if module.name.is_empty() {
                "(root)"
            } else {
                &module.name
            };
            println!(
                "module,\"{}\",{},{},{},{},{}",
                name,
                module.stats.code(),
                module.stats.blanks(),
                module.stats.docs(),
                module.stats.comments(),
                module.stats.total()
            );
        }
    }

    if by_file {
        for file in &result.files {
            let stats = &file.stats;
            println!(
                "file,\"{}\",{},{},{},{},{}",
                file.path.display(),
                stats.code(),
                stats.blanks(),
                stats.docs(),
                stats.comments(),
                stats.total()
            );
        }
    }

    if by_crate {
        for crate_stats in &result.crates {
            println!(
                "crate,\"{}\",{},{},{},{},{}",
                crate_stats.name,
                crate_stats.stats.code(),
                crate_stats.stats.blanks(),
                crate_stats.stats.docs(),
                crate_stats.stats.comments(),
                crate_stats.stats.total()
            );
        }
    }

    let stats = &result.total;
    println!(
        "main,total,{},{},{},{},{}",
        stats.main.code,
        stats.main.blanks,
        stats.main.docs,
        stats.main.comments,
        stats.main.total()
    );
    println!(
        "tests,total,{},{},{},{},{}",
        stats.tests.code,
        stats.tests.blanks,
        stats.tests.docs,
        stats.tests.comments,
        stats.tests.total()
    );
    println!(
        "examples,total,{},{},{},{},{}",
        stats.examples.code,
        stats.examples.blanks,
        stats.examples.docs,
        stats.examples.comments,
        stats.examples.total()
    );
    println!(
        "total,total,{},{},{},{},{}",
        stats.code(),
        stats.blanks(),
        stats.docs(),
        stats.comments(),
        stats.total()
    );
}

// ============================================================================
// Diff output functions
// ============================================================================

fn print_diff_table(result: &DiffResult, by_crate: bool, by_file: bool) {
    println!("Diff: {} → {}", result.from_commit, result.to_commit);
    println!();

    if by_file && !result.files.is_empty() {
        println!("By-file breakdown:");
        println!(
            "{:<50} {:>6} {:>14} {:>14} {:>14} {:>14} {:>14}",
            "File", "Change", "Code", "Blank", "Docs", "Comments", "Total"
        );
        println!("{}", "-".repeat(130));

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

            println!(
                "{:<50} {:>6} {:>14} {:>14} {:>14} {:>14} {:>14}",
                truncated,
                change_type,
                format_diff_cell(total_diff.added.code, total_diff.removed.code),
                format_diff_cell(total_diff.added.blanks, total_diff.removed.blanks),
                format_diff_cell(total_diff.added.docs, total_diff.removed.docs),
                format_diff_cell(total_diff.added.comments, total_diff.removed.comments),
                format_diff_cell(total_diff.added.total(), total_diff.removed.total()),
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
            print_diff_stats_table(&crate_stats.diff);
        }
        println!();
    }

    if by_crate || by_file {
        println!("Total ({} files changed):", result.total.file_count);
    } else {
        println!("Files changed: {}", result.total.file_count);
    }
    print_diff_stats_table(&result.total);
}

fn print_diff_stats_table(diff: &LocStatsDiff) {
    println!(
        "{:<12} | {:>14} | {:>14} | {:>14} | {:>14} | {:>14}",
        "Type", "Code", "Blank", "Doc comments", "Comments", "Total"
    );
    println!("{}", "-".repeat(89));

    print_diff_locs_row("Main", &diff.main);
    print_diff_locs_row("Tests", &diff.tests);
    print_diff_locs_row("Examples", &diff.examples);

    println!("{}", "-".repeat(89));

    // Calculate totals across contexts
    let total_added = diff.total_added();
    let total_removed = diff.total_removed();

    println!(
        "{:<12} | {:>14} | {:>14} | {:>14} | {:>14} | {:>14}",
        "",
        format_diff_cell(total_added.code, total_removed.code),
        format_diff_cell(total_added.blanks, total_removed.blanks),
        format_diff_cell(total_added.docs, total_removed.docs),
        format_diff_cell(total_added.comments, total_removed.comments),
        format_diff_cell(total_added.total(), total_removed.total()),
    );
}

fn print_diff_locs_row(name: &str, diff: &LocsDiff) {
    println!(
        "{:<12} | {:>14} | {:>14} | {:>14} | {:>14} | {:>14}",
        name,
        format_diff_cell(diff.added.code, diff.removed.code),
        format_diff_cell(diff.added.blanks, diff.removed.blanks),
        format_diff_cell(diff.added.docs, diff.removed.docs),
        format_diff_cell(diff.added.comments, diff.removed.comments),
        format_diff_cell(diff.added.total(), diff.removed.total()),
    );
}

/// Format a diff cell as "+added/-removed/net"
fn format_diff_cell(added: u64, removed: u64) -> String {
    let net = added as i64 - removed as i64;
    format!("+{}/-{}/{}", added, removed, net)
}

/// Sum up the diff across all contexts (main, tests, examples)
fn sum_diff_contexts(diff: &LocStatsDiff) -> LocsDiff {
    LocsDiff {
        added: diff.total_added(),
        removed: diff.total_removed(),
    }
}

fn print_diff_json(
    result: &DiffResult,
    by_crate: bool,
    by_file: bool,
) -> Result<(), serde_json::Error> {
    #[derive(serde::Serialize)]
    struct JsonDiffOutput<'a> {
        from_commit: &'a str,
        to_commit: &'a str,
        files_changed: u64,
        totals: JsonDiffStats,
        #[serde(skip_serializing_if = "Option::is_none")]
        crates: Option<Vec<JsonDiffCrate<'a>>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        files: Option<Vec<JsonDiffFile<'a>>>,
    }

    #[derive(serde::Serialize)]
    struct JsonDiffStats {
        main: JsonLocsDiff,
        tests: JsonLocsDiff,
        examples: JsonLocsDiff,
        total: JsonLocsDiff,
    }

    #[derive(serde::Serialize)]
    struct JsonLocsDiff {
        added: JsonLocs,
        removed: JsonLocs,
        net: JsonLocsNet,
    }

    #[derive(serde::Serialize)]
    struct JsonLocs {
        code: u64,
        blanks: u64,
        docs: u64,
        comments: u64,
        total: u64,
    }

    #[derive(serde::Serialize)]
    struct JsonLocsNet {
        code: i64,
        blanks: i64,
        docs: i64,
        comments: i64,
        total: i64,
    }

    #[derive(serde::Serialize)]
    struct JsonDiffCrate<'a> {
        name: &'a str,
        files_changed: u64,
        stats: JsonDiffStats,
    }

    #[derive(serde::Serialize)]
    struct JsonDiffFile<'a> {
        path: &'a str,
        change_type: &'static str,
        stats: JsonDiffStats,
    }

    fn make_json_locs_diff(diff: &LocsDiff) -> JsonLocsDiff {
        JsonLocsDiff {
            added: JsonLocs {
                code: diff.added.code,
                blanks: diff.added.blanks,
                docs: diff.added.docs,
                comments: diff.added.comments,
                total: diff.added.total(),
            },
            removed: JsonLocs {
                code: diff.removed.code,
                blanks: diff.removed.blanks,
                docs: diff.removed.docs,
                comments: diff.removed.comments,
                total: diff.removed.total(),
            },
            net: JsonLocsNet {
                code: diff.net_code(),
                blanks: diff.net_blanks(),
                docs: diff.net_docs(),
                comments: diff.net_comments(),
                total: diff.net_total(),
            },
        }
    }

    fn make_json_diff_stats(diff: &LocStatsDiff) -> JsonDiffStats {
        let total_diff = LocsDiff {
            added: diff.total_added(),
            removed: diff.total_removed(),
        };
        JsonDiffStats {
            main: make_json_locs_diff(&diff.main),
            tests: make_json_locs_diff(&diff.tests),
            examples: make_json_locs_diff(&diff.examples),
            total: make_json_locs_diff(&total_diff),
        }
    }

    let output = JsonDiffOutput {
        from_commit: &result.from_commit,
        to_commit: &result.to_commit,
        files_changed: result.total.file_count,
        totals: make_json_diff_stats(&result.total),
        crates: if by_crate {
            Some(
                result
                    .crates
                    .iter()
                    .map(|c| JsonDiffCrate {
                        name: &c.name,
                        files_changed: c.diff.file_count,
                        stats: make_json_diff_stats(&c.diff),
                    })
                    .collect(),
            )
        } else {
            None
        },
        files: if by_file {
            Some(
                result
                    .files
                    .iter()
                    .map(|f| JsonDiffFile {
                        path: f.path.to_str().unwrap_or("<invalid utf8>"),
                        change_type: match f.change_type {
                            rustloclib::FileChangeType::Added => "added",
                            rustloclib::FileChangeType::Deleted => "deleted",
                            rustloclib::FileChangeType::Modified => "modified",
                        },
                        stats: make_json_diff_stats(&f.diff),
                    })
                    .collect(),
            )
        } else {
            None
        },
    };

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_diff_csv(result: &DiffResult, by_crate: bool, by_file: bool) {
    println!("type,name,change,code_added,code_removed,code_net,blanks_added,blanks_removed,blanks_net,docs_added,docs_removed,docs_net,comments_added,comments_removed,comments_net,total_added,total_removed,total_net");

    if by_file {
        for file in &result.files {
            let total_diff = sum_diff_contexts(&file.diff);
            let change_type_str = match file.change_type {
                rustloclib::FileChangeType::Added => "added",
                rustloclib::FileChangeType::Deleted => "deleted",
                rustloclib::FileChangeType::Modified => "modified",
            };
            println!(
                "file,\"{}\",{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                file.path.display(),
                change_type_str,
                total_diff.added.code,
                total_diff.removed.code,
                total_diff.net_code(),
                total_diff.added.blanks,
                total_diff.removed.blanks,
                total_diff.net_blanks(),
                total_diff.added.docs,
                total_diff.removed.docs,
                total_diff.net_docs(),
                total_diff.added.comments,
                total_diff.removed.comments,
                total_diff.net_comments(),
                total_diff.added.total(),
                total_diff.removed.total(),
                total_diff.net_total(),
            );
        }
    }

    if by_crate {
        for crate_stats in &result.crates {
            let d = &crate_stats.diff;
            let total_added = d.total_added();
            let total_removed = d.total_removed();
            println!(
                "crate,\"{}\",-,{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                crate_stats.name,
                total_added.code,
                total_removed.code,
                d.net_code(),
                total_added.blanks,
                total_removed.blanks,
                (total_added.blanks as i64 - total_removed.blanks as i64),
                total_added.docs,
                total_removed.docs,
                (total_added.docs as i64 - total_removed.docs as i64),
                total_added.comments,
                total_removed.comments,
                (total_added.comments as i64 - total_removed.comments as i64),
                total_added.total(),
                total_removed.total(),
                d.net_total(),
            );
        }
    }

    // Category totals
    let d = &result.total;

    // Main
    println!(
        "main,total,-,{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        d.main.added.code,
        d.main.removed.code,
        d.main.net_code(),
        d.main.added.blanks,
        d.main.removed.blanks,
        d.main.net_blanks(),
        d.main.added.docs,
        d.main.removed.docs,
        d.main.net_docs(),
        d.main.added.comments,
        d.main.removed.comments,
        d.main.net_comments(),
        d.main.added.total(),
        d.main.removed.total(),
        d.main.net_total(),
    );

    // Tests
    println!(
        "tests,total,-,{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        d.tests.added.code,
        d.tests.removed.code,
        d.tests.net_code(),
        d.tests.added.blanks,
        d.tests.removed.blanks,
        d.tests.net_blanks(),
        d.tests.added.docs,
        d.tests.removed.docs,
        d.tests.net_docs(),
        d.tests.added.comments,
        d.tests.removed.comments,
        d.tests.net_comments(),
        d.tests.added.total(),
        d.tests.removed.total(),
        d.tests.net_total(),
    );

    // Examples
    println!(
        "examples,total,-,{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        d.examples.added.code,
        d.examples.removed.code,
        d.examples.net_code(),
        d.examples.added.blanks,
        d.examples.removed.blanks,
        d.examples.net_blanks(),
        d.examples.added.docs,
        d.examples.removed.docs,
        d.examples.net_docs(),
        d.examples.added.comments,
        d.examples.removed.comments,
        d.examples.net_comments(),
        d.examples.added.total(),
        d.examples.removed.total(),
        d.examples.net_total(),
    );

    // Grand total
    let total_added = d.total_added();
    let total_removed = d.total_removed();
    println!(
        "total,total,-,{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        total_added.code,
        total_removed.code,
        d.net_code(),
        total_added.blanks,
        total_removed.blanks,
        (total_added.blanks as i64 - total_removed.blanks as i64),
        total_added.docs,
        total_removed.docs,
        (total_added.docs as i64 - total_removed.docs as i64),
        total_added.comments,
        total_removed.comments,
        (total_added.comments as i64 - total_removed.comments as i64),
        total_added.total(),
        total_removed.total(),
        d.net_total(),
    );
}
