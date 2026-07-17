//! # rustloc
//!
//! A CLI tool for counting lines of code with language-aware test/code separation.
//!
//! ## Overview
//!
//! rustloc is built on top of rustloclib and provides a command-line interface for
//! analyzing Rust, Python, TypeScript, and generic source trees. It separates production code
//! from test code, even when a language backend can find both in the same file.
//!
//! ## Features
//!
//! - **Language-aware**: Distinguishes code, tests, examples, comments, docs, and blanks
//! - **Language selection**: Rust by default; opt into Python, TypeScript, or generic counting
//! - **Cargo workspace support**: Filter by crate with `--crate` or `-c`
//! - **Glob filtering**: Include/exclude files with glob patterns
//! - **Multiple output formats**: Table (default), JSON, YAML, XML, CSV
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
//! # Analyze another backend instead of the default Rust backend
//! rustloc . --lang python
//! rustloc . --lang typescript
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

mod table;

/// Language-aware lines of code counter with test/code separation
#[derive(Parser)]
#[command(name = "rustloc")]
#[command(version, author = "Arthur Debert")]
#[command(long_about = "\
Language-aware lines of code counter with test/code separation.

Rust is analyzed by default. Python, TypeScript, and generic source files can
be selected with --lang. Rust and Python backends classify same-file test code;
the TypeScript and generic backends use file paths for code/test/example context.")]
#[command(after_help = "Use --help for examples")]
#[command(after_long_help = "\
Examples:
  rustloc                              Totals for current directory
  rustloc --by-crate                   Group by crate
  rustloc --by-module                  Group by module
  rustloc --by-file                    Group by file
  rustloc --by-file -o -code           Sort files by code (descending)
  rustloc -t code,tests               Only code and test lines
  rustloc --lang python                Analyze Python files only
  rustloc --lang rust,python           Analyze Rust and Python files
  rustloc --lang typescript            Analyze TypeScript files only
  rustloc --lang rust,typescript       Analyze Rust and TypeScript files
  rustloc -c my-lib                    Only a specific crate
  rustloc diff                         Changes since last commit
  rustloc diff --lang python           Python changes since last commit
  rustloc diff --lang typescript       TypeScript changes since last commit
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
    #[dispatch(default, template = "stats_table", post_dispatch = presentation::count)]
    Count(CountArgs),

    /// Show LOC differences between git commits
    #[dispatch(template = "stats_table", post_dispatch = presentation::diff)]
    #[command(long_about = "\
Show LOC differences between git commits.

Compares line counts between two commits, or between the working directory
and HEAD. Shows additions, removals, and net change per line type.")]
    #[command(after_help = "Use --help for examples")]
    #[command(after_long_help = "\
Revspec syntax mirrors `git diff` / `git rev-parse`: tags, branches, short
hashes, HEAD~N, ranges (a..b), and merge-base ranges (a...b) are all accepted.
A single rev is diffed against HEAD; tag objects are peeled to their target
commit automatically.

Examples:
  rustloc diff                         All uncommitted changes
  rustloc diff --staged                Only staged changes
  rustloc diff HEAD~5..HEAD            Between two commits
  rustloc diff v1.0.0..v2.0.0          Between two tags
  rustloc diff main feature --by-file  Two-arg form, per-file breakdown
  rustloc diff main...feature          From their merge base to feature
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

    /// Language backend(s) to analyze [-l rust,typescript]
    ///
    /// Example: -l rust,python or -l rust,typescript
    #[arg(short = 'l', long = "lang", value_delimiter = ',', action = clap::ArgAction::Append)]
    #[arg(long_help = "\
Language backend groups to analyze.

Default: rust
Available: rust, python, typescript, generic

  -l python            Analyze Python files only
  -l rust,python       Analyze Rust and Python files
  -l typescript        Analyze TypeScript files only
  -l rust,typescript   Analyze Rust and TypeScript files
  -l all               Analyze all available backend groups")]
    languages: Vec<String>,

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

    /// Show only the top N rows after sorting [requires --by-* aggregation]
    #[arg(long = "top", value_name = "N")]
    #[arg(long_help = "\
