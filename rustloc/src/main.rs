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

use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use rustloclib::{
    count_workspace, diff_commits, diff_workdir, Aggregation, Contexts, CountOptions, CountResult,
    DiffOptions, DiffResult, FilterConfig, WorkdirDiffMode,
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

    // Map CLI output format to outstanding OutputMode
    let output = match args.common.output {
        OutputFormat::Table => render::render_count(
            &result,
            args.by_crate,
            args.by_file,
            args.by_module,
            &contexts,
            &base_path,
            render::OutputMode::Term,
        )?,
        OutputFormat::TermDebug => render::render_count(
            &result,
            args.by_crate,
            args.by_file,
            args.by_module,
            &contexts,
            &base_path,
            render::OutputMode::TermDebug,
        )?,
        OutputFormat::Json => render::render_count(
            &result,
            args.by_crate,
            args.by_file,
            args.by_module,
            &contexts,
            &base_path,
            render::OutputMode::Json,
        )?,
        OutputFormat::Csv => render::render_count_csv(
            &result,
            args.by_crate,
            args.by_file,
            args.by_module,
            &contexts,
            &base_path,
        ),
    };
    print!("{}", output);
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

    // Map CLI output format to outstanding OutputMode
    let output = match args.common.output {
        OutputFormat::Table => render::render_diff(
            &result,
            args.by_crate,
            args.by_file,
            &contexts,
            &base_path,
            render::OutputMode::Term,
        )?,
        OutputFormat::TermDebug => render::render_diff(
            &result,
            args.by_crate,
            args.by_file,
            &contexts,
            &base_path,
            render::OutputMode::TermDebug,
        )?,
        OutputFormat::Json => render::render_diff(
            &result,
            args.by_crate,
            args.by_file,
            &contexts,
            &base_path,
            render::OutputMode::Json,
        )?,
        OutputFormat::Csv => {
            render::render_diff_csv(&result, args.by_crate, args.by_file, &contexts, &base_path)
        }
    };
    print!("{}", output);
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
