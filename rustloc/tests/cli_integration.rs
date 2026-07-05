//! Integration tests for rustloc CLI

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use tempfile::TempDir;

fn run_rustloc(args: &[&str]) -> (String, String, bool) {
    let (stdout, stderr, code) = run_rustloc_with_code(args);
    (stdout, stderr, code == Some(0))
}

/// Like `run_rustloc` but returns the actual exit code so tests can pin
/// down the precise failure mode (e.g. clap usage errors must exit 2,
/// not just "any nonzero").
fn run_rustloc_with_code(args: &[&str]) -> (String, String, Option<i32>) {
    let mut cmd_args = vec!["run", "--quiet", "-p", "rustloc", "--"];
    cmd_args.extend(args);

    let output = Command::new("cargo")
        .args(&cmd_args)
        .current_dir(env!("CARGO_MANIFEST_DIR").to_string() + "/..")
        .output()
        .expect("Failed to execute command");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (stdout, stderr, output.status.code())
}

// ============================================================================
// Hermetic fixture repo for diff-command tests
// ============================================================================
//
// The earlier version of these tests invoked `rustloc diff HEAD~5..HEAD`
// against the rustloc repo's OWN git history. That coupled test
// correctness to commit cadence (output drifts with every merge) and
// forced CI to use `fetch-depth: 0` so HEAD~5 actually existed. The
// rust-ci reusable workflow defaults to shallow checkout, so the only
// hermetic fix is to give the tests their own deterministic git repo.
//
// `fixture_repo()` builds a small repo in a TempDir on first call and
// caches it for the lifetime of the test binary. Every test that needs
// a diff target passes its path via rustloc's `--path` flag — the diff
// command resolves the git repo from there, not from cwd. The fixture
// has ≥6 commits and tags `v0.14.0` and `v0.14.2` so the historical
// HEAD~5..HEAD / tag-range tests keep their original shape.

/// Layout of the fixture repo.
///
/// Six commits (tags don't create commits — they just label an existing
/// one):
///
///   commit 1: add foo.rs (`rust_fn("foo_one", 10)` → 12 code lines)
///   commit 2: add bar.rs (`rust_fn("bar_one", 20)` → 22 code lines)
///             — tag `v0.14.0` (lightweight) on this commit
///   commit 3: extend foo.rs (+`rust_fn("foo_two", 5)` → +7 code lines)
///   commit 4: add baz.rs (`rust_fn("baz_one", 15)` → 17 code lines)
///   commit 5: extend bar.rs (+`rust_fn("bar_two", 3)` → +5 code lines)
///   commit 6: add qux.rs (`rust_fn("qux_one", 8)` → 10 code lines)
///             — tag `v0.14.2` (annotated) on this commit
///
/// `rust_fn(name, n)` always emits `n + 2` code lines (the signature line
/// and the closing brace count as code too), so the "code lines" column
/// above is `n + 2` per call.
///
/// HEAD-relative resolution (6 commits total, HEAD = commit 6):
///   HEAD~5 = commit 1
///   HEAD~5..HEAD covers commits 2, 3, 4, 5, 6 (5 commits)
///   v0.14.0..v0.14.2 covers commits 3, 4, 5, 6 (4 commits)
fn build_fixture_repo() -> TempDir {
    let dir = tempfile::Builder::new()
        .prefix("rustloc-fixture-")
        .tempdir()
        .expect("create tempdir for fixture repo");
    let path = dir.path();

    git(path, &["init", "--quiet", "--initial-branch=main"]);
    git(
        path,
        &["config", "user.email", "rustloc-tests@example.invalid"],
    );
    git(path, &["config", "user.name", "rustloc tests"]);
    git(path, &["config", "commit.gpgsign", "false"]);
    git(path, &["config", "tag.gpgsign", "false"]);

    // Commit 1: add foo.rs (12 code lines = 10 + signature + brace)
    write_file(&path.join("foo.rs"), &rust_fn("foo_one", 10));
    git(path, &["add", "foo.rs"]);
    commit(path, "add foo.rs");

    // Commit 2: add bar.rs (22 code lines = 20 + signature + brace)
    write_file(&path.join("bar.rs"), &rust_fn("bar_one", 20));
    git(path, &["add", "bar.rs"]);
    commit(path, "add bar.rs");

    // Tag v0.14.0 on commit 2 (lightweight tag — the gix peel path handles
    // both annotated and lightweight; we cover annotated below at v0.14.2).
    // A tag is not a commit, so this doesn't advance HEAD.
    git(path, &["tag", "v0.14.0"]);

    // Commit 3: extend foo.rs (+7 code lines = 5 + signature + brace)
    let mut foo_v2 = rust_fn("foo_one", 10);
    foo_v2.push_str(&rust_fn("foo_two", 5));
    write_file(&path.join("foo.rs"), &foo_v2);
    git(path, &["add", "foo.rs"]);
    commit(path, "extend foo.rs");

    // Commit 4: add baz.rs (17 code lines = 15 + signature + brace)
    write_file(&path.join("baz.rs"), &rust_fn("baz_one", 15));
    git(path, &["add", "baz.rs"]);
    commit(path, "add baz.rs");

    // Commit 5: extend bar.rs (+5 code lines = 3 + signature + brace)
    let mut bar_v2 = rust_fn("bar_one", 20);
    bar_v2.push_str(&rust_fn("bar_two", 3));
    write_file(&path.join("bar.rs"), &bar_v2);
    git(path, &["add", "bar.rs"]);
    commit(path, "extend bar.rs");

    // Commit 6: add qux.rs (10 code lines = 8 + signature + brace),
    // then tag v0.14.2 (annotated) on this commit.
    write_file(&path.join("qux.rs"), &rust_fn("qux_one", 8));
    git(path, &["add", "qux.rs"]);
    commit(path, "add qux.rs");
    git(
        path,
        &["tag", "-a", "v0.14.2", "-m", "release v0.14.2 (fixture)"],
    );

    dir
}