Truncate the result to the top N rows after sorting. The truncation is
applied after `--ordering`, so use the two together for things like
`--by-file -o -code --top 10` (the 10 files with the most code).

The total row and file count still describe the full data set, not the
truncated slice. No-op when no `--by-*` aggregation is in effect.")]
    top: Option<usize>,
}

/// Arguments for diff command
#[derive(Args, Clone)]
struct DiffArgs {
    /// Revspec or range [HEAD~5..HEAD, v1.0.0..v2.0.0, main]
    #[arg(long_help = "\
Revspec or range, parsed by gix. Common forms work: tags (annotated or
lightweight), branches, short hashes, HEAD~N, ranges (a..b), merge-base
ranges (a...b). A single rev is diffed against HEAD. Without arguments,
diffs the working directory.

Some less-common rev-parse forms (e.g. `@{-N}` for previous branch,
`:/regex` for commit-message search) aren't supported by gix yet — pass
the resolved hash from `git rev-parse` if you need them.")]
    from: Option<String>,

    /// Target revspec (alternative to a..b range syntax)
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

    /// Language backend(s) to analyze [-l rust,typescript]
    ///
    /// Example: -l rust,python or -l rust,typescript
    #[arg(short = 'l', long = "lang", value_delimiter = ',', action = clap::ArgAction::Append)]
    #[arg(long_help = "\
Language backend groups to analyze.

Default: rust
Available: rust, python, typescript, generic

  -l python            Analyze Python file changes only
  -l rust,python       Analyze Rust and Python file changes
  -l typescript        Analyze TypeScript file changes only
  -l rust,typescript   Analyze Rust and TypeScript file changes
  -l all               Analyze all available backend groups")]
    languages: Vec<String>,

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

    /// Show only the top N rows after sorting [requires --by-* aggregation]
    #[arg(long = "top", value_name = "N")]
    #[arg(long_help = "\
Truncate the result to the top N rows after sorting. The truncation is
applied after `--ordering`, so use the two together for things like
`--by-file -o -code --top 10` (the 10 files with the most code change).

The total row and file count still describe the full data set, not the
truncated slice. No-op when no `--by-*` aggregation is in effect.")]
    top: Option<usize>,
}

/// Command handlers.
///
/// Handlers are **pure with respect to presentation**: each builds and returns
/// the one canonical typed response for its command ([`CountQuerySet`] /
/// [`DiffQuerySet`]) and never inspects the output mode. Choosing how that
/// response is presented — human table, CSV rows, or direct serialization —
/// belongs to [`super::presentation`], which runs at the render boundary.
mod handlers {
    use clap::ArgMatches;
    use rustloclib::{
        available_languages, count_directory_with_options, count_file_with_filter, count_workspace,
        default_languages, diff_revspec, diff_workdir, Aggregation, CountOptions, CountQuerySet,
        CountResult, DiffOptions, DiffQuerySet, FilterConfig, LanguageName, LanguageSelection,
        LineTypes, OrderBy, OrderDirection, Ordering, WorkdirDiffMode,
    };
    use standout::cli::{CommandContext, HandlerResult, Output};

