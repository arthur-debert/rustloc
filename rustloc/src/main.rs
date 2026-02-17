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
//! - **Rust-aware**: Distinguishes code, tests, examples, comments, docs, and blanks
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
//! # Show only code and tests (exclude docs, comments, blanks)
//! rustloc . --type code,tests
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

use clap::{Args, CommandFactory, Parser, Subcommand};
use standout::cli::{App, Dispatch, RunResult};
use standout::{embed_styles, embed_templates};

/// Rust-aware lines of code counter with test/code separation
#[derive(Parser)]
#[command(name = "rustloc")]
#[command(version, author = "Arthur Debert")]
#[command(long_about = "\
Rust-aware lines of code counter with test/code separation.

Parses Rust source files and categorizes each line as code, tests, examples,
docs, comments, or blanks. Understands #[cfg(test)] blocks, doc comments,
and Cargo workspace structure.")]
#[command(after_help = "Use --help for examples")]
#[command(after_long_help = "\
Examples:
  rustloc                              Totals for current directory
  rustloc --by-crate                   Group by crate
  rustloc --by-module                  Group by module
  rustloc --by-file                    Group by file
  rustloc --by-file -o -code           Sort files by code (descending)
  rustloc -t code,tests               Only code and test lines
  rustloc -c my-lib                    Only a specific crate
  rustloc diff                         Changes since last commit
  rustloc diff HEAD~5..HEAD --by-file  Per-file diff between commits")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[command(flatten)]
    count_args: CountArgs,
}

/// Commands enum with Dispatch derive for automatic handler routing
#[derive(Subcommand, Dispatch)]
#[dispatch(handlers = handlers)]
enum Commands {
    /// Count lines of code (default command)
    #[dispatch(default, template = "stats_table")]
    Count(CountArgs),

    /// Show LOC differences between git commits
    #[dispatch(template = "stats_table")]
    #[command(long_about = "\
Show LOC differences between git commits.

Compares line counts between two commits, or between the working directory
and HEAD. Shows additions, removals, and net change per line type.")]
    #[command(after_help = "Use --help for examples")]
    #[command(after_long_help = "\
Examples:
  rustloc diff                         All uncommitted changes
  rustloc diff --staged                Only staged changes
  rustloc diff HEAD~5..HEAD            Between two commits
  rustloc diff main..feature --by-file Per-file breakdown
  rustloc diff -t code                 Only code line changes")]
    Diff(DiffArgs),
}

/// Shared arguments for count command and top-level
#[derive(Args, Clone, Default)]
struct CountArgs {
    /// Path to analyze
    #[arg(default_value = ".")]
    path: String,

    /// Only count specific crate(s) [-c my-lib -c my-cli]
    #[arg(short = 'c', long = "crate", action = clap::ArgAction::Append)]
    crates: Vec<String>,

    /// Only include files matching a glob [-i "src/**/*.rs"]
    #[arg(short = 'i', long = "include", action = clap::ArgAction::Append)]
    include: Vec<String>,

    /// Exclude files matching a glob [-e "**/generated/**"]
    #[arg(short = 'e', long = "exclude", action = clap::ArgAction::Append)]
    exclude: Vec<String>,

    /// Line types to show (comma-separated)
    #[arg(short = 't', long = "type", value_delimiter = ',')]
    #[arg(value_parser = ["code", "tests", "examples", "docs", "comments", "blanks", "total"])]
    #[arg(long_help = "\
Line types to show (comma-separated).

By default all types are shown. Use this to filter to specific types.
Values: code, tests, examples, docs, comments, blanks, total

  -t code,tests       Show only code and test lines
  -t code             Show only code lines")]
    line_types: Vec<String>,

    /// Group results by crate
    #[arg(long = "by-crate", conflicts_with_all = ["by_file", "by_module"])]
    by_crate: bool,

    /// Group results by file
    #[arg(short = 'f', long = "by-file", conflicts_with_all = ["by_crate", "by_module"])]
    by_file: bool,

