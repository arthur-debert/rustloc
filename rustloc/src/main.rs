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
//! rustloc . --format json
//!
//! # Filter files with glob patterns
//! rustloc . --include "src/**/*.rs" --exclude "**/generated/**"
//! ```
//!
//! ## Origins
//!
//! The parsing logic is adapted from [cargo-warloc](https://github.com/Maximkaaa/cargo-warloc)
//! by Maxim Gritsenko. This CLI wraps rustloclib to provide a user-friendly interface.

use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use rustloclib::{count_workspace, CountOptions, CountResult, FilterConfig, LocStats, Locs};

/// Rust-aware lines of code counter with test/code separation
#[derive(Parser, Debug)]
#[command(name = "rustloc")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to analyze (defaults to current directory)
    #[arg(default_value = ".")]
    path: String,

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
    #[arg(short, long, value_enum, default_value = "table")]
    format: OutputFormat,

    /// Show per-crate breakdown
    #[arg(long)]
    per_crate: bool,

    /// Show per-file breakdown
    #[arg(long)]
    per_file: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Table,
    Json,
    Csv,
}

fn main() -> ExitCode {
    let args = Args::parse();

    if let Err(e) = run(args) {
        eprintln!("Error: {e}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    // Build filter config
    let mut filter = FilterConfig::new();
    for pattern in &args.include {
        filter = filter.include(pattern)?;
    }
    for pattern in &args.exclude {
        filter = filter.exclude(pattern)?;
    }

    // Build count options
    let mut options = CountOptions::new()
        .crates(args.crates.clone())
        .filter(filter);

    if args.per_file {
        options = options.with_file_stats();
    }

    // Run the count
    let result = count_workspace(&args.path, options)?;

    // Output results
    match args.format {
        OutputFormat::Table => print_table(&result, args.per_crate, args.per_file),
        OutputFormat::Json => print_json(&result, args.per_crate, args.per_file)?,
        OutputFormat::Csv => print_csv(&result, args.per_crate, args.per_file),
    }

    Ok(())
}

fn print_table(result: &CountResult, per_crate: bool, per_file: bool) {
    // Print per-file breakdown if requested
    if per_file && !result.files.is_empty() {
        println!("Per-file breakdown:");
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

    // Print per-crate breakdown if requested
    if per_crate && !result.crates.is_empty() {
        println!("Per-crate breakdown:");
        for crate_stats in &result.crates {
            println!(
                "\n{} ({} files):",
                crate_stats.name, crate_stats.stats.file_count
            );
            print_stats_table(&crate_stats.stats);
        }
        println!();
    }

    // Print totals
    if per_crate || per_file {
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

fn print_json(
    result: &CountResult,
    per_crate: bool,
    per_file: bool,
) -> Result<(), serde_json::Error> {
    #[derive(serde::Serialize)]
    struct JsonOutput<'a> {
        file_count: u64,
        totals: JsonStats<'a>,
        #[serde(skip_serializing_if = "Option::is_none")]
        crates: Option<Vec<JsonCrate<'a>>>,
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
        crates: if per_crate {
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
        files: if per_file {
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

fn print_csv(result: &CountResult, per_crate: bool, per_file: bool) {
    // CSV header
    println!("type,name,code,blanks,docs,comments,total");

    // Per-file rows
    if per_file {
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

    // Per-crate rows
    if per_crate {
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

    // Category totals
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

    // Grand total
    println!(
        "total,total,{},{},{},{},{}",
        stats.code(),
        stats.blanks(),
        stats.docs(),
        stats.comments(),
        stats.total()
    );
}