    /// Handler for count command.
    ///
    /// Returns the canonical [`CountQuerySet`] regardless of output mode.
    pub fn count(matches: &ArgMatches, _ctx: &CommandContext) -> HandlerResult<CountQuerySet> {
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

        let path_ref = std::path::Path::new(path);
        let is_workspace = path_ref.is_dir() && path_ref.join("Cargo.toml").exists()
            || path_ref.is_file() && path_ref.file_name() == Some("Cargo.toml".as_ref());

        if !is_workspace && matches!(aggregation, Aggregation::ByCrate) {
            return Err(anyhow::anyhow!(
                "{} requires a Cargo workspace (directory with Cargo.toml), but '{}' is not a workspace",
                "--by-crate",
                path,
            ));
        }

        // The canonical response always carries complete counts; `line_types`
        // only describes the requested view and is applied when rendering.
        let result: CountResult = if is_workspace {
            let options = CountOptions::new()
                .crates(crates)
                .filter(filter)
                .aggregation(aggregation)
                .line_types(LineTypes::everything());
            count_workspace(path, options)?
        } else if path_ref.is_file() {
            let stats = count_file_with_filter(path, &filter)?;
            let mut r = CountResult::new();
            r.root = path_ref.to_path_buf();
            r.file_count = 1;
            r.total = stats;
            r
        } else {
            count_directory_with_options(
                path,
                CountOptions::new()
                    .filter(filter)
                    .aggregation(aggregation)
                    .line_types(LineTypes::everything()),
            )?
        };

        let top = extract_top(matches);
        let preds = super::filter_args::extract(matches);
        // Apply filter first, then top, so `--top` slices the already-
        // filtered set rather than slicing first and dropping rows that
        // happened to be in the top-N but failed the predicate.
        let apply_post = |qs: CountQuerySet| {
            let qs = qs.filter(&preds);
            match top {
                Some(n) => qs.top(n),
                None => qs,
            }
        };

        Ok(Output::Render(apply_post(CountQuerySet::from_result(
            &result,
            aggregation,
            line_types,
            ordering,
        ))))
    }