    /// Group results by module
    #[arg(short = 'm', long = "by-module", conflicts_with_all = ["by_crate", "by_file"])]
    by_module: bool,

    /// Sort by field [-o FIELD, prefix - for desc: -o -code]
    #[arg(
        short = 'o',
        long = "ordering",
        value_name = "FIELD",
        allow_hyphen_values = true
    )]
    #[arg(long_help = "\
Sort by field. Prefix with - for descending, + for ascending.

Fields: label, code, tests, examples, docs, comments, blanks, total
Default direction: descending for numeric fields, ascending for label.

  -o code         Sort by code lines (descending)
  -o -code        Sort by code lines (descending, explicit)
  -o +code        Sort by code lines (ascending)
  -o label        Sort by name (ascending)")]
    ordering: Option<String>,
}

/// Arguments for diff command
#[derive(Args, Clone)]
struct DiffArgs {
    /// Base commit or range [HEAD~5..HEAD, main..feature, main]
    #[arg(long_help = "\
Base commit or range.

Accepts commit ranges (HEAD~5..HEAD, main..feature) or a single commit
(compared against HEAD). Without arguments, diffs the working directory.")]
    from: Option<String>,

    /// Target commit (when not using .. range syntax)
    to: Option<String>,

    /// Path to the repository
    #[arg(short = 'p', long = "path", default_value = ".")]
    path: String,

    /// Only staged changes (like git diff --cached)
    #[arg(long = "staged", visible_alias = "cached")]
    staged: bool,

    /// Only count specific crate(s)
    #[arg(short = 'c', long = "crate", action = clap::ArgAction::Append)]
    crates: Vec<String>,

    /// Only include files matching a glob
    #[arg(short = 'i', long = "include", action = clap::ArgAction::Append)]
    include: Vec<String>,

    /// Exclude files matching a glob
    #[arg(short = 'e', long = "exclude", action = clap::ArgAction::Append)]
    exclude: Vec<String>,

    /// Line types to show (comma-separated)
    #[arg(short = 't', long = "type", value_delimiter = ',')]
    #[arg(value_parser = ["code", "tests", "examples", "docs", "comments", "blanks", "total"])]
    #[arg(long_help = "\
Line types to show (comma-separated).

By default all types are shown. Use this to filter to specific types.
Values: code, tests, examples, docs, comments, blanks, total

  -t code,tests       Show only code and test lines
  -t code             Show only code lines")]
    line_types: Vec<String>,

    /// Group results by crate
    #[arg(long = "by-crate", conflicts_with_all = ["by_file", "by_module"])]
    by_crate: bool,

    /// Group results by file
    #[arg(short = 'f', long = "by-file", conflicts_with_all = ["by_crate", "by_module"])]
    by_file: bool,

    /// Group results by module
    #[arg(short = 'm', long = "by-module", conflicts_with_all = ["by_crate", "by_file"])]
    by_module: bool,

    /// Sort by field [-o FIELD, prefix - for desc: -o -code]
    #[arg(
        short = 'o',
        long = "ordering",
        value_name = "FIELD",
        allow_hyphen_values = true
    )]
    #[arg(long_help = "\
Sort by field. Prefix with - for descending, + for ascending.

Fields: label, code, tests, examples, docs, comments, blanks, total
Default direction: descending for numeric fields, ascending for label.

  -o code         Sort by code lines (descending)
  -o -code        Sort by code lines (descending, explicit)
  -o +code        Sort by code lines (ascending)
  -o label        Sort by name (ascending)")]
    ordering: Option<String>,
}

/// Command handlers module
mod handlers {
    use clap::ArgMatches;
    use rustloclib::{
        count_directory, count_file, count_workspace, diff_commits, diff_workdir, Aggregation,
        CountOptions, CountQuerySet, CountResult, DiffOptions, DiffQuerySet, FilterConfig,
        LOCTable, LineTypes, OrderBy, OrderDirection, Ordering, WorkdirDiffMode,
    };
    use standout::cli::{CommandContext, HandlerResult, Output};