/// Cached fixture repo path. The TempDir lives in a OnceLock so it's
/// not dropped until the test process exits. All diff tests share the
/// same fixture — building it once amortises the ~20 git invocations.
fn fixture_repo() -> &'static Path {
    static FIXTURE: OnceLock<TempDir> = OnceLock::new();
    FIXTURE.get_or_init(build_fixture_repo).path()
}

/// Convenience: the fixture repo's path as a `&str` (UTF-8 path on all
/// platforms we test on, including the tempfile-injected hex suffix).
fn fixture_path_str() -> String {
    fixture_repo().to_string_lossy().to_string()
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("git {:?} in {:?} failed to spawn: {}", args, dir, e));
    assert!(
        output.status.success(),
        "git {:?} failed in {:?}: stdout={:?} stderr={:?}",
        args,
        dir,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Commit with stable author/committer dates so commit hashes — though
/// not asserted directly — at least don't drift across re-runs. Useful
/// when debugging.
fn commit(dir: &Path, message: &str) {
    let env_date = "2024-01-01T00:00:00Z";
    let output = Command::new("git")
        .args(["commit", "--quiet", "-m", message])
        .env("GIT_AUTHOR_DATE", env_date)
        .env("GIT_COMMITTER_DATE", env_date)
        .current_dir(dir)
        .output()
        .expect("git commit failed to spawn");
    assert!(
        output.status.success(),
        "git commit failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn write_file(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap_or_else(|e| panic!("write {:?}: {}", path, e));
}

/// Render N lines of trivial Rust code wrapped in a function. Lines are
/// `let _<i> = <i>;` — they count as `code` (not blank/comment/test) and
/// the line count is exactly N + 2 (fn signature + closing brace). The
/// classifier counts the fn signature and brace as code too, so the
/// effective "code" delta from this helper is N + 2 per call.
fn rust_fn(name: &str, body_lines: usize) -> String {
    let mut s = format!("pub fn {name}() {{\n");
    for i in 0..body_lines {
        s.push_str(&format!("    let _{i} = {i};\n"));
    }
    s.push_str("}\n");
    s
}

fn expected_language_help_line() -> String {
    let languages = rustloclib::available_languages()
        .iter()
        .map(|language| language.name())
        .collect::<Vec<_>>()
        .join(", ");
    format!("Available: {languages}")
}

#[test]
fn test_cli_help() {
    let (stdout, _, success) = run_rustloc(&["--help"]);

    assert!(success);
    assert!(stdout.contains("rustloc"));
    assert!(stdout.contains("--crate"));
    assert!(stdout.contains("--lang"));
    assert!(stdout.contains("--output"));
    assert!(stdout.contains("--by-crate"));
    assert!(stdout.contains("--by-file"));
    assert!(stdout.contains(&expected_language_help_line()));
    assert!(stdout.contains("-l all"));
}

#[test]
fn test_cli_version() {
    let (stdout, _, success) = run_rustloc(&["--version"]);

    assert!(success);
    assert!(stdout.contains("rustloc"));
}

#[test]
fn test_lang_typescript_counts_typescript_file() {
    let dir = tempfile::Builder::new()
        .prefix("rustloc-typescript-")
        .tempdir()
        .expect("tempdir");
    write_file(
        &dir.path().join("widget.test.ts"),
        "/** Widget docs */\ntest('works', () => true);\n",
    );

    let path = dir.path().to_string_lossy().to_string();
    let (stdout, _, success) = run_rustloc(&[&path, "--lang", "typescript", "--output", "json"]);

    assert!(success);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    assert_eq!(parsed["total"]["docs"].as_u64(), Some(1));
    assert_eq!(parsed["total"]["tests"].as_u64(), Some(1));
    assert_eq!(parsed["file_count"].as_u64(), Some(1));
}

#[test]
fn test_lang_python_by_module_counts_python_tree() {
    let dir = tempfile::Builder::new()
        .prefix("rustloc-python-modules-")
        .tempdir()
        .expect("tempdir");
    std::fs::create_dir_all(dir.path().join("pkg/sub")).unwrap();
    write_file(&dir.path().join("pkg/__init__.py"), "VALUE = 1\n");
    write_file(
        &dir.path().join("pkg/sub/worker.py"),
        "def run():\n    return VALUE\n",
    );

    let path = dir.path().to_string_lossy().to_string();
    let (stdout, _, success) = run_rustloc(&[&path, "--lang", "python", "--by-module"]);

    assert!(success);
    assert!(stdout.contains("pkg "));
    assert!(stdout.contains("pkg::sub"));
}

#[test]
fn test_table_output() {
    let (stdout, _, success) = run_rustloc(&["."]);

    assert!(success);
    // Check for context column headers in default view (code, tests, docs, total)
    assert!(stdout.contains("Code"));
    assert!(stdout.contains("Tests"));
    assert!(stdout.contains("Docs"));
    assert!(stdout.contains("Total"));
    // Total row shows file count in row name
    assert!(stdout.contains("Total (") && stdout.contains("files)"));
}

// ============================================================================
// Table line-structure regression tests (issue #84)
// ============================================================================
//
// The table layout has silently regressed more than once: a stray Jinja
// whitespace-trim marker (`{#-` in stats_table.jinja, commit 4a4efd7) ate
// the newline after the header separator, so the first data row — or the
// Total footer when there are no rows — rendered on the same line as the
// `────` rule. Nothing asserted the line structure, so it shipped (v0.17.1).
//
// These tests pin the structure down: separator lines must contain nothing
// but `─`, the header must sit on its own line directly above a separator,
// and the footer must not share a line with a rule. Any future trim bug in
// the template collapses two of these lines into one and fails the "pure
// separator" assertion.

/// True when `line` is a horizontal rule and nothing else.
fn is_pure_separator(line: &str) -> bool {
    !line.is_empty() && line.chars().all(|c| c == '─')
}

/// Assert the structural invariants of a rendered table:
///
/// - every line containing `─` contains ONLY `─` (a jammed row/footer is
///   exactly how the v0.17.1 regression manifested),
/// - there are exactly `expected_separators` rule lines (1 when the table
///   has no breakdown rows, 2 when rows sit between header and footer),
/// - the line directly above the first rule is the column header,
/// - the line directly below each rule is non-empty (a data row or the
///   footer — never a continuation jammed into the rule itself),
/// - the `Total (` footer line carries no rule characters.
fn assert_table_line_structure(stdout: &str, expected_separators: usize) {
    let lines: Vec<&str> = stdout.lines().collect();

    for line in &lines {
        if line.contains('─') {
            assert!(
                is_pure_separator(line),
                "separator line is jammed with other content: {line:?}\nfull output:\n{stdout}"
            );
        }
    }

    let separator_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| is_pure_separator(l))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        separator_indices.len(),
        expected_separators,
        "expected {expected_separators} separator lines, got {}:\n{stdout}",
        separator_indices.len()
    );

    let first = separator_indices[0];
    assert!(first > 0, "separator must not be the first line:\n{stdout}");
    let header = lines[first - 1];
    assert!(
        header.contains("Code") && header.contains("Total"),
        "line above the first separator must be the column header, got {header:?}:\n{stdout}"
    );

    for &idx in &separator_indices {
        let below = lines
            .get(idx + 1)
            .unwrap_or_else(|| panic!("separator must not be the last line:\n{stdout}"));
        assert!(
            !below.trim().is_empty(),
            "line below a separator must be a data row or the footer:\n{stdout}"
        );
    }

    let footer = lines
        .iter()
        .find(|l| l.contains("Total ("))
        .unwrap_or_else(|| panic!("missing `Total (` footer line:\n{stdout}"));
    assert!(
        !footer.contains('─'),
        "footer must not share a line with a rule: {footer:?}\n{stdout}"
    );
}