    /// Handler for diff command.
    ///
    /// Returns the canonical [`DiffQuerySet`] regardless of output mode.
    pub fn diff(matches: &ArgMatches, _ctx: &CommandContext) -> HandlerResult<DiffQuerySet> {
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

        // The canonical response always carries complete counts; `line_types`
        // only describes the requested view and is applied when rendering.
        let options = DiffOptions::new()
            .crates(crates)
            .filter(filter)
            .aggregation(aggregation)
            .line_types(LineTypes::everything());

        let from = matches.get_one::<String>("from");
        let to = matches.get_one::<String>("to");

        let result = if let Some(from_str) = from {
            // Commit diff: pass the revspec straight through to the library,
            // which delegates parsing to gix's rev_parse. Two positional args
            // (e.g. `rustloc diff main feature`) are joined as `<a>..<b>` so
            // gix sees a single range expression.
            if staged {
                return Err(anyhow::anyhow!(
                    "--staged/--cached can only be used without commit arguments"
                ));
            }
            let revspec = match to {
                Some(to_str) => {
                    if from_str.contains("..") {
                        return Err(anyhow::anyhow!(
                            "Pass either a single range/revspec (e.g. `a..b`) \
                             or two revs as separate args (e.g. `a b`), not both."
                        ));
                    }
                    format!("{}..{}", from_str, to_str)
                }
                None => from_str.clone(),
            };
            diff_revspec(path, &revspec, options)?
        } else {
            // Working directory diff
            let mode = if staged {
                WorkdirDiffMode::Staged
            } else {
                WorkdirDiffMode::All
            };
            diff_workdir(path, mode, options)?
        };

        let top = extract_top(matches);
        let preds = super::filter_args::extract(matches);
        let apply_post = |qs: DiffQuerySet| {
            let qs = qs.filter(&preds);
            match top {
                Some(n) => qs.top(n),
                None => qs,
            }
        };

        Ok(Output::Render(apply_post(DiffQuerySet::from_result(
            &result,
            aggregation,
            line_types,
            ordering,
        ))))
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

    fn extract_top(matches: &ArgMatches) -> Option<usize> {
        matches.get_one::<usize>("top").copied()
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
        let mut filter = FilterConfig::new().languages(extract_languages(matches)?);

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

    fn extract_languages(matches: &ArgMatches) -> Result<LanguageSelection, anyhow::Error> {
        let values: Vec<&str> = matches
            .get_many::<String>("languages")
            .map(|v| v.map(|s| s.as_str()).collect())
            .unwrap_or_default();

        if values.is_empty() {
            return Ok(LanguageSelection::new(default_languages()));
        }

        if values.iter().any(|value| value.eq_ignore_ascii_case("all")) {
            return Ok(LanguageSelection::new(available_languages()));
        }

        let mut languages = Vec::new();
        for value in values {
            languages.push(value.parse::<LanguageName>().map_err(anyhow::Error::msg)?);
        }
        Ok(LanguageSelection::new(&languages))
    }

    fn extract_crates(matches: &ArgMatches) -> Vec<String> {
        matches
            .get_many::<String>("crates")
            .map(|v| v.cloned().collect())
            .unwrap_or_default()
    }
}

/// Presentation adapters — the render boundary.
///
/// Handlers return one canonical response per command, independent of output
/// mode. Something still has to decide how that response reaches the user, and
/// that decision lives here, in post-dispatch hooks that run *after* the
/// handler and *before* rendering. This is the one place in the application
/// that is allowed to know the output mode.
///
/// Three targets:
///
/// - **Data** (`json`/`yaml`/`xml`) — the canonical response is passed through
///   untouched and standout serializes it directly. Numbers stay numbers.
/// - **Csv** — standout's CSV writer flattens whatever object it is handed
///   into a *single* row, which would turn a query set into hundreds of
///   `items.0.code`-style columns. So we adapt the response into a top-level
///   array of flat, typed row structs (see [`CountCsvRow`] / [`DiffCsvRow`]),
///   which flattens to the row-per-item CSV users expect. This is the isolated
///   workaround for that render-layer limitation — it is deliberately *not* a
///   branch in the handler.
/// - **Table** (everything else: `auto`/`term`/`text`/`term-debug`) — the
///   response is formatted into a `LOCTable` for the `stats_table` template.
///   `line_types` picks the columns here, at render time.
mod presentation {
    use crate::table::LOCTable;
    use clap::ArgMatches;
    use rustloclib::{CountQuerySet, DiffQuerySet, Locs, LocsDiff};
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use standout::cli::{CommandContext, HookError};

    /// How the canonical response should reach the user.
    enum Target {
        /// Serialize the response as-is (json/yaml/xml).
        Data,
        /// Adapt to row-per-item CSV.
        Csv,
        /// Format into a `LOCTable` for the template.
        Table,
    }

    /// Read the render target from standout's injected `_output_mode` arg.
    ///
    /// Reading the output mode is legitimate *here* — this is the render
    /// boundary, not command logic.
    fn target(matches: &ArgMatches) -> Target {
        match matches
            .get_one::<String>("_output_mode")
            .map(|s| s.as_str())
        {
            Some("csv") => Target::Csv,
            Some("json" | "yaml" | "xml") => Target::Data,
            _ => Target::Table,
        }
    }

    fn decode<T: for<'de> Deserialize<'de>>(data: Value) -> Result<T, HookError> {
        serde_json::from_value(data).map_err(|e| {
            HookError::post_dispatch(format!("canonical response was not well-formed: {e}"))
        })
    }

    fn encode<T: Serialize>(value: T) -> Result<Value, HookError> {
        serde_json::to_value(value)
            .map_err(|e| HookError::post_dispatch(format!("failed to encode presentation: {e}")))
    }

    /// One CSV row of the count schema.
    ///
    /// The schema is **stable and mode-independent**: every line-type column is
    /// always present with its real count, regardless of `--type`. `--type` is
    /// a display filter for the human table; narrowing CSV columns to match it
    /// would make the schema vary per invocation, which is exactly what a
    /// machine-readable format must not do.
    #[derive(Serialize)]
    struct CountCsvRow {
        label: String,
        code: u64,
        tests: u64,
        examples: u64,
        docs: u64,
        comments: u64,
        blanks: u64,
        total: u64,
    }

    impl CountCsvRow {
        fn new(label: impl Into<String>, stats: &Locs) -> Self {
            Self {
                label: label.into(),
                code: stats.code,
                tests: stats.tests,
                examples: stats.examples,
                docs: stats.docs,
                comments: stats.comments,
                blanks: stats.blanks,
                total: stats.total,
            }
        }
    }

    /// One CSV row of the diff schema: label plus added/removed/net columns for
    /// every line type. Same stability contract as [`CountCsvRow`].
    ///
    /// `net_*` is `i64` so a net removal reads `-42` rather than underflowing
    /// into a very large positive number.
    #[derive(Serialize)]
    struct DiffCsvRow {
        label: String,
        added_code: u64,
        added_tests: u64,
        added_examples: u64,
        added_docs: u64,
        added_comments: u64,
        added_blanks: u64,
        added_total: u64,
        removed_code: u64,
        removed_tests: u64,
        removed_examples: u64,
        removed_docs: u64,
        removed_comments: u64,
        removed_blanks: u64,
        removed_total: u64,
        net_code: i64,
        net_tests: i64,
        net_examples: i64,
        net_docs: i64,
        net_comments: i64,
        net_blanks: i64,
        net_total: i64,
    }

    impl DiffCsvRow {
        fn new(label: impl Into<String>, d: &LocsDiff) -> Self {
            Self {
                label: label.into(),
                added_code: d.added.code,
                added_tests: d.added.tests,
                added_examples: d.added.examples,
                added_docs: d.added.docs,
                added_comments: d.added.comments,
                added_blanks: d.added.blanks,
                added_total: d.added.total,
                removed_code: d.removed.code,
                removed_tests: d.removed.tests,
                removed_examples: d.removed.examples,
                removed_docs: d.removed.docs,
                removed_comments: d.removed.comments,
                removed_blanks: d.removed.blanks,
                removed_total: d.removed.total,
                net_code: d.net_code(),
                net_tests: d.net_tests(),
                net_examples: d.net_examples(),
                net_docs: d.net_docs(),
                net_comments: d.net_comments(),
                net_blanks: d.net_blanks(),
                net_total: d.net_total(),
            }
        }
    }

    /// One row per item, then a `TOTAL` summary row.
    fn count_csv_rows(qs: &CountQuerySet) -> Vec<CountCsvRow> {
        let mut rows: Vec<CountCsvRow> = qs
            .items
            .iter()
            .map(|item| CountCsvRow::new(item.label.clone(), &item.stats))
            .collect();
        rows.push(CountCsvRow::new("TOTAL", &qs.total));
        rows
    }

    /// One row per item, an optional `SKIPPED` row, then a `TOTAL` row.
    fn diff_csv_rows(qs: &DiffQuerySet) -> Vec<DiffCsvRow> {
        let mut rows: Vec<DiffCsvRow> = qs
            .items
            .iter()
            .map(|item| DiffCsvRow::new(item.label.clone(), &item.stats))
            .collect();

        // Preserve the skipped-file summary that the text footer and JSON
        // output expose. We have no per-line-type breakdown for skipped
        // files, so only the *_total fields carry data; everything else
        // is left at zero. Skip the row entirely when there's nothing to
        // show, so a fully analyzed diff stays clean.
        if qs.non_rust_added > 0 || qs.non_rust_removed > 0 {
            let non_rust = LocsDiff {
                added: Locs {
                    total: qs.non_rust_added,
                    ..Locs::default()
                },
                removed: Locs {
                    total: qs.non_rust_removed,
                    ..Locs::default()
                },
            };
            rows.push(DiffCsvRow::new("SKIPPED", &non_rust));
        }

        rows.push(DiffCsvRow::new("TOTAL", &qs.total));
        rows
    }

    /// Post-dispatch adapter for `count`.
    pub fn count(
        matches: &ArgMatches,
        _ctx: &CommandContext,
        data: Value,
    ) -> Result<Value, HookError> {
        match target(matches) {
            Target::Data => Ok(data),
            Target::Csv => encode(count_csv_rows(&decode::<CountQuerySet>(data)?)),
            Target::Table => encode(LOCTable::from_count_queryset(&decode::<CountQuerySet>(
                data,
            )?)),
        }
    }

    /// Post-dispatch adapter for `diff`.
    pub fn diff(
        matches: &ArgMatches,
        _ctx: &CommandContext,
        data: Value,
    ) -> Result<Value, HookError> {
        match target(matches) {
            Target::Data => Ok(data),
            Target::Csv => encode(diff_csv_rows(&decode::<DiffQuerySet>(data)?)),
            Target::Table => encode(LOCTable::from_diff_queryset(&decode::<DiffQuerySet>(data)?)),
        }
    }
}

/// Filter-flag generation.
///
/// We support a `--<field>-<op> <N>` grid: 7 fields × 6 ops = 42 hidden args.
/// Listing each individually would clutter `--help`, so we hide them and
/// document the synthetic pattern via `after_long_help`. clap still parses
/// them natively, which gives us tab-completion-friendly errors and bypasses
/// any custom mini-grammar.
mod filter_args {
    use clap::{value_parser, Arg, ArgAction, ArgMatches, Command};
    use rustloclib::{Field, Op, Predicate};
    use std::sync::OnceLock;

    /// (field, op, leaked-static-name) for each of the 42 (field × op) pairs.
    ///
    /// Computed exactly once and cached. Each name is leaked as a
    /// `&'static str` because clap's `Arg::new` / `Arg::long` want
    /// `IntoResettable<Str>` which is implemented for `&'static str` but
    /// not for `String`. Caching avoids re-leaking on repeated calls
    /// (`make_args` is invoked once per injection point: top-level + count
    /// + diff = three calls), keeping the leak count to exactly 42.
    fn flag_table() -> &'static [(Field, Op, &'static str)] {
        static TABLE: OnceLock<Vec<(Field, Op, &'static str)>> = OnceLock::new();
        TABLE.get_or_init(|| {
            let mut v = Vec::with_capacity(Field::all().len() * Op::all().len());
            for &field in Field::all() {
                for &op in Op::all() {
                    let s: &'static str =
                        Box::leak(format!("{}-{}", field.name(), op.name()).into_boxed_str());
                    v.push((field, op, s));
                }
            }
            v
        })
    }

    /// Generate one clap arg per (field, op) pair, all hidden from `--help`.
    pub fn make_args() -> Vec<Arg> {
        flag_table()
            .iter()
            .map(|&(_, _, name)| {
                Arg::new(name)
                    .long(name)
                    .value_name("N")
                    .value_parser(value_parser!(u64))
                    .action(ArgAction::Append)
                    .hide(true)
            })
            .collect()
    }

    /// Synthetic doc block describing the filter pattern. Rendered in
    /// `--help` via `after_long_help` so users see one block instead of 42
    /// individual flag lines.
    pub const SYNTHETIC_DOC: &str = "Filter options (combine with AND):\n  \
         --<category>-<op> <N>\n  \
         Categories: code, tests, examples, docs, comments, blanks, total\n  \
         Operators:  gt, gte, eq, ne, lt, lte\n\
         \n\
         Examples:\n  \
         rustloc --by-file --code-gte 1000\n  \
         rustloc --by-file --code-gte 1000 --tests-lt 500 --top 10";

    /// `Command::after_long_help` is a setter — it replaces any previous
    /// value. The Cli derive (and the diff subcommand) already define an
    /// examples block via `#[command(after_long_help = "...")]`, so we
    /// concatenate rather than overwrite.
    fn append_long_help(cmd: Command, addition: &'static str) -> Command {
        let combined: &'static str = match cmd.get_after_long_help() {
            Some(existing) if !existing.to_string().is_empty() => {
                let merged = format!("{}\n\n{}", existing, addition);
                Box::leak(merged.into_boxed_str())
            }
            _ => addition,
        };
        cmd.after_long_help(combined)
    }