    /// Check if the output mode is a structured data format (json, yaml, xml, csv).
    fn is_structured_output(matches: &ArgMatches) -> bool {
        matches!(
            matches
                .get_one::<String>("_output_mode")
                .map(|s| s.as_str()),
            Some("json" | "yaml" | "xml" | "csv")
        )
    }

    /// Handler for count command
    pub fn count(matches: &ArgMatches, _ctx: &CommandContext) -> HandlerResult<serde_json::Value> {
        let path = matches
            .get_one::<String>("path")
            .map(|s| s.as_str())
            .unwrap_or(".");
        let filter = build_filter(matches)?;
        let line_types = extract_line_types(matches);
        let crates = extract_crates(matches);
        let ordering = extract_ordering(matches);

        let by_file = matches.get_flag("by_file");
        let by_module = matches.get_flag("by_module");
        let by_crate = matches.get_flag("by_crate");

        let aggregation = if by_file {
            Aggregation::ByFile
        } else if by_module {
            Aggregation::ByModule
        } else if by_crate {
            Aggregation::ByCrate
        } else {
            Aggregation::Total
        };

        let structured = is_structured_output(matches);
        let effective_line_types = if structured {
            LineTypes::everything()
        } else {
            line_types
        };

        let path_ref = std::path::Path::new(path);
        let is_workspace = path_ref.is_dir() && path_ref.join("Cargo.toml").exists()
            || path_ref.is_file() && path_ref.file_name() == Some("Cargo.toml".as_ref());

        let result: CountResult = if is_workspace {
            let options = CountOptions::new()
                .crates(crates)
                .filter(filter)
                .aggregation(aggregation)
                .line_types(effective_line_types);
            count_workspace(path, options)?
        } else if path_ref.is_file() {
            let stats = count_file(path)?;
            let mut r = CountResult::new();
            r.root = path_ref.to_path_buf();
            r.file_count = 1;
            r.total = stats;
            r
        } else {
            count_directory(path, &filter)?
        };

        if structured {
            let queryset =
                CountQuerySet::from_result(&result, aggregation, LineTypes::everything(), ordering);
            Ok(Output::Render(serde_json::to_value(queryset)?))
        } else {
            let queryset = CountQuerySet::from_result(&result, aggregation, line_types, ordering);
            let table = LOCTable::from_count_queryset(&queryset);
            Ok(Output::Render(serde_json::to_value(table)?))
        }
    }

    /// Handler for diff command
    pub fn diff(matches: &ArgMatches, _ctx: &CommandContext) -> HandlerResult<serde_json::Value> {
        let path = matches
            .get_one::<String>("path")
            .map(|s| s.as_str())
            .unwrap_or(".");
        let filter = build_filter(matches)?;
        let line_types = extract_line_types(matches);
        let crates = extract_crates(matches);
        let ordering = extract_ordering(matches);
        let staged = matches.get_flag("staged");

        let by_file = matches.get_flag("by_file");
        let by_module = matches.get_flag("by_module");
        let by_crate = matches.get_flag("by_crate");

        let aggregation = if by_file {
            Aggregation::ByFile
        } else if by_module {
            Aggregation::ByModule
        } else if by_crate {
            Aggregation::ByCrate
        } else {
            Aggregation::Total
        };

        let structured = is_structured_output(matches);
        let effective_line_types = if structured {
            LineTypes::everything()
        } else {
            line_types
        };

        let options = DiffOptions::new()
            .crates(crates)
            .filter(filter)
            .aggregation(aggregation)
            .line_types(effective_line_types);

        let from = matches.get_one::<String>("from");
        let to = matches.get_one::<String>("to");

        let result = if let Some(from_str) = from {
            // Commit diff
            if staged {
                return Err(anyhow::anyhow!(
                    "--staged/--cached can only be used without commit arguments"
                ));
            }
            let (from_commit, to_commit) = parse_commit_range(from_str, to.map(|s| s.as_str()))?;
            diff_commits(path, &from_commit, &to_commit, options)?
        } else {
            // Working directory diff
            let mode = if staged {
                WorkdirDiffMode::Staged
            } else {
                WorkdirDiffMode::All
            };
            diff_workdir(path, mode, options)?
        };

        if structured {
            let queryset =
                DiffQuerySet::from_result(&result, aggregation, LineTypes::everything(), ordering);
            Ok(Output::Render(serde_json::to_value(queryset)?))
        } else {
            let queryset = DiffQuerySet::from_result(&result, aggregation, line_types, ordering);
            let table = LOCTable::from_diff_queryset(&queryset);
            Ok(Output::Render(serde_json::to_value(table)?))
        }
    }