#[test]
fn test_count_total_table_line_structure() {
    // Total aggregation: no breakdown rows, so a single rule between the
    // header and the footer.
    let (stdout, _, success) = run_rustloc(&["."]);

    assert!(success);
    assert_table_line_structure(&stdout, 1);
}

#[test]
fn test_count_by_file_table_line_structure() {
    // Breakdown rows present: header rule + footer rule, rows in between.
    let (stdout, _, success) = run_rustloc(&[".", "--by-file"]);

    assert!(success);
    assert_table_line_structure(&stdout, 2);

    // The line right after the header rule must be a data row (a file
    // path), not the footer — i.e. rows actually sit between the rules.
    let lines: Vec<&str> = stdout.lines().collect();
    let first_sep = lines.iter().position(|l| is_pure_separator(l)).unwrap();
    let first_row = lines[first_sep + 1];
    assert!(
        first_row.contains(".rs"),
        "expected a file row right after the header separator, got {first_row:?}:\n{stdout}"
    );
}

#[test]
fn test_count_by_crate_table_line_structure() {
    let (stdout, _, success) = run_rustloc(&[".", "--by-crate"]);

    assert!(success);
    assert_table_line_structure(&stdout, 2);
}

#[test]
fn test_diff_total_table_line_structure() {
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&["diff", "HEAD~5..HEAD", "--path", &fixture]);

    assert!(success);
    assert_table_line_structure(&stdout, 1);
}

#[test]
fn test_diff_by_file_table_line_structure() {
    let fixture = fixture_path_str();
    let (stdout, _, success) =
        run_rustloc(&["diff", "HEAD~5..HEAD", "--path", &fixture, "--by-file"]);

    assert!(success);
    assert_table_line_structure(&stdout, 2);
}

#[test]
fn test_diff_zero_files_table_line_structure() {
    // The exact shape from the original report: a diff with no changed
    // files still renders header / rule / footer — the footer must not be
    // jammed onto the rule.
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&["diff", "HEAD..HEAD", "--path", &fixture]);

    assert!(success);
    assert!(stdout.contains("Total (0 files)"));
    assert_table_line_structure(&stdout, 1);
}

#[test]
fn test_json_output() {
    let (stdout, _, success) = run_rustloc(&[".", "--output", "json"]);

    assert!(success);

    // Verify it's valid JSON with data structure (numeric values, not presentation table)
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");
    assert!(parsed.get("total").is_some());
    assert!(parsed.get("file_count").is_some());
    assert!(parsed.get("aggregation").is_some());

    // Values should be numeric, not strings
    let total = &parsed["total"];
    assert!(total["code"].is_u64(), "code should be a number");
    assert!(total["tests"].is_u64(), "tests should be a number");
    assert!(total["docs"].is_u64(), "docs should be a number");
    assert!(total["total"].is_u64(), "total should be a number");
    assert!(total["blanks"].is_u64(), "blanks should be a number");
    assert!(total["comments"].is_u64(), "comments should be a number");
    assert!(total["examples"].is_u64(), "examples should be a number");
}

