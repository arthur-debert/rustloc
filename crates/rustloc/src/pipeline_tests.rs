//! In-process pipeline tests: argv → clap → handler → post-dispatch → render.
//!
//! This is the middle layer of the pyramid. Below it, [`crate::command`] and
//! [`crate::application`] test parsing and orchestration as plain functions.
//! Above it, `tests/cli_integration.rs` spawns the real executable for the
//! boundaries a process owns (exit codes, stderr, packaging).
//!
//! These tests drive the **same app `main` runs** — [`crate::app::app`] and
//! [`crate::app::cli_command`] — through Standout's `run_to_string`, which is
//! the entry point `main` itself calls. So everything between argv and the
//! rendered string is real: the injected filter grid, default-command routing,
//! post-dispatch presentation, the template, and every serializer.
//!
//! ## Why not `standout-test::TestHarness`
//!
//! The workstream called for `TestHarness`, and it is currently unusable here.
//! `standout-test` is published only up to **7.5.1**, which does not compile
//! against the **7.6.2** standout this CLI requires: 7.6.0 added the
//! `RunResult::Error` variant to a `#[non_exhaustive]` enum, and 7.5.1's
//! matches have no wildcard arm. Upstream's 7.6.x `standout-test` handles the
//! variant but is marked `publish = false`, so it is not on crates.io; taking
//! it as a git dependency pulls a *second* standout into the graph, and the
//! harness then cannot accept an `App` built from the registry standout
//! ("expected `&AppBuilder`, found `&App`"). Downgrading standout to 7.5.x is
//! not an option — `main` depends on `RunResult::Error` to give clap usage
//! errors a nonzero exit, the exact regression tested in `cli_integration.rs`.
//!
//! `run_to_string` is what the harness wraps, so the pipeline coverage below is
//! equivalent for everything argv can express — which is all of this CLI's
//! behavior. What the harness would add on top is control of *ambient* seams
//! (TTY, terminal width, colour capability, cwd, stdin). Those stay uncovered;
//! see the PR's Context note. When a 7.6.x `standout-test` is published, these
//! tests port to it directly, and only then do they need `#[serial]`: the
//! harness mutates process-global detectors, whereas the argv-driven runs below
//! touch no global state and are safe to run in parallel.

use rustloclib::{CountQuerySet, DiffQuerySet};
use standout::cli::RunResult;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Run the real app over `args`, with `rustloc` pushed on as argv[0].
///
/// Fixture paths are always passed absolute, so no test changes the process
/// working directory and the whole module stays parallel-safe.
fn run(args: &[&str]) -> RunResult {
    let app = crate::app::app().expect("app must build from embedded assets");
    let argv: Vec<String> = std::iter::once("rustloc".to_string())
        .chain(args.iter().map(|s| s.to_string()))
        .collect();
    app.run_to_string(crate::app::cli_command(), argv)
}

/// The rendered/serialized stdout of a successful run.
#[track_caller]
fn stdout(args: &[&str]) -> String {
    match run(args) {
        RunResult::Handled(s) => s,
        other => panic!("expected Handled for {args:?}, got {other:?}"),
    }
}

/// The message of a run that failed at the parsing boundary.
#[track_caller]
fn error(args: &[&str]) -> String {
    match run(args) {
        RunResult::Error(msg) => msg,
        other => panic!("expected Error for {args:?}, got {other:?}"),
    }
}

/// A two-file crate: `src/lib.rs` (3 code lines) and `src/small.rs` (1).
fn workspace() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn a() {}\npub fn b() {}\npub fn c() {}\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/small.rs"), "pub fn d() {}\n").unwrap();
    dir
}

fn path_of(dir: &TempDir) -> String {
    dir.path().to_str().unwrap().to_string()
}

// ---------------------------------------------------------------------------
// Routing: the default command and the explicit subcommands
// ---------------------------------------------------------------------------

/// A bare path routes through the default `count` command. This is the shape
/// most users type, and it is the one the filter grid has to be injected at
/// the top level for.
#[test]
fn bare_path_routes_to_the_default_count_command() {
    let dir = workspace();
    let out = stdout(&[&path_of(&dir), "--output", "json"]);
    let parsed: CountQuerySet = serde_json::from_str(&out).expect("count response");
    assert_eq!(parsed.total.code, 4);
}