    /// Inject the filter args + synthetic doc onto the top-level Cli, the
    /// `count` subcommand, and the `diff` subcommand.
    ///
    /// All three are required:
    /// - top-level: for the bare-call form (`rustloc --code-gte 100 .`)
    ///   where clap routes through the default subcommand machinery via
    ///   the flattened `CountArgs`.
    /// - count subcommand: for the explicit form (`rustloc count ...`).
    /// - diff subcommand: for the diff path.
    ///
    /// Adding to fewer than all three breaks one or more of the call shapes.
    pub fn inject(cmd: Command) -> Command {
        fn augment(sub: Command) -> Command {
            let mut sub = sub;
            for a in make_args() {
                sub = sub.arg(a);
            }
            append_long_help(sub, SYNTHETIC_DOC)
        }

        let mut cmd = cmd;
        for a in make_args() {
            cmd = cmd.arg(a);
        }
        append_long_help(cmd, SYNTHETIC_DOC)
            .mut_subcommand("count", augment)
            .mut_subcommand("diff", augment)
    }

    /// Read all filter args back from `ArgMatches` into a flat
    /// AND-combined predicate list.
    pub fn extract(matches: &ArgMatches) -> Vec<Predicate> {
        let mut out = Vec::new();
        for &(field, op, name) in flag_table() {
            if let Some(values) = matches.get_many::<u64>(name) {
                for &v in values {
                    out.push(Predicate::new(field, op, v));
                }
            }
        }
        out
    }
}