/// Parse `output` with the csv crate so column counts respect quoting
/// (file paths with commas, embedded quotes, etc.) — `split(',')` would
/// miscount any well-formed-but-quoted row. Returns (headers, records).
fn parse_csv(output: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let mut rdr = csv::Reader::from_reader(output.as_bytes());
    let headers: Vec<String> = rdr
        .headers()
        .expect("CSV must have a header row")
        .iter()
        .map(|s| s.to_string())
        .collect();
    let records: Vec<Vec<String>> = rdr
        .records()
        .map(|r| {
            r.expect("malformed CSV record")
                .iter()
                .map(|s| s.to_string())
                .collect()
        })
        .collect();
    (headers, records)
}

/// Headers that every count CSV must expose, regardless of column order.
/// Pinning the full set so a regression that drops, say, `examples` would
/// fail the test instead of slipping through under the row-width check.
const COUNT_CSV_HEADERS: &[&str] = &[
    "label", "code", "tests", "examples", "docs", "comments", "blanks", "total",
];

/// Headers that every diff CSV must expose. Includes added_*, removed_*,
/// and net_* for each line type plus a label column.
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

fn assert_headers_present(headers: &[String], required: &[&str]) {
    for col in required {
        assert!(
            headers.iter().any(|h| h == col),
            "missing required CSV column `{col}`; headers: {headers:?}"
        );
    }
}

/// A valid CSV from `--output csv` must be one row per item (file, crate,
/// or module) plus a final TOTAL row — not a single mega-row formed by
/// flattening the whole queryset object into hundreds of `items.0.*`,
/// `items.1.*` columns.
#[test]
fn test_csv_output_by_file() {
    let (stdout, _, success) = run_rustloc(&[".", "--output", "csv", "--by-file"]);
    assert!(success);

    let (headers, records) = parse_csv(&stdout);

    assert!(
        !headers.iter().any(|h| h.starts_with("items.")),
        "header must not flatten the queryset object into items.0.* columns; got: {headers:?}"
    );
    assert_headers_present(&headers, COUNT_CSV_HEADERS);

    assert!(
        records.len() >= 2,
        "expected at least one data row + a TOTAL row"
    );
    let label_idx = headers.iter().position(|h| h == "label").unwrap();
    assert!(
        records.iter().any(|r| r[label_idx] == "TOTAL"),
        "expected a TOTAL summary row"
    );
}

#[test]
fn test_csv_output_total() {
    let (stdout, _, success) = run_rustloc(&[".", "--output", "csv"]);
    assert!(success);

    let (headers, records) = parse_csv(&stdout);
    assert_headers_present(&headers, COUNT_CSV_HEADERS);

    // Total aggregation → exactly one TOTAL row.
    assert_eq!(records.len(), 1, "expected only a TOTAL row");
    let label_idx = headers.iter().position(|h| h == "label").unwrap();
    assert_eq!(records[0][label_idx], "TOTAL");
}

#[test]
fn test_csv_output_diff() {
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&[
        "diff",
        "HEAD~5..HEAD",
        "--path",
        &fixture,
        "--output",
        "csv",
        "--by-file",
    ]);
    assert!(success);

    let (headers, records) = parse_csv(&stdout);

    assert!(
        !headers.iter().any(|h| h.starts_with("items.")),
        "diff header must not flatten items.*; got: {headers:?}"
    );
    assert_headers_present(&headers, DIFF_CSV_HEADERS);

    let label_idx = headers.iter().position(|h| h == "label").unwrap();
    assert!(records.iter().any(|r| r[label_idx] == "TOTAL"));
}

#[test]
fn test_by_crate_output() {
    let (stdout, _, success) = run_rustloc(&[".", "--by-crate"]);

    assert!(success);
    assert!(stdout.contains("Crate"));
    assert!(stdout.contains("rustloclib"));
    assert!(stdout.contains("rustloc"));
    assert!(stdout.contains("Total ("));
}

#[test]
fn test_by_module_output() {
    let (stdout, _, success) = run_rustloc(&[".", "--by-module"]);

    assert!(success);
    assert!(stdout.contains("Module"));
    // Modules aggregate at directory level
    assert!(stdout.contains("rustloclib::data"));
    assert!(stdout.contains("rustloclib::output"));
    assert!(stdout.contains("rustloclib::query"));
    assert!(stdout.contains("rustloclib::source"));
    // Should NOT contain per-file module paths
    assert!(!stdout.contains("rustloclib::data::counter"));
    assert!(!stdout.contains("rustloclib::data::diff"));
    assert!(stdout.contains("rustloc"));
    assert!(stdout.contains("Total ("));
}

#[test]
fn test_crate_filter() {
    let (stdout, _, success) = run_rustloc(&[".", "--crate", "rustloc"]);

    assert!(success);
    // The filtered output should show only 2 files
    assert!(stdout.contains("2")); // Files column shows 2
}

#[test]
fn test_invalid_path() {
    let (_, stderr, success) = run_rustloc(&["/nonexistent/path"]);

    assert!(!success);
    assert!(stderr.contains("Error:"));
}

// ============================================================================
// Diff command tests
// ============================================================================