/// The explicit `count` subcommand must produce the identical response — the
/// two spellings are the same command, not two code paths that can drift.
#[test]
fn explicit_count_subcommand_matches_the_default_route() {
    let dir = workspace();
    let path = path_of(&dir);
    assert_eq!(
        stdout(&[&path, "--output", "json"]),
        stdout(&["count", &path, "--output", "json"]),
    );
}

// ---------------------------------------------------------------------------
// Structured output modes
// ---------------------------------------------------------------------------

/// Every structured mode serializes the one canonical response, so the numbers
/// must agree across all three. A presentation branch leaking into a handler
/// would show up here as a mode-dependent count.
///
/// Both modes deserialize back into [`CountQuerySet`] rather than into a loose
/// `Value`: the round trip is what pins the schema, so a renamed or dropped
/// field fails here instead of silently comparing two equally-wrong trees. It
/// also sidesteps the number-representation quirks that make a raw YAML `Value`
/// unequal to a JSON one.
#[test]
fn structured_modes_carry_the_same_canonical_numbers() {
    let dir = workspace();
    let path = path_of(&dir);

    let json: CountQuerySet =
        serde_json::from_str(&stdout(&[&path, "--output", "json"])).expect("json must parse");
    let yaml: CountQuerySet =
        serde_yaml::from_str(&stdout(&[&path, "--output", "yaml"])).expect("yaml must parse");

    assert_eq!(json.total.code, 4);
    assert_eq!(
        json, yaml,
        "json and yaml must serialize the same canonical response"
    );
}

/// JSON keeps numbers as numbers — the reason the data modes bypass the table
/// adapter entirely.
#[test]
fn json_numbers_stay_numbers() {
    let dir = workspace();
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&[&path_of(&dir), "--output", "json"])).unwrap();

    for field in [
        "code", "tests", "docs", "blanks", "comments", "examples", "total",
    ] {
        assert!(
            json["total"][field].is_u64(),
            "{field} should be a number, got {:?}",
            json["total"][field]
        );
    }
}

/// Read a CSV column by *name*. Standout's CSV writer emits columns in
/// alphabetical order, so `label` is not column 0 — indexing positionally
/// would pin an incidental ordering rather than the schema.
fn csv_column(out: &str, column: &str) -> Vec<String> {
    let mut rdr = csv::Reader::from_reader(out.as_bytes());
    let headers: Vec<String> = rdr.headers().unwrap().iter().map(String::from).collect();
    let idx = headers
        .iter()
        .position(|h| h == column)
        .unwrap_or_else(|| panic!("no {column:?} column in {headers:?}"));
    rdr.records().map(|r| r.unwrap()[idx].to_string()).collect()
}

/// CSV is the one mode with a presentation adapter: the query set becomes a
/// top-level array so it flattens to one row per item plus a TOTAL row, rather
/// than a single row of `items.0.code`-style columns.
#[test]
fn csv_renders_one_row_per_file_plus_a_total() {
    let dir = workspace();
    let out = stdout(&[&path_of(&dir), "--by-file", "--output", "csv"]);

    let labels = csv_column(&out, "label");
    assert_eq!(labels.len(), 3, "two files + TOTAL, got {labels:?}");
    assert_eq!(labels.last().unwrap(), "TOTAL");
    assert!(
        labels.iter().any(|l| l.contains("lib.rs")),
        "expected a row per file, got {labels:?}"
    );
}

/// Every column a count CSV must expose. Pinned as a full set so a regression
/// that drops, say, `examples` fails here rather than slipping through a
/// looser "some headers exist" check.
const COUNT_CSV_HEADERS: &[&str] = &[
    "label", "code", "tests", "examples", "docs", "comments", "blanks", "total",
];

fn csv_headers(out: &str) -> Vec<String> {
    csv::Reader::from_reader(out.as_bytes())
        .headers()
        .unwrap()
        .iter()
        .map(String::from)
        .collect()
}

/// The CSV schema is machine-readable, so it must not vary per invocation:
/// `--type` narrows the human table, never the columns.
#[test]
fn csv_schema_is_stable_regardless_of_type_flag() {
    let dir = workspace();
    let path = path_of(&dir);

    let plain = csv_headers(&stdout(&[&path, "--output", "csv"]));
    let narrowed = csv_headers(&stdout(&[&path, "--output", "csv", "--type", "code"]));

    for col in COUNT_CSV_HEADERS {
        assert!(
            plain.iter().any(|h| h == col),
            "missing required CSV column `{col}`; headers: {plain:?}"
        );
    }
    assert_eq!(
        plain, narrowed,
        "--type must not change the machine-readable schema"
    );
}