fn run() -> Result<RunResult, anyhow::Error> {
    // Load theme: start with standout defaults (includes table_row_even/odd),
    // then merge our custom stylesheet on top
    use standout::{StylesheetRegistry, Theme};
    let mut registry: StylesheetRegistry = embed_styles!("styles").into();
    let custom_theme = registry.get("default")?;
    let theme = Theme::default().merge(custom_theme);

    // Build the standout app with derive-based dispatch
    let app = App::builder()
        .templates(embed_templates!("templates"))
        .theme(theme)
        .commands(Commands::dispatch_config())?
        .build()?;

    let cli_cmd = filter_args::inject(Cli::command());
    Ok(app.run_to_string(cli_cmd, std::env::args()))
}

fn main() -> ExitCode {
    match run() {
        Ok(result) => match result {
            RunResult::Handled(output) => {
                if !output.is_empty() {
                    print!("{}", output);
                    if !output.ends_with('\n') {
                        println!();
                    }
                }
                ExitCode::SUCCESS
            }
            // standout-dispatch ≥ 7.6.0 routes clap parse errors (and any
            // handler/hook errors that called .use_stderr()) here instead
            // of stuffing them into Handled. Without this arm, a typo like
            // `--total-fsdgte 1300` is silently swallowed: no output, no
            // error, exit 0. clap's own message is already in `msg`, so
            // just write it to stderr and exit with the standard usage-error
            // code (2) so scripts notice.
            RunResult::Error(msg) => {
                eprint!("{}", msg);
                if !msg.ends_with('\n') {
                    eprintln!();
                }
                ExitCode::from(2)
            }
            RunResult::Binary(_, _) => ExitCode::SUCCESS,
            RunResult::Silent => ExitCode::SUCCESS,
            RunResult::NoMatch(_) => {
                // Should not happen with default command set
                eprintln!("Error: Unknown command");
                ExitCode::FAILURE
            }
            // RunResult is #[non_exhaustive]; future variants we don't
            // know about should be treated as failures rather than silently
            // ignored — better to surface the gap than hide it.
            _ => {
                eprintln!("Error: unhandled result from command dispatch");
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::FAILURE
        }
    }
}