#[test]
fn test_diff_help() {
    let (stdout, _, success) = run_rustloc(&["diff", "--help"]);

    assert!(success);
    assert!(stdout.contains("diff"));
    assert!(stdout.contains("Revspec or range"));
    assert!(stdout.contains("--by-file"));
    assert!(stdout.contains("--by-crate"));
    assert!(stdout.contains(&expected_language_help_line()));
    assert!(stdout.contains("-l all"));
}

#[test]
fn test_diff_table_output() {
    // Hermetic fixture repo: 6 commits, deterministic file changes.
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&["diff", "HEAD~5..HEAD", "--path", &fixture]);

    assert!(success, "diff command should succeed");
    assert!(stdout.contains("Diff:"));
    // Total row shows file count in row name (same layout as counts)
    assert!(stdout.contains("Total (") && stdout.contains("files)"));
    assert!(stdout.contains("Code"));
    assert!(stdout.contains("Tests"));
    assert!(stdout.contains("Docs"));
    assert!(stdout.contains("Total"));
    // Check for diff format (additions and deletions present)
    assert!(stdout.contains("+") && stdout.contains("-"));
}

#[test]
fn test_diff_with_separate_commits() {
    // Test using two separate commit arguments
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&["diff", "HEAD~5", "HEAD", "--path", &fixture]);

    assert!(success);
    assert!(stdout.contains("Diff:"));
    // Total row shows file count in row name (same layout as counts)
    assert!(stdout.contains("Total (") && stdout.contains("files)"));
}

#[test]
fn test_diff_annotated_tag_range() {
    // Annotated tags must be peeled to their target commit by the resolver.
    // This used to fail with "expected commit, got tag" before delegating
    // resolution to gix::Repository::rev_parse + peel_to_commit. The fixture
    // sets `v0.14.2` as an annotated tag specifically to exercise that path.
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&["diff", "v0.14.0..v0.14.2", "--path", &fixture]);

    assert!(success, "tag-to-tag diff should resolve via gix rev_parse");
    assert!(stdout.contains("Diff: v0.14.0"));
    assert!(stdout.contains("v0.14.2"));
}

#[test]
fn test_diff_single_tag_against_head() {
    // A single revspec is diffed against HEAD.
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&["diff", "v0.14.2", "--path", &fixture]);

    assert!(success);
    assert!(stdout.contains("Diff: v0.14.2"));
    assert!(stdout.contains("HEAD"));
}

#[test]
fn test_diff_rejects_range_with_extra_arg() {
    // `rustloc diff a..b c` would naively become `a..b..c`, an invalid
    // revspec. The CLI should detect and reject this with a clear error.
    // This is a pure CLI-validation test (rejection happens before any
    // git work), so we don't need the fixture — but we use it anyway to
    // keep the test fully decoupled from the rustloc repo's own state.
    let fixture = fixture_path_str();
    let (_, stderr, success) = run_rustloc(&["diff", "HEAD~1..HEAD", "HEAD", "--path", &fixture]);

    assert!(!success, "passing a range plus a second arg should fail");
    assert!(
        stderr.contains("either a single range") || stderr.contains("not both"),
        "unexpected error message: {}",
        stderr
    );
}

#[test]
fn test_diff_merge_base_syntax() {
    // a...b should resolve to merge-base(a, b)..b.
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&["diff", "v0.14.0...v0.14.2", "--path", &fixture]);

    assert!(success);
    assert!(stdout.contains("merge-base"));
}

#[test]
fn test_diff_json_output() {
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&[
        "diff",
        "HEAD~5..HEAD",
        "--path",
        &fixture,
        "--output",
        "json",
    ]);

    assert!(success);

    // Verify it's valid JSON with data structure
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");
    assert!(parsed.get("from_commit").is_some());
    assert!(parsed.get("to_commit").is_some());
    assert!(parsed.get("total").is_some());
    assert!(parsed.get("file_count").is_some());

    // Diff total has added/removed with numeric values
    let total = &parsed["total"];
    assert!(
        total["added"]["code"].is_u64(),
        "added.code should be a number"
    );
    assert!(
        total["removed"]["code"].is_u64(),
        "removed.code should be a number"
    );

    // Commit range info
    assert_eq!(parsed["from_commit"].as_str().unwrap(), "HEAD~5");
    assert_eq!(parsed["to_commit"].as_str().unwrap(), "HEAD");
}

// CSV output removed - using outstanding's built-in JSON/table output

#[test]
fn test_diff_by_file() {
    let fixture = fixture_path_str();
    let (stdout, _, success) =
        run_rustloc(&["diff", "HEAD~5..HEAD", "--path", &fixture, "--by-file"]);

    assert!(success);
    // New unified layout has File header and Code/Tests/Examples/Total columns
    assert!(stdout.contains("File"));
    assert!(stdout.contains("Code"));
    assert!(stdout.contains("Total"));
    // Should show diff format (additions and deletions present)
    assert!(stdout.contains("+") && stdout.contains("-"));
}

#[test]
fn test_diff_invalid_commit() {
    let fixture = fixture_path_str();
    let (_, stderr, success) = run_rustloc(&["diff", "invalid_commit_hash", "--path", &fixture]);

    assert!(!success);
    assert!(stderr.contains("Error:"));
}

#[test]
fn test_diff_same_commit() {
    // Diffing a commit against itself should show no changes.
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&["diff", "HEAD..HEAD", "--path", &fixture]);

    assert!(success);
    assert!(stdout.contains("Total (0 files)"));
}

// ============================================================================
// Working directory diff tests
// ============================================================================