// ---------------------------------------------------------------------------
// Text rendering and the template
// ---------------------------------------------------------------------------

/// Text mode goes through the `stats_table` template. Asserting the headers
/// and the total row proves the template rendered the LOCTable rather than
/// erroring into an empty string.
#[test]
fn text_mode_renders_the_stats_table_template() {
    let dir = workspace();
    let out = stdout(&[&path_of(&dir), "--output", "text"]);

    for header in ["Code", "Tests", "Docs", "Total"] {
        assert!(out.contains(header), "missing {header} column in:\n{out}");
    }
    assert!(
        out.contains("Total (") && out.contains("files)"),
        "missing total row in:\n{out}"
    );
}

/// The template's line structure has regressed before (a stray Jinja trim
/// marker ate the newline after the header rule, shipping in v0.17.1). A
/// separator line must contain nothing but `─`.
#[test]
fn template_keeps_separators_on_their_own_lines() {
    let dir = workspace();
    let out = stdout(&[&path_of(&dir), "--by-file", "--output", "text"]);

    let separators = out.lines().filter(|l| l.contains('─')).collect::<Vec<_>>();
    assert!(!separators.is_empty(), "expected rules in:\n{out}");
    for line in separators {
        assert!(
            line.trim().chars().all(|c| c == '─'),
            "separator line carried content: {line:?}\n{out}"
        );
    }
}

/// `--type` narrows the *table* columns — the display-only half of the
/// contract whose other half `csv_schema_is_stable_regardless_of_type_flag`
/// pins.
#[test]
fn type_flag_narrows_the_table_columns() {
    let dir = workspace();
    let out = stdout(&[&path_of(&dir), "--output", "text", "--type", "code"]);

    assert!(out.contains("Code"), "expected Code column in:\n{out}");
    assert!(!out.contains("Docs"), "Docs should be hidden in:\n{out}");
}

/// term-debug mode exposes the style tags, which is how a theme regression
/// becomes visible without a TTY.
#[test]
fn term_debug_mode_exposes_style_tags() {
    let dir = workspace();
    let out = stdout(&[&path_of(&dir), "--output", "term-debug"]);
    assert!(
        out.contains('[') && out.contains(']'),
        "expected style tags in:\n{out}"
    );
}

// ---------------------------------------------------------------------------
// The filter grid, through the real injected args
// ---------------------------------------------------------------------------

/// The synthetic `--<field>-<op>` flags only exist because `filter_args`
/// injected them onto this exact command — which is why the factory builds the
/// command rather than tests reaching for a bare `Cli::command()`.
#[test]
fn filter_grid_is_injected_on_the_default_route() {
    let dir = workspace();
    let out = stdout(&[
        &path_of(&dir),
        "--by-file",
        "--code-gte",
        "3",
        "--output",
        "json",
    ]);
    let parsed: CountQuerySet = serde_json::from_str(&out).unwrap();

    assert_eq!(parsed.items.len(), 1, "only lib.rs clears --code-gte 3");
    assert!(parsed.items[0].label.contains("lib.rs"));
}

/// ...and on the explicit subcommand too. Injecting at only one site silently
/// breaks the other call shape.
#[test]
fn filter_grid_is_injected_on_the_count_subcommand() {
    let dir = workspace();
    let out = stdout(&[
        "count",
        &path_of(&dir),
        "--by-file",
        "--code-gte",
        "3",
        "--output",
        "json",
    ]);
    assert_eq!(
        serde_json::from_str::<CountQuerySet>(&out)
            .unwrap()
            .items
            .len(),
        1
    );
}

/// The hidden flags stay hidden, but the synthetic doc block must reach
/// `--help` — otherwise the grid is undiscoverable.
///
/// Help is a *successful* dispatch (`Handled`), not an error: clap renders it
/// and standout hands back the text.
#[test]
fn help_hides_the_grid_flags_but_documents_the_pattern() {
    let help = stdout(&["--help"]);

    // Match the `--code-gte <N>` *listing* form, not a bare mention: the
    // synthetic doc's own examples name these flags on purpose, so asserting
    // on the bare name would fail against the very block we want present.
    for listed in ["--code-gte <", "--tests-lt <", "--total-eq <"] {
        assert!(
            !help.contains(listed),
            "{listed:?} should stay hidden from the options list:\n{help}"
        );
    }

    assert!(help.contains("Filter options"));
    assert!(
        help.contains("--<category>-<op>"),
        "synthetic doc block missing from:\n{help}"
    );
}