    // Helper functions

    fn parse_ordering(s: &str) -> Result<Ordering, String> {
        let (direction, field) = if let Some(stripped) = s.strip_prefix('-') {
            (OrderDirection::Descending, stripped)
        } else if let Some(stripped) = s.strip_prefix('+') {
            (OrderDirection::Ascending, stripped)
        } else {
            let order_by: OrderBy = s.parse()?;
            let direction = if order_by == OrderBy::Label {
                OrderDirection::Ascending
            } else {
                OrderDirection::Descending
            };
            return Ok(Ordering {
                by: order_by,
                direction,
            });
        };

        let order_by: OrderBy = field.parse()?;
        Ok(Ordering {
            by: order_by,
            direction,
        })
    }

    fn extract_ordering(matches: &ArgMatches) -> Ordering {
        matches
            .get_one::<String>("ordering")
            .map(|s| parse_ordering(s).unwrap_or_default())
            .unwrap_or_default()
    }

    fn extract_line_types(matches: &ArgMatches) -> LineTypes {
        let types: Vec<&str> = matches
            .get_many::<String>("line_types")
            .map(|v| v.map(|s| s.as_str()).collect())
            .unwrap_or_default();

        if types.is_empty() {
            LineTypes::default()
        } else {
            LineTypes {
                code: types.contains(&"code"),
                tests: types.contains(&"tests"),
                examples: types.contains(&"examples"),
                docs: types.contains(&"docs"),
                comments: types.contains(&"comments"),
                blanks: types.contains(&"blanks"),
                total: types.contains(&"total") || types.is_empty(),
            }
        }
    }

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

    fn extract_crates(matches: &ArgMatches) -> Vec<String> {
        matches
            .get_many::<String>("crates")
            .map(|v| v.cloned().collect())
            .unwrap_or_default()
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
}

fn run() -> Result<RunResult, anyhow::Error> {
    // Load theme from stylesheet
    use standout::StylesheetRegistry;
    let mut registry: StylesheetRegistry = embed_styles!("styles").into();
    let theme = registry.get("default")?;

    // Build the standout app with derive-based dispatch
    let app = App::builder()
        .templates(embed_templates!("templates"))
        .theme(theme)
        .commands(Commands::dispatch_config())?
        .build()?;

    Ok(app.run_to_string(Cli::command(), std::env::args()))
}

fn main() -> ExitCode {
    match run() {
        Ok(result) => match result {
            RunResult::Handled(output) => {
                if !output.is_empty() {
                    if output.starts_with("Error:") {
                        eprintln!("{}", output);
                        return ExitCode::FAILURE;
                    }
                    print!("{}", output);
                    if !output.ends_with('\n') {
                        println!();
                    }
                }
                ExitCode::SUCCESS
            }
            RunResult::Binary(_, _) => ExitCode::SUCCESS,
            RunResult::Silent => ExitCode::SUCCESS,
            RunResult::NoMatch(_) => {
                // Should not happen with default command set
                eprintln!("Error: Unknown command");
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::FAILURE
        }
    }
}
