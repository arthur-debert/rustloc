//! Process-level tests for the rustloc CLI: the boundaries a real executable
//! owns and no in-process test can model.
//!
//! This is the **top** of the pyramid, and it is deliberately the thinnest
//! layer. Anything argv can express — routing, parsing, presentation, output
//! modes, templates — is covered in-process by `src/pipeline_tests.rs`, which
//! drives the same app through the same entry point `main` calls, in
//! milliseconds instead of a `cargo run` per assertion. Below that,
//! `src/command.rs` and `src/application.rs` test parsing and orchestration as
//! plain functions.
//!
//! ## What still earns a process test
//!
//! Each remaining test protects one of these, and nothing else:
//!
//! - **Exit codes.** `RunResult` → `ExitCode` mapping only exists in `main`.
//!   A clap usage error must exit **2** and a library failure **1**; both have
//!   regressed before (standout ≥7.6 routes parse errors to
//!   `RunResult::Error`, and without that arm they were swallowed silently at
//!   exit 0). In-process tests see the `RunResult`, never the exit code.
//! - **Stream routing.** Errors must land on **stderr** and leave stdout
//!   empty, so `rustloc ... | jq` fails loudly rather than parsing a message.
//!   `run_to_string` returns one string and cannot distinguish the streams.
//! - **Executable integration.** That the binary builds, links, and runs at
//!   all (`--version`), and that `.` resolves against the real process cwd.
//! - **Real Git behavior.** Revspec resolution, tag peeling, and merge-base
//!   ranges against an actual repository — gix territory that a fabricated
//!   fixture cannot faithfully stand in for.
//! - **Table line structure (issue #84).** Kept at this layer *despite* the
//!   in-process template coverage, because the regression it guards shipped
//!   (v0.17.1) — the documented exception to "don't test the same fact twice".
//!
//! Adding a test here that asserts none of the above means it belongs one
//! layer down.

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
    run_rustloc_with_env(args, &[])
}

/// Like `run_rustloc_with_code` but sets environment variables on the child.
///
/// This is the whole reason the colour test lives at the process layer. Colour
/// capability is not something argv can say: `console` reads it from the
/// process, so forcing it in-process would mean mutating global state and
/// costing `pipeline_tests` its parallelism. A spawned child owns its own
/// environment, so the ambient seam gets exercised without any test in this
/// workspace touching a global.
fn run_rustloc_with_env(args: &[&str], env: &[(&str, &str)]) -> (String, String, Option<i32>) {
    let mut cmd_args = vec!["run", "--quiet", "-p", "rustloc", "--"];
    cmd_args.extend(args);

    let mut command = Command::new("cargo");
    command.args(&cmd_args);
    for (key, value) in env {
        command.env(key, value);
    }

    let output = command
        // Run from the workspace root so `.` resolves to the 2-crate
        // workspace these tests assert against. This crate lives at
        // `<root>/crates/rustloc`, so the root is two levels up.
        .current_dir(env!("CARGO_MANIFEST_DIR").to_string() + "/../..")
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

// ============================================================================
// Forced-colour term output
// ============================================================================
//
// The one rendering mode `pipeline_tests` cannot cover, and the only test in
// this workspace that reads ANSI. Both facts have the same cause: `--output
// term` asks for colour, but whether colour is *emitted* is decided by
// `console` reading the process — piped stdout means no ANSI, however loudly
// argv asked. `CLICOLOR_FORCE=1` is the override, and it only exists as an
// environment variable, which is why this needs a child process.
//
// This is also the only place the two halves of the theme meet end to end: that
// tags reach the parser (term-debug's job), and that the CSS gives them
// attributes (the theme test's job), are each pinned in isolation — a wiring
// mistake between them would show up only here.

/// Forced `term` output paints the semantic tags and leaves none of them behind.
///
/// Three distinct failures are covered, and it takes all three assertions:
/// escape bytes prove the theme reached the output at all; the specific SGR
/// codes prove the *right* styles landed (a theme resolving to empty styles
/// still emits nothing, but so does a colour-blind pipe — only the codes tell
/// them apart); and the absence of `[` markers proves the parser consumed the
/// tags rather than printing them at a user.
#[test]
fn term_output_is_ansi_when_colour_is_forced() {
    let (stdout, _, code) = run_rustloc_with_env(&["--output", "term"], &[("CLICOLOR_FORCE", "1")]);
    assert_eq!(code, Some(0), "forced-colour term run failed:\n{stdout}");

    assert!(
        stdout.contains('\x1b'),
        "term mode emitted no ANSI even with CLICOLOR_FORCE=1:\n{stdout:?}"
    );
    // The header is cyan (SGR 36) + bold (SGR 1) per styles/default.css. It is
    // the one styled tag the count table always renders, whatever the workspace.
    assert!(
        stdout.contains("\x1b[36m") && stdout.contains("\x1b[1m"),
        "the header lost its cyan/bold styling:\n{stdout:?}"
    );

    // No tag — known or unknown — may survive into what a user reads. An unknown
    // one arrives as `[header?]`, so this catches a typo'd tag as well as an
    // unconsumed one. The escapes have to come out first: an SGR sequence is
    // itself `ESC [ ... m`, so a bare search for `[` would match the very
    // styling the assertions above just demanded.
    let visible = strip_ansi(&stdout);
    assert!(
        !visible.contains('['),
        "a raw style tag leaked into term output:\n{visible}"
    );
}

/// Remove SGR escape sequences (`ESC [ ... m`), leaving what a user reads.
///
/// Deliberately minimal: rustloc emits nothing but SGR, so a full ANSI grammar
/// would be more code to be wrong about. Anything unexpected survives the strip
/// and shows up in an assertion rather than being quietly swallowed.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        // Consume through the terminating 'm'. An unterminated sequence just
        // ends the string, which is fine — there is nothing left to keep.
        for c in chars.by_ref() {
            if c == 'm' {
                break;
            }
        }
    }
    out
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