/// The grid injection appends to `after_long_help` rather than replacing it,
/// so the derive's own examples block has to survive alongside it.
#[test]
fn long_help_keeps_both_the_examples_and_the_grid_doc() {
    let help = stdout(&["--help"]);
    assert!(
        help.contains("rustloc --by-crate") && help.contains("--<category>-<op>"),
        "expected both blocks in:\n{help}"
    );
}

// ---------------------------------------------------------------------------
// Errors at the parsing boundary
// ---------------------------------------------------------------------------

/// An unknown ordering field is a usage error, not a silent fall back to the
/// default. The regression: `-o -coed` used to sort by label and exit 0.
///
/// The *exit code* for this is a process fact and stays in `cli_integration`;
/// what belongs here is that dispatch produces an Error naming the field.
#[test]
fn invalid_ordering_is_a_usage_error_on_every_route() {
    let dir = workspace();
    let path = path_of(&dir);

    for args in [
        vec![path.as_str(), "-o", "coed"],
        vec!["count", path.as_str(), "-o", "coed"],
        vec!["diff", "-o", "coed"],
    ] {
        let msg = error(&args);
        assert!(
            msg.contains("invalid value 'coed'") && msg.contains("Unknown order field: coed"),
            "{args:?} should name the bad value, got: {msg}"
        );
    }
}

/// The valid forms must keep parsing — strictness that also rejects good input
/// is just a different bug.
#[test]
fn valid_ordering_forms_still_parse() {
    let dir = workspace();
    let path = path_of(&dir);

    for good in ["code", "-code", "+code", "label", "total", "name", "test"] {
        let result = run(&[&path, "--by-file", "-o", good]);
        assert!(
            matches!(result, RunResult::Handled(_)),
            "`-o {good}` should succeed, got {result:?}"
        );
    }
}

/// A typo'd grid flag must be rejected rather than ignored: `--total-fsdgte`
/// is not a registered arg, and silently dropping it would return unfiltered
/// output for a request the tool never honoured.
#[test]
fn unknown_grid_flag_is_rejected() {
    let dir = workspace();
    let msg = error(&[&path_of(&dir), "--total-fsdgte", "1300"]);
    assert!(
        msg.contains("unexpected argument") || msg.contains("--total-fsdgte"),
        "expected the bad flag to be named, got: {msg}"
    );
}

/// The grid's values are typed `u64`, so a non-numeric value is a parse error.
#[test]
fn non_numeric_grid_value_is_rejected() {
    let dir = workspace();
    let msg = error(&[&path_of(&dir), "--code-gte", "lots"]);
    assert!(
        msg.contains("invalid value") && msg.contains("lots"),
        "expected an invalid-value error, got: {msg}"
    );
}