#[test]
fn test_diff_workdir() {
    // Diff without commit args should show working directory changes.
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&["diff", "--path", &fixture]);

    assert!(success, "diff without args should succeed");
    assert!(stdout.contains("Diff: HEAD"));
    assert!(stdout.contains("working tree"));
    // Total row shows file count in row name (same layout as counts)
    assert!(stdout.contains("Total (") && stdout.contains("files)"));
}

#[test]
fn test_diff_workdir_staged() {
    // Diff with --staged should show staged changes vs HEAD.
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&["diff", "--staged", "--path", &fixture]);

    assert!(success, "diff --staged should succeed");
    assert!(stdout.contains("Diff: HEAD"));
    assert!(stdout.contains("index"));
    // Total row shows file count in row name (same layout as counts)
    assert!(stdout.contains("Total (") && stdout.contains("files)"));
}

#[test]
fn test_diff_workdir_cached_alias() {
    // --cached should work as an alias for --staged.
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&["diff", "--cached", "--path", &fixture]);

    assert!(success, "diff --cached should succeed");
    assert!(stdout.contains("Diff: HEAD"));
    assert!(stdout.contains("index"));
}

#[test]
fn test_diff_workdir_json() {
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&["diff", "--path", &fixture, "--output", "json"]);

    assert!(success);

    // Verify it's valid JSON with data structure
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");
    assert!(parsed.get("from_commit").is_some());
    assert!(parsed.get("to_commit").is_some());
    assert!(parsed.get("total").is_some());
    assert!(parsed["from_commit"].as_str().unwrap().contains("HEAD"));
}

#[test]
fn test_diff_workdir_by_file() {
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&["diff", "--by-file", "--path", &fixture]);

    assert!(success);
    // Should still show working tree diff
    assert!(stdout.contains("working tree"));
}

#[test]
fn test_diff_skipped_summary_is_language_neutral() {
    let dir = tempfile::Builder::new()
        .prefix("rustloc-skipped-summary-")
        .tempdir()
        .expect("tempdir");
    let path = dir.path();
    git(path, &["init", "--quiet", "--initial-branch=main"]);
    git(
        path,
        &["config", "user.email", "rustloc-tests@example.invalid"],
    );
    git(path, &["config", "user.name", "rustloc tests"]);
    git(path, &["config", "commit.gpgsign", "false"]);
    write_file(&path.join("app.py"), "print('old')\n");
    git(path, &["add", "app.py"]);
    commit(path, "add python");
    write_file(&path.join("app.py"), "print('old')\nprint('new')\n");

    let path_arg = path.to_string_lossy().to_string();
    let (stdout, _, success) = run_rustloc(&["diff", "--path", &path_arg]);

    assert!(success);
    assert!(stdout.contains("Skipped changes:"));
    assert!(!stdout.contains("Non-Rust changes:"));
}

#[test]
fn test_diff_staged_with_commits_error() {
    // Using --staged with commit args should fail.
    let fixture = fixture_path_str();
    let (_, stderr, success) = run_rustloc(&["diff", "HEAD~1", "--staged", "--path", &fixture]);

    assert!(!success);
    assert!(stderr.contains("--staged") || stderr.contains("--cached"));
}

#[test]
fn test_top_truncates_by_crate() {
    // This workspace has 2 crates; --top 1 should leave just one row.
    let (stdout, _, success) = run_rustloc(&[".", "--by-crate", "--top", "1"]);

    assert!(success);
    // Footer must make truncation explicit and report the original count.
    assert!(
        stdout.contains("top 1 of 2 crates"),
        "expected 'top 1 of 2 crates' in footer, got:\n{}",
        stdout
    );
}

#[test]
fn test_top_no_truncation_label_when_not_truncated() {
    let (stdout, _, success) = run_rustloc(&[".", "--by-crate"]);

    assert!(success);
    // Without --top the footer should be the plain count, no "top N of M".
    assert!(stdout.contains("Total (2 crates)"));
    assert!(!stdout.contains("top "));
}

#[test]
fn test_top_with_by_file_and_ordering() {
    // Truncation runs after ordering: top 2 by code descending should
    // be the two largest files.
    let (stdout, _, success) = run_rustloc(&[".", "--by-file", "-o", "-code", "--top", "2"]);

    assert!(success);
    assert!(stdout.contains("top 2 of"));
    // Header still present.
    assert!(stdout.contains("File"));
}

#[test]
fn test_top_zero_shows_no_rows() {
    let (stdout, _, success) = run_rustloc(&[".", "--by-crate", "--top", "0"]);

    assert!(success);
    assert!(stdout.contains("top 0 of 2 crates"));
    // No crate row should be shown — the workspace has crates "rustloc"
    // and "rustloclib"; assert neither appears as a row label by checking
    // there's no row line that begins with one.
    let rows_with_rustloc = stdout
        .lines()
        .filter(|line| {
            line.trim_start().starts_with("rustloc ") || line.trim_start().starts_with("rustloclib")
        })
        .count();
    assert_eq!(
        rows_with_rustloc, 0,
        "expected no crate rows, got:\n{}",
        stdout
    );
}

#[test]
fn test_top_larger_than_data_is_noop() {
    // With --top 99 in a 2-crate workspace, we still see both, no truncation marker.
    let (stdout, _, success) = run_rustloc(&[".", "--by-crate", "--top", "99"]);

    assert!(success);
    assert!(stdout.contains("Total (2 crates)"));
    assert!(!stdout.contains("top "));
}

