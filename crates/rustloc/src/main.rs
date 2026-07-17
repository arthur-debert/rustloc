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

use clap::{Args, Parser, Subcommand};
use rustloclib::Ordering;
use standout::cli::{Dispatch, RunResult};

mod app;
mod application;
mod command;
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
    #[dispatch(default, template = "count_table", post_dispatch = presentation::count)]
    Count(CountArgs),

    /// Show LOC differences between git commits
    #[dispatch(template = "diff_table", post_dispatch = presentation::diff)]
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
    // `allow_hyphen_values` keeps `-o -code` from being read as a flag;
    // `value_parser` makes an unknown field a clap usage error at parse time
    // rather than a silent fall back to the default ordering.
    #[arg(
        short = 'o',
        long = "ordering",
        value_name = "FIELD",
        allow_hyphen_values = true,
        value_parser = command::parse_ordering
    )]
    #[arg(long_help = "\
Sort by field. Prefix with - for descending, + for ascending.

Fields: label, code, tests, examples, docs, comments, blanks, total
Default direction: descending for numeric fields, ascending for label.

  -o code         Sort by code lines (descending)
  -o -code        Sort by code lines (descending, explicit)
  -o +code        Sort by code lines (ascending)
  -o label        Sort by name (ascending)")]
    ordering: Option<Ordering>,

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
    // `allow_hyphen_values` keeps `-o -code` from being read as a flag;
    // `value_parser` makes an unknown field a clap usage error at parse time
    // rather than a silent fall back to the default ordering.
    #[arg(
        short = 'o',
        long = "ordering",
        value_name = "FIELD",
        allow_hyphen_values = true,
        value_parser = command::parse_ordering
    )]
    #[arg(long_help = "\
Sort by field. Prefix with - for descending, + for ascending.

Fields: label, code, tests, examples, docs, comments, blanks, total
Default direction: descending for numeric fields, ascending for label.

  -o code         Sort by code lines (descending)
  -o -code        Sort by code lines (descending, explicit)
  -o +code        Sort by code lines (ascending)
  -o label        Sort by name (ascending)")]
    ordering: Option<Ordering>,

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

/// Command handlers — the dispatch boundary.
///
/// Handlers are deliberately **thin**: each converts `ArgMatches` into a typed
/// request ([`crate::command`]), hands it to typed orchestration
/// ([`crate::application`]), and wraps the canonical response in
/// [`Output::Render`]. Nothing else. Every decision worth testing — parsing,
/// validation, library selection, query narrowing — sits on one side of this
/// seam or the other, reachable without building an `ArgMatches`.
///
/// Handlers are also **pure with respect to presentation**: they return the one
/// canonical response for their command ([`CountQuerySet`] / [`DiffQuerySet`])
/// and never inspect the output mode. How that response is presented — human
/// table, CSV rows, or direct serialization — belongs to
/// [`super::presentation`], which runs at the render boundary.
///
/// Standout's `#[handler]` macro would normally generate this bridge from typed
/// parameters, but it maps one parameter per named clap arg, and the count/diff
/// grammar includes the 42 dynamically registered `--<field>-<op>` filter flags
/// (see [`super::filter_args`]) that no fixed parameter list can express. Its
/// `#[matches]` escape hatch would hand the raw matches back to the handler
/// anyway, so we keep the plain dispatch signature and put the typed seam in
/// `command` + `application` instead — the same separation, minus a macro that
/// cannot cover the grammar.
mod handlers {
    use crate::application;
    use crate::command::{CountRequest, DiffRequest};
    use clap::ArgMatches;
    use rustloclib::{CountQuerySet, DiffQuerySet};
    use standout::cli::{CommandContext, HandlerResult, Output};

    /// Handler for the count command (also the default command).
    pub fn count(matches: &ArgMatches, _ctx: &CommandContext) -> HandlerResult<CountQuerySet> {
        let request = CountRequest::from_matches(matches)?;
        Ok(Output::Render(application::count(&request)?))
    }

    /// Handler for the diff command.
    pub fn diff(matches: &ArgMatches, _ctx: &CommandContext) -> HandlerResult<DiffQuerySet> {
        let request = DiffRequest::from_matches(matches)?;
        Ok(Output::Render(application::diff(&request)?))
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
///   response becomes a [`CountView`] / [`DiffView`] for the `count_table` /
///   `diff_table` templates. `line_types` picks the columns here, at render
///   time. The view carries typed numbers, not display strings: the template
///   owns every word, width, and style tag a reader sees (see [`crate::table`]).
mod presentation {
    use crate::table::{CountView, DiffView};
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
        /// Narrow to a `CountView`/`DiffView` for the template.
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
            Target::Table => encode(CountView::from_queryset(&decode::<CountQuerySet>(data)?)),
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
            Target::Table => encode(DiffView::from_queryset(&decode::<DiffQuerySet>(data)?)),
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

/// Read the process environment and run the app built by [`crate::app`].
///
/// The construction itself lives in `app` so tests can build the same app;
/// what stays here is the one thing a test must not inherit — `std::env::args`.
fn run() -> Result<RunResult, anyhow::Error> {
    Ok(app::app()?.run_to_string(app::cli_command(), std::env::args()))
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

#[cfg(test)]
mod pipeline_tests;