// ============================================================================
// Table line-structure regression tests (issue #84)
// ============================================================================
//
// The table layout has silently regressed more than once: a stray Jinja
// whitespace-trim marker (`{#-` in count_table.jinja, commit 4a4efd7) ate
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
    assert!(stdout.contains("rustloclib::query"));
    assert!(stdout.contains("rustloclib::source"));
    // The library's pipeline ends at the query stage: presentation lives in
    // this crate's `table` module, not in a `rustloclib::output` one.
    assert!(!stdout.contains("rustloclib::output"));
    assert!(stdout.contains("rustloc::table"));
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
    // Only the `rustloc` crate's own files are counted: src/main.rs,
    // src/app.rs, src/command.rs, src/application.rs, src/table.rs,
    // src/pipeline_tests.rs, and tests/cli_integration.rs. Bump this when the
    // crate gains a file. Asserting the footer (rather than a bare
    // `contains("2")`, which any stray digit satisfies) is what actually
    // proves the filter ran.
    assert!(
        stdout.contains("Total (7 files)"),
        "expected the crate filter to narrow to rustloc's 7 files, got:\n{stdout}"
    );
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
        run_rustloc(&[".", "--by-crate", "--code-gte", "50", "--tests-lt", "2400"]);

    assert!(success);
    // The repo has 2 crates; the AND should keep the rustloc crate (which
    // has < 2400 tests) and exclude rustloclib (which has more). The
    // threshold is a real count of this repo, so it drifts as the crates
    // grow: it sits roughly midway between the two crates' test counts to
    // leave headroom on both sides. Re-centre it if either crate crosses.
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

// ============================================================================
// Canonical response contract (issue #119)
// ============================================================================
//
// Handlers return exactly ONE typed response per command, regardless of
// `--output`. Presentation — table, CSV rows, direct serialization — is
// resolved at the render boundary. These tests pin that contract from the
// outside: the observable proof that the response is mode-independent is that
// every structured mode reports the same data, and that a display-only flag
// (`--type`) never changes a number.
//
// A deterministic sample tree keeps the compatibility fixtures from drifting
// with the repo's own line counts (`rustloc .` moves with every commit).