#[test]
fn test_top_with_diff_by_file() {
    // --top should also work on diff output.
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&[
        "diff",
        "HEAD~5..HEAD",
        "--path",
        &fixture,
        "--by-file",
        "--top",
        "2",
    ]);

    assert!(success);
    // If there are at least 3 files changed, we'll see the truncation marker;
    // if fewer, we'll see the plain count. Either way the command should not
    // error and the footer should be present.
    assert!(stdout.contains("Total ("));
}

#[test]
fn test_top_count_json_truncates_items_and_carries_total_items() {
    // The structured-output path should also honour --top: items array
    // truncated, but `total_items` carries the pre-truncation count and
    // `total` reflects the full data set.
    let (stdout, _, success) = run_rustloc(&[".", "--by-crate", "--top", "1", "--output", "json"]);

    assert!(success);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");

    let items = parsed["items"].as_array().expect("items should be array");
    assert_eq!(items.len(), 1, "items should be truncated to top 1");

    assert_eq!(
        parsed["total_items"].as_u64().unwrap(),
        2,
        "total_items should be pre-truncation count"
    );

    // Total counts must be unaffected by truncation.
    assert!(parsed["total"]["code"].as_u64().unwrap() > 0);
}

#[test]
fn test_top_diff_json_truncates_items_and_carries_total_items() {
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&[
        "diff",
        "HEAD~5..HEAD",
        "--path",
        &fixture,
        "--by-file",
        "--top",
        "1",
        "--output",
        "json",
    ]);

    assert!(success);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");

    let items = parsed["items"].as_array().expect("items should be array");
    assert!(items.len() <= 1, "items should be truncated to at most 1");

    let total_items = parsed["total_items"]
        .as_u64()
        .expect("total_items should be present");
    // total_items should be at least items.len(); when truncation actually
    // happened it'll be strictly larger.
    assert!(total_items >= items.len() as u64);
}

// ============================================================================
// Filter query tests (--<field>-<op> N)
// ============================================================================

#[test]
fn test_filter_help_hides_individual_flags_and_shows_synthetic_doc() {
    let (stdout, _, success) = run_rustloc(&["--help"]);

    assert!(success);

    // The 42 flags must be hidden. Pick a few representatives and assert
    // they don't appear in the rendered help.
    assert!(
        !stdout.contains("--code-gte <"),
        "--code-gte should not appear in help"
    );
    assert!(
        !stdout.contains("--tests-lt <"),
        "--tests-lt should not appear in help"
    );
    assert!(
        !stdout.contains("--total-eq <"),
        "--total-eq should not appear in help"
    );

    // The synthetic doc block IS expected to be present.
    assert!(stdout.contains("Filter options"));
    assert!(stdout.contains("--<category>-<op>"));
    assert!(stdout.contains("Categories: code, tests, examples, docs, comments, blanks, total"));
    assert!(stdout.contains("Operators:  gt, gte, eq, ne, lt, lte"));
}

#[test]
fn test_filter_help_preserves_existing_examples_block() {
    // The Cli derive defines an `after_long_help` examples block. The
    // filter injection appends to it rather than replacing — both must
    // be visible in --help.
    let (stdout, _, success) = run_rustloc(&["--help"]);

    assert!(success);
    // Original examples (from Cli derive)
    assert!(
        stdout.contains("rustloc --by-crate"),
        "original Cli examples block should still appear"
    );
    // Filter examples (from our appended doc)
    assert!(
        stdout.contains("rustloc --by-file --code-gte 1000"),
        "filter doc block should appear"
    );
}

#[test]
fn test_diff_help_preserves_existing_examples_block() {
    // Same invariant for the diff subcommand.
    let (stdout, _, success) = run_rustloc(&["diff", "--help"]);

    assert!(success);
    assert!(
        stdout.contains("rustloc diff main feature"),
        "original diff examples should still appear"
    );
    assert!(
        stdout.contains("rustloc --by-file --code-gte 1000"),
        "filter doc block should appear in diff --help"
    );
}

#[test]
fn test_filter_gte_works_via_default_subcommand() {
    // `rustloc --by-file --code-gte 100 .` (no explicit `count`)
    // should work because the args live on top-level Cli too.
    let (stdout, _, success) = run_rustloc(&[".", "--by-file", "--code-gte", "100"]);

    assert!(success);
    // Footer wording must be the filter-only form, not the "top" form.
    assert!(
        stdout.contains(" of ") && !stdout.contains("top "),
        "expected filter-only footer wording, got:\n{}",
        stdout
    );
}

#[test]
fn test_filter_via_explicit_count_subcommand() {
    let (stdout, _, success) = run_rustloc(&["count", ".", "--by-file", "--code-gte", "100"]);

    assert!(success);
    assert!(stdout.contains("File"));
}

#[test]
fn test_filter_combines_with_and() {
    // Two predicates AND-combined; should narrow more than either alone.
    let (stdout, _, success) =
        run_rustloc(&[".", "--by-crate", "--code-gte", "50", "--tests-lt", "1500"]);

    assert!(success);
    // The repo has 2 crates; the AND should keep the rustloc crate (which
    // has < 1500 tests) and exclude rustloclib (which has more).
    let rows: Vec<_> = stdout.lines().collect();
    assert!(rows
        .iter()
        .any(|row| row.trim_start().starts_with("rustloc ")));
    assert!(!rows
        .iter()
        .any(|row| row.trim_start().starts_with("rustloclib ")));
}