/// `--by-crate` needs a workspace. The rule lives in `application`, and this
/// pins that its error survives dispatch instead of being swallowed.
#[test]
fn by_crate_on_a_non_workspace_reports_the_orchestration_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_str().unwrap();

    match run(&[path, "--by-crate"]) {
        RunResult::Error(msg) => assert!(
            msg.contains("requires a Cargo workspace"),
            "unexpected message: {msg}"
        ),
        other => panic!("expected an Error, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Diff, through the real pipeline
// ---------------------------------------------------------------------------

/// Run a git command in `dir`, failing loudly with its output.
///
/// The output is carried into the panic message because a fixture that fails to
/// build is otherwise a bare "git failed" in CI, where the actual cause (commit
/// identity, permissions, a missing git) only lives in git's stderr.
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .output()
        .expect("git must be available");
    assert!(
        output.status.success(),
        "git {args:?} failed in {dir:?}: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Every column a diff CSV must expose: added_*, removed_*, and net_* for each
/// line type, plus a label. Same full-set rationale as [`COUNT_CSV_HEADERS`].
const DIFF_CSV_HEADERS: &[&str] = &[
    "label",
    "added_code",
    "added_tests",
    "added_examples",
    "added_docs",
    "added_comments",
    "added_blanks",
    "added_total",
    "removed_code",
    "removed_tests",
    "removed_examples",
    "removed_docs",
    "removed_comments",
    "removed_blanks",
    "removed_total",
    "net_code",
    "net_tests",
    "net_examples",
    "net_docs",
    "net_comments",
    "net_blanks",
    "net_total",
];

/// A repo with one commit (`Cargo.toml` + `src/lib.rs`), plus an uncommitted
/// edit to `src/lib.rs` that adds exactly one line of code — the single added
/// line the workdir-diff tests below assert on.
fn repo() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git(p, &["init", "-q"]);
    std::fs::write(
        p.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir(p.join("src")).unwrap();
    std::fs::write(p.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "one"]);

    std::fs::write(p.join("src/lib.rs"), "pub fn a() {}\npub fn b() {}\n").unwrap();
    dir
}

/// A workdir diff renders added lines. Representative coverage that the diff
/// command's own handler → post-dispatch → template chain is wired, without
/// re-testing gix's rev parsing (that is `rustloclib`'s job).
#[test]
fn diff_workdir_reports_the_added_line() {
    let dir = repo();
    let out = stdout(&[
        "diff",
        "-p",
        dir.path().to_str().unwrap(),
        "--output",
        "json",
    ]);
    let parsed: DiffQuerySet = serde_json::from_str(&out).expect("diff response");
    assert_eq!(parsed.total.added.code, 1);
}

/// Diff's CSV adapter is a second, separate adapter from count's — it carries
/// added/removed/net columns and must also flatten to rows.
#[test]
fn diff_csv_carries_net_columns() {
    let dir = repo();
    let out = stdout(&[
        "diff",
        "-p",
        dir.path().to_str().unwrap(),
        "--output",
        "csv",
    ]);

    let headers = csv_headers(&out);
    for expected in DIFF_CSV_HEADERS {
        assert!(
            headers.iter().any(|h| h == expected),
            "missing required CSV column `{expected}`; headers: {headers:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Approved render fixtures
// ---------------------------------------------------------------------------
//
// The tests above assert *properties* of the rendered table (a header exists, a
// separator carries nothing but rules). Those survive a change that quietly
// shifts every column by one space, which is exactly the class of regression a
// move of formatting policy into MiniJinja can introduce.
//
// So the whole rendered string is pinned against an approved fixture. Both
// halves of the human contract are covered: `text` mode for content and line
// structure, `term-debug` for semantic tag placement (which the text mode
// strips, and which no ANSI-scraping test should ever try to read).
//
// Regenerate after an *intended* wording or layout change:
//
//     UPDATE_RENDER_FIXTURES=1 cargo test -p rustloc
//
// then read the fixture diff — that diff IS the review artifact for a change to
// what users see.

/// Compare a rendered string against `tests/fixtures/render/<name>`.
///
/// The fixture directory is resolved from `CARGO_MANIFEST_DIR` rather than a
/// path relative to the process cwd, so these stay parallel-safe with the rest
/// of the module.
#[track_caller]
fn assert_render_fixture(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/render")
        .join(name);

    if std::env::var_os("UPDATE_RENDER_FIXTURES").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("missing fixture {path:?} ({e}); regenerate with UPDATE_RENDER_FIXTURES=1")
    });
    assert_eq!(
        actual, expected,
        "rendered output drifted from the approved fixture {name:?}"
    );
}

/// A two-crate workspace with deliberately lopsided magnitudes.
///
/// `alpha` is three digits of code and carries every line type (docs, comments,
/// blanks, an inline `#[cfg(test)]` module); `beta` is a single line. The gap is
/// the point: it forces the value columns wider than their own header words and
/// makes every per-column width the max of *different* rows, so a padding or
/// alignment regression has somewhere to show up.
fn wide_workspace() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    std::fs::write(
        p.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/alpha\", \"crates/beta\"]\nresolver = \"2\"\n",
    )
    .unwrap();

    let manifest = |name: &str| {
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n")
    };

    std::fs::create_dir_all(p.join("crates/alpha/src")).unwrap();
    std::fs::write(p.join("crates/alpha/Cargo.toml"), manifest("alpha")).unwrap();
    let mut alpha = String::from("//! Crate docs.\n\n");
    for i in 0..60 {
        alpha.push_str(&format!(
            "/// Docs for f{i}.\n// A comment.\npub fn f{i}() -> u32 {{ {i} }}\n\n"
        ));
    }
    alpha.push_str("#[cfg(test)]\nmod tests {\n    use super::*;\n");
    for i in 0..20 {
        alpha.push_str(&format!(
            "    #[test]\n    fn t{i}() {{ assert_eq!(f{i}(), {i}); }}\n"
        ));
    }
    alpha.push_str("}\n");
    std::fs::write(p.join("crates/alpha/src/lib.rs"), alpha).unwrap();

    std::fs::create_dir_all(p.join("crates/beta/src")).unwrap();
    std::fs::write(p.join("crates/beta/Cargo.toml"), manifest("beta")).unwrap();
    std::fs::write(p.join("crates/beta/src/lib.rs"), "pub fn b() {}\n").unwrap();

    dir
}

/// The default count table: every column, total aggregation.
#[test]
fn count_total_text_matches_the_approved_fixture() {
    let dir = wide_workspace();
    assert_render_fixture(
        "count_total.text",
        &stdout(&[&path_of(&dir), "--output", "text"]),
    );
}

/// Grouped rows, where alternating-row semantics and label alignment apply.
#[test]
fn count_by_crate_text_matches_the_approved_fixture() {
    let dir = wide_workspace();
    assert_render_fixture(
        "count_by_crate.text",
        &stdout(&[&path_of(&dir), "--by-crate", "--output", "text"]),
    );
}

/// `--top` reduces the rows, which changes only the footer *wording*.
#[test]
fn count_top_footer_matches_the_approved_fixture() {
    let dir = wide_workspace();
    assert_render_fixture(
        "count_by_crate_top.text",
        &stdout(&[
            &path_of(&dir),
            "--by-crate",
            "--top",
            "1",
            "--output",
            "text",
        ]),
    );
}

/// A filter (not `--top`) reduces the rows, which must use the plain "X of Y"
/// wording rather than "top X of Y".
#[test]
fn count_filtered_footer_matches_the_approved_fixture() {
    let dir = wide_workspace();
    assert_render_fixture(
        "count_by_crate_filtered.text",
        &stdout(&[
            &path_of(&dir),
            "--by-crate",
            "--code-gte",
            "10",
            "--output",
            "text",
        ]),
    );
}

/// A narrowed column set — the widths must re-derive from the columns that
/// remain, not from the full set.
#[test]
fn count_narrowed_columns_text_matches_the_approved_fixture() {
    let dir = wide_workspace();
    assert_render_fixture(
        "count_type_code.text",
        &stdout(&[
            &path_of(&dir),
            "--by-crate",
            "--type",
            "code",
            "--output",
            "text",
        ]),
    );
}

/// Every column at once.
///
/// The default table shows only Code/Tests/Docs/Total, so without this fixture
/// the `Examples`, `Comments` and `Blanks` header words would be rendered by no
/// test at all — and those words are exactly what this workstream moves out of
/// Rust and into the template, where a typo has nothing to catch it.
#[test]
fn count_all_columns_text_matches_the_approved_fixture() {
    let dir = wide_workspace();
    assert_render_fixture(
        "count_all_types.text",
        &stdout(&[
            &path_of(&dir),
            "--by-crate",
            "--type",
            "code",
            "--type",
            "tests",
            "--type",
            "examples",
            "--type",
            "docs",
            "--type",
            "comments",
            "--type",
            "blanks",
            "--type",
            "total",
            "--output",
            "text",
        ]),
    );
}

/// Each `--by-*` aggregation names its label column differently ("Crate" vs
/// "Module" vs "File") and counts a different unit in the footer ("crates" vs
/// "modules" vs "files"). Total aggregation is covered by
/// [`count_total_text_matches_the_approved_fixture`]; this covers the rest.
///
/// Only the label column and footer are asserted rather than a whole fixture:
/// the by-file/by-module labels are real paths, which vary per fixture layout,
/// and the full-table structure is already pinned by the by-crate fixtures.
#[test]
fn each_aggregation_names_its_label_column_and_unit() {
    let dir = wide_workspace();
    let path = path_of(&dir);

    for (flag, label_header, unit) in [
        ("--by-crate", "Crate", "crates"),
        ("--by-module", "Module", "modules"),
        ("--by-file", "File", "files"),
    ] {
        let out = stdout(&[&path, flag, "--output", "text"]);
        let header = out.lines().next().unwrap_or_default();
        assert!(
            header.contains(label_header),
            "{flag} should head its label column {label_header:?}, got: {header:?}"
        );
        assert!(
            out.contains(&format!(" {unit})")),
            "{flag} footer should count {unit:?} in:\n{out}"
        );
    }
}

/// The semantic tags themselves: `[header]` on the header line and the
/// alternating `[table_row_odd]` wrap. This is the assertion that a theme or
/// tag-placement regression trips.
#[test]
fn count_by_crate_term_debug_matches_the_approved_fixture() {
    let dir = wide_workspace();
    assert_render_fixture(
        "count_by_crate.term-debug",
        &stdout(&[&path_of(&dir), "--by-crate", "--output", "term-debug"]),
    );
}

/// A git repo over [`wide_workspace`] with one commit, then an uncommitted edit
/// that both adds and removes lines in `alpha` and adds a non-Rust file — so the
/// diff exercises `+added/-removed/net` notation with a negative net *and* the
/// skipped-changes summary in one fixture.
fn wide_repo() -> TempDir {
    let dir = wide_workspace();
    let p = dir.path();
    git(p, &["init", "-q"]);
    // README.md is committed *now* and edited below. A workdir diff compares
    // HEAD against the working tree, so an untracked file contributes nothing —
    // the skipped-changes summary would silently never render and this fixture
    // would approve its absence.
    std::fs::write(p.join("README.md"), "# demo\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "one"]);

    // Rewrite alpha smaller: removals dominate, so `net` goes negative.
    let mut alpha = String::from("//! Crate docs.\n\n");
    for i in 0..10 {
        alpha.push_str(&format!(
            "/// Docs for f{i}.\npub fn f{i}() -> u32 {{ {i} }}\n\n"
        ));
    }
    std::fs::write(p.join("crates/alpha/src/lib.rs"), alpha).unwrap();

    // beta grows a little, so at least one row nets positive.
    std::fs::write(
        p.join("crates/beta/src/lib.rs"),
        "pub fn b() {}\npub fn c() {}\npub fn d() {}\n",
    )
    .unwrap();

    // A skipped (non-Rust) change, which feeds the skipped-changes summary.
    std::fs::write(p.join("README.md"), "# demo\n\nSome prose.\n").unwrap();

    dir
}

/// The skipped-changes summary is the one line the diff fixtures cover only
/// incidentally, and it is easy to lose without noticing: the fixture would
/// simply approve a table that no longer mentions the skipped file. Assert it
/// renders at all, so [`wide_repo`] failing to produce a non-Rust change fails
/// loudly here rather than quietly weakening the fixtures.
#[test]
fn diff_reports_skipped_non_rust_changes() {
    let dir = wide_repo();
    let out = stdout(&[
        "diff",
        "-p",
        dir.path().to_str().unwrap(),
        "--by-crate",
        "--output",
        "text",
    ]);
    assert!(
        out.contains("Skipped changes:"),
        "expected a skipped-changes summary for the README.md edit in:\n{out}"
    );
}

/// The diff table: title, diff notation, legend, skipped-changes summary.
#[test]
fn diff_by_crate_text_matches_the_approved_fixture() {
    let dir = wide_repo();
    assert_render_fixture(
        "diff_by_crate.text",
        &stdout(&[
            "diff",
            "-p",
            dir.path().to_str().unwrap(),
            "--by-crate",
            "--output",
            "text",
        ]),
    );
}

/// The diff table's semantic tags: `[additions]`/`[deletions]` on the digits
/// only, `[muted]` on the legend.
#[test]
fn diff_by_crate_term_debug_matches_the_approved_fixture() {
    let dir = wide_repo();
    assert_render_fixture(
        "diff_by_crate.term-debug",
        &stdout(&[
            "diff",
            "-p",
            dir.path().to_str().unwrap(),
            "--by-crate",
            "--output",
            "term-debug",
        ]),
    );
}

/// `--staged` is only meaningful without revs. The rule lives in `command`;
/// this pins that it surfaces as a dispatch error rather than being ignored.
#[test]
fn diff_staged_with_revs_is_rejected() {
    let dir = repo();
    match run(&[
        "diff",
        "-p",
        dir.path().to_str().unwrap(),
        "HEAD~1",
        "--staged",
    ]) {
        RunResult::Error(msg) => assert!(msg.contains("--staged"), "unexpected message: {msg}"),
        other => panic!("expected an Error, got {other:?}"),
    }
}