/// A fixed source tree with known counts: production code, an inline test
/// module, doc comments, plain comments, and blank lines. Built once per test
/// binary in a TempDir so nothing in the repo can perturb it.
fn sample_tree() -> &'static Path {
    static SAMPLE: OnceLock<TempDir> = OnceLock::new();
    SAMPLE
        .get_or_init(|| {
            let dir = TempDir::new().expect("create sample tree");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).expect("create src");

            // Counts (pinned by tests/fixtures/count_by_file.after.json):
            // 6 code, 8 tests, 2 docs, 1 comment, 3 blanks — 20 total.
            std::fs::write(
                src.join("lib.rs"),
                "\
/// Adds two numbers.
/// Deliberately trivial.
pub fn add(a: u64, b: u64) -> u64 {
    a + b
}

// A plain comment.
pub fn double(x: u64) -> u64 {
    x * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds() {
        assert_eq!(add(1, 2), 3);
    }
}
",
            )
            .expect("write lib.rs");

            std::fs::write(
                src.join("util.rs"),
                "\
/// A helper.
pub fn noop() {}
",
            )
            .expect("write util.rs");

            dir
        })
        .path()
}

fn sample_path_str() -> String {
    sample_tree().to_string_lossy().to_string()
}

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {path:?}: {e}"))
}

/// json and yaml serialize the handler's value directly, with no adapter in
/// between, so both must decode to the *same data* — byte equality is not the
/// claim, since each mode has its own syntax. Parses both with their real
/// parsers and compares the decoded values. If a handler ever branched on
/// output mode again, these would drift. (xml is the same contract, but needs
/// a different parser: see the test below.)
#[test]
fn test_canonical_response_identical_across_structured_modes() {
    let path = sample_path_str();

    let (json_out, _, ok) = run_rustloc(&[&path, "--by-file", "--output", "json"]);
    assert!(ok);
    let (yaml_out, _, ok) = run_rustloc(&[&path, "--by-file", "--output", "yaml"]);
    assert!(ok);

    // Parse each with its own real parser, then compare as data.
    let from_json: serde_json::Value =
        serde_json::from_str(&json_out).expect("json mode must emit valid JSON");
    let from_yaml: serde_json::Value =
        serde_yaml::from_str(&yaml_out).expect("yaml mode must emit valid YAML");

    assert_eq!(
        from_json, from_yaml,
        "json and yaml must serialize the same canonical response"
    );
}