#[test]
fn test_filter_chains_with_top_uses_top_wording() {
    // When both filter and top apply, the footer says "top X of Y" because
    // the visible rows are the sorted top of what passed the filter.
    let (stdout, _, success) = run_rustloc(&[
        ".",
        "--by-file",
        "-o",
        "-code",
        "--code-gte",
        "100",
        "--top",
        "1",
    ]);

    assert!(success);
    assert!(
        stdout.contains("top 1 of"),
        "expected 'top 1 of' wording, got:\n{}",
        stdout
    );
}

#[test]
fn test_filter_total_uses_filtered_sum() {
    // --total-gte semantics: sum of currently-enabled types (per --type).
    // With default types (code+tests+docs+total), the rustloc crate should
    // pass --total-gte 100 (its sum is well over).
    let (stdout, _, success) = run_rustloc(&[".", "--by-crate", "--total-gte", "100"]);

    assert!(success);
    assert!(stdout.contains("rustloc"));
}

#[test]
fn test_filter_eliminates_all_rows() {
    // No file in this codebase has 100000+ code lines; --code-gte 100000
    // should produce an empty rows section. The footer should reflect "0 of N".
    let (stdout, _, success) = run_rustloc(&[".", "--by-file", "--code-gte", "100000"]);

    assert!(success);
    assert!(
        stdout.contains("0 of "),
        "expected '0 of N' in footer, got:\n{}",
        stdout
    );
    assert!(!stdout.contains("top "));
}

#[test]
fn test_filter_unknown_op_errors_to_stderr_with_nonzero_exit() {
    // Unknown operator on a real field. Must exit with the standard
    // usage-error code (2), surface clap's error text on stderr, and
    // produce no stdout — otherwise a script piping rustloc would silently
    // miss the typo (the real-world report that motivated this test).
    let (stdout, stderr, code) = run_rustloc_with_code(&[".", "--total-fsdgte", "1300"]);
    assert_eq!(code, Some(2), "expected exit 2, got {:?}", code);
    assert!(
        stderr.contains("error:") && stderr.contains("unexpected argument"),
        "expected clap error on stderr, got stdout={:?} stderr={:?}",
        stdout,
        stderr
    );
    assert!(
        stdout.is_empty(),
        "stdout should be empty on parse error, got {:?}",
        stdout
    );
}

#[test]
fn test_filter_unknown_flag_errors_clearly() {
    let (stdout, stderr, code) = run_rustloc_with_code(&[".", "--code-foo", "100"]);
    assert_eq!(code, Some(2), "expected exit 2, got {:?}", code);
    assert!(
        stderr.contains("error:") && stderr.contains("unexpected argument"),
        "expected clap error on stderr, got stdout={:?} stderr={:?}",
        stdout,
        stderr
    );
}

#[test]
fn test_filter_malformed_double_dash_in_flag_errors() {
    // `--total--fsdgte` is well-formed at the shell level (passed through
    // verbatim) but isn't a valid clap long-flag name and isn't in our
    // registered set. Must still fail loud rather than silently no-op.
    let (stdout, stderr, code) = run_rustloc_with_code(&[".", "--total--fsdgte", "1300"]);
    assert_eq!(code, Some(2), "expected exit 2, got {:?}", code);
    assert!(
        stderr.contains("error:") && stderr.contains("unexpected argument"),
        "expected clap error on stderr, got stdout={:?} stderr={:?}",
        stdout,
        stderr
    );
}

#[test]
fn test_filter_non_numeric_value_errors_clearly() {
    let (stdout, stderr, code) = run_rustloc_with_code(&[".", "--code-gte", "abc"]);
    assert_eq!(code, Some(2), "expected exit 2, got {:?}", code);
    assert!(
        stderr.contains("error:")
            && (stderr.contains("invalid value") || stderr.contains("invalid digit")),
        "expected clap value error on stderr, got stdout={:?} stderr={:?}",
        stdout,
        stderr
    );
}

#[test]
fn test_filter_works_on_diff() {
    // The filter args are also injected on the diff subcommand.
    let fixture = fixture_path_str();
    let (stdout, _, success) = run_rustloc(&[
        "diff",
        "HEAD~5..HEAD",
        "--path",
        &fixture,
        "--by-file",
        "--code-gte",
        "0",
    ]);

    assert!(success);
    assert!(stdout.contains("Diff:"));
}

#[test]
fn test_filter_json_carries_filter_results() {
    // Structured output should also honour the filter.
    let (stdout, _, success) =
        run_rustloc(&[".", "--by-crate", "--code-gte", "100", "--output", "json"]);

    assert!(success);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");
    let items = parsed["items"].as_array().expect("items should be array");

    // Every surviving item must satisfy the predicate.
    for item in items {
        let code = item["stats"]["code"]
            .as_u64()
            .expect("stats.code should be numeric");
        assert!(code >= 100, "filter violated: code={} < 100", code);
    }

    // total_items reflects the pre-filter count (still 2 crates in this repo).
    let total_items = parsed["total_items"].as_u64().unwrap();
    assert_eq!(total_items, 2);
    // top_applied stays false when only --filter was used.
    assert_eq!(parsed["top_applied"].as_bool(), Some(false));
}

#[test]
fn test_filter_repeated_same_flag_combines_with_and() {
    // `--code-gte 100 --code-gte 200` is a tautology of "code >= 100 AND
    // code >= 200" which simplifies to "code >= 200". Both predicates are
    // applied so the binary must accept the repetition.
    let (_, _, success) =
        run_rustloc(&[".", "--by-file", "--code-gte", "100", "--code-gte", "200"]);

    assert!(success);
}
