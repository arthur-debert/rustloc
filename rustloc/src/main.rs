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
    count_workspace, diff_commits, diff_workdir, Aggregation, Contexts, CountOptions, CountResult,
    DiffOptions, DiffResult, FilterConfig, WorkdirDiffMode,
};

/// Include template at compile time
const STATS_TABLE_TEMPLATE: &str = include_str!("../templates/stats_table.jinja");

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

/// Handler for count command - returns raw CountResult
fn count_handler(matches: &ArgMatches, _ctx: &CommandContext) -> HandlerResult<CountResult> {
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
    Ok(Output::Render(result))
}

/// Handler for diff command - returns raw DiffResult
fn diff_handler(matches: &ArgMatches, _ctx: &CommandContext) -> HandlerResult<DiffResult> {
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

    Ok(Output::Render(result))
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
    // default_command("count") means `rustloc .` is treated as `rustloc count .`
    let result = App::builder()
        .theme(theme)
        .default_command("count")
        .command("count", count_handler, STATS_TABLE_TEMPLATE)
        .command("diff", diff_handler, STATS_TABLE_TEMPLATE)
        .run_to_string(cmd, std::env::args());

    match result {
        RunResult::Handled(output) => {
            if !output.is_empty() {
                if output.starts_with("Error:") {
                    eprintln!("{}", output);
                    return ExitCode::FAILURE;
                }
                print!("{}", output);
            }
            ExitCode::SUCCESS
        }
        RunResult::Binary(_, _) => ExitCode::SUCCESS,
        RunResult::NoMatch(_) => {
            // Should not happen with default_command set
            eprintln!("Error: Unknown command");
            ExitCode::FAILURE
        }
    }
}