/// XML is generated by a different serializer, so parse it with a real XML
/// reader and confirm the same numbers survive the trip.
#[test]
fn test_xml_mode_is_well_formed_and_carries_the_same_totals() {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let path = sample_path_str();
    let (json_out, _, ok) = run_rustloc(&[&path, "--output", "json"]);
    assert!(ok);
    let (xml_out, _, ok) = run_rustloc(&[&path, "--output", "xml"]);
    assert!(ok);

    let json: serde_json::Value = serde_json::from_str(&json_out).unwrap();

    // Walk the XML with a real parser: it must be well-formed, and the
    // <total><code> value must match JSON's total.code.
    let mut reader = Reader::from_str(&xml_out);
    let mut path_stack: Vec<String> = Vec::new();
    let mut found: Option<u64> = None;
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .expect("XML must be well-formed")
        {
            Event::Start(e) => path_stack.push(String::from_utf8_lossy(e.name().as_ref()).into()),
            Event::End(_) => {
                path_stack.pop();
            }
            Event::Text(e) => {
                if path_stack.ends_with(&["total".to_string(), "code".to_string()]) {
                    found = e.unescape().unwrap().parse::<u64>().ok();
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    let xml_code = found.expect("xml must contain total/code");
    assert_eq!(
        xml_code,
        json["total"]["code"].as_u64().unwrap(),
        "xml and json must report the same total.code"
    );
}

/// `--type` is a *view* flag. It selects columns in the human table; it must
/// never change a number in structured output, because the handler no longer
/// knows what the output mode is.
#[test]
fn test_type_flag_never_changes_structured_numbers() {
    let path = sample_path_str();

    let (full, _, ok) = run_rustloc(&[&path, "--by-file", "--output", "json"]);
    assert!(ok);
    let (narrow, _, ok) = run_rustloc(&[&path, "--by-file", "--output", "json", "-t", "code"]);
    assert!(ok);

    let mut full: serde_json::Value = serde_json::from_str(&full).unwrap();
    let mut narrow: serde_json::Value = serde_json::from_str(&narrow).unwrap();

    // `line_types` is the one field that legitimately differs: it records the
    // requested view. Everything else — every count — must be identical.
    assert_ne!(full["line_types"], narrow["line_types"]);
    full.as_object_mut().unwrap().remove("line_types");
    narrow.as_object_mut().unwrap().remove("line_types");
    assert_eq!(
        full, narrow,
        "--type must not change any count in structured output"
    );
}

/// Regression for the mode-dependence bug this contract removes: predicates
/// used to run against display-filtered stats, so filtering on a line type
/// that `--type` didn't display silently matched nothing in the table — while
/// the identical query worked under `--output json`.
#[test]
fn test_filter_on_undisplayed_line_type_matches_in_table_mode() {
    let path = sample_path_str();

    // `blanks` is not in the default displayed set. lib.rs has blank lines;
    // util.rs has none. The predicate must still see the real counts.
    let (stdout, _, ok) =
        run_rustloc(&[&path, "--by-file", "--blanks-gte", "1", "--output", "text"]);
    assert!(ok);
    assert!(
        stdout.contains("lib.rs"),
        "blanks predicate must match on real counts in table mode; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("util.rs"),
        "util.rs has no blank lines and must not match; got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Checked-in compatibility fixtures
// ---------------------------------------------------------------------------
//
// These pin the public JSON/CSV output for the deterministic sample tree: JSON
// as decoded data (key order is not part of the contract), CSV as exact bytes
// (its column order is). See tests/fixtures/README.md for the before/after
// rationale — the only intentional change in this workstream is the
// `line_types` metadata field.

#[test]
fn test_count_json_matches_compat_fixture() {
    let path = sample_path_str();
    let (stdout, _, ok) = run_rustloc(&[&path, "--by-file", "--output", "json"]);
    assert!(ok);

    let actual: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(&fixture("count_by_file.after.json")).unwrap();
    assert_eq!(
        actual, expected,
        "public count JSON changed unintentionally"
    );
}

#[test]
fn test_count_csv_matches_compat_fixture() {
    let path = sample_path_str();
    let (stdout, _, ok) = run_rustloc(&[&path, "--by-file", "--output", "csv"]);
    assert!(ok);
    assert_eq!(
        stdout,
        fixture("count_by_file.csv"),
        "public count CSV changed unintentionally"
    );
}

// ============================================================================
// Strict ordering parsing (issue #121)
// ============================================================================
//
// `--ordering` is parsed by a clap `value_parser`, so an unknown field is a
// usage error *before* dispatch. The bug this guards: ordering used to be
// parsed inside the handler with `.unwrap_or_default()`, so `-o -coed`
// silently sorted by label and exited 0 — a script asking for the wrong field
// got plausible output and no signal. These tests assert the three things a
// caller can observe: exit 2, clap's message on stderr, and empty stdout.

/// Every prefix form of an unknown field must be rejected: plain, `-`
/// (descending), and `+` (ascending). The prefixed forms matter most —
/// `allow_hyphen_values` lets them reach the parser as values rather than
/// flags, so they are exactly where a swallowed error could hide.
#[test]
fn test_invalid_ordering_is_a_usage_error() {
    for bad in ["coed", "-coed", "+coed"] {
        let (stdout, stderr, code) = run_rustloc_with_code(&[".", "--by-crate", "-o", bad]);

        assert_eq!(code, Some(2), "`-o {bad}` should exit 2, got {code:?}");
        assert!(
            stderr.contains("error:") && stderr.contains("--ordering"),
            "`-o {bad}` should name --ordering on stderr, got {stderr:?}"
        );
        assert!(
            stdout.is_empty(),
            "`-o {bad}` must produce no stdout, got {stdout:?}"
        );
    }
}

/// The invalid-value message names the offending value and the field, so the
/// user can see what to fix rather than just that something was wrong.
#[test]
fn test_invalid_ordering_message_names_the_bad_value() {
    let (_, stderr, _) = run_rustloc_with_code(&[".", "-o", "coed"]);
    assert!(
        stderr.contains("invalid value 'coed'") && stderr.contains("Unknown order field: coed"),
        "expected a message naming the bad value, got {stderr:?}"
    );
}
