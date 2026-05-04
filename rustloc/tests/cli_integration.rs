//! Integration tests for rustloc CLI

use std::process::Command;

fn run_rustloc(args: &[&str]) -> (String, String, bool) {
    let mut cmd_args = vec!["run", "-p", "rustloc", "--"];
    cmd_args.extend(args);

    let output = Command::new("cargo")
        .args(&cmd_args)
        .current_dir(env!("CARGO_MANIFEST_DIR").to_string() + "/..")
        .output()
        .expect("Failed to execute command");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let success = output.status.success();

    (stdout, stderr, success)
}

#[test]
fn test_cli_help() {
    let (stdout, _, success) = run_rustloc(&["--help"]);

    assert!(success);
    assert!(stdout.contains("rustloc"));
    assert!(stdout.contains("--crate"));
    assert!(stdout.contains("--output"));
    assert!(stdout.contains("--by-crate"));
    assert!(stdout.contains("--by-file"));
}

#[test]
fn test_cli_version() {
    let (stdout, _, success) = run_rustloc(&["--version"]);

    assert!(success);
    assert!(stdout.contains("rustloc"));
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

// CSV output removed - using outstanding's built-in JSON/table output

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
}

#[test]
fn test_diff_table_output() {
    // Use known commits from the repo
    let (stdout, _, success) = run_rustloc(&["diff", "HEAD~5..HEAD"]);

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
    let (stdout, _, success) = run_rustloc(&["diff", "HEAD~5", "HEAD"]);

    assert!(success);
    assert!(stdout.contains("Diff:"));
    // Total row shows file count in row name (same layout as counts)
    assert!(stdout.contains("Total (") && stdout.contains("files)"));
}

#[test]
fn test_diff_annotated_tag_range() {
    // Annotated tags must be peeled to their target commit by the resolver.
    // This used to fail with "expected commit, got tag" before delegating
    // resolution to gix::Repository::rev_parse + peel_to_commit.
    let (stdout, _, success) = run_rustloc(&["diff", "v0.14.0..v0.14.2"]);

    assert!(success, "tag-to-tag diff should resolve via gix rev_parse");
    assert!(stdout.contains("Diff: v0.14.0"));
    assert!(stdout.contains("v0.14.2"));
}

#[test]
fn test_diff_single_tag_against_head() {
    // A single revspec is diffed against HEAD.
    let (stdout, _, success) = run_rustloc(&["diff", "v0.14.2"]);

    assert!(success);
    assert!(stdout.contains("Diff: v0.14.2"));
    assert!(stdout.contains("HEAD"));
}

#[test]
fn test_diff_rejects_range_with_extra_arg() {
    // `rustloc diff a..b c` would naively become `a..b..c`, an invalid
    // revspec. The CLI should detect and reject this with a clear error.
    let (_, stderr, success) = run_rustloc(&["diff", "HEAD~1..HEAD", "HEAD"]);

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
    let (stdout, _, success) = run_rustloc(&["diff", "v0.14.0...v0.14.2"]);

    assert!(success);
    assert!(stdout.contains("merge-base"));
}

#[test]
fn test_diff_json_output() {
    let (stdout, _, success) = run_rustloc(&["diff", "HEAD~5..HEAD", "--output", "json"]);

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
    let (stdout, _, success) = run_rustloc(&["diff", "HEAD~5..HEAD", "--by-file"]);

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
    let (_, stderr, success) = run_rustloc(&["diff", "invalid_commit_hash"]);

    assert!(!success);
    assert!(stderr.contains("Error:"));
}

#[test]
fn test_diff_same_commit() {
    // Diffing a commit against itself should show no changes
    let (stdout, _, success) = run_rustloc(&["diff", "HEAD..HEAD"]);

    assert!(success);
    assert!(stdout.contains("Total (0 files)"));
}

// ============================================================================
// Working directory diff tests
// ============================================================================

#[test]
fn test_diff_workdir() {
    // Diff without commit args should show working directory changes
    let (stdout, _, success) = run_rustloc(&["diff"]);

    assert!(success, "diff without args should succeed");
    assert!(stdout.contains("Diff: HEAD"));
    assert!(stdout.contains("working tree"));
    // Total row shows file count in row name (same layout as counts)
    assert!(stdout.contains("Total (") && stdout.contains("files)"));
}

#[test]
fn test_diff_workdir_staged() {
    // Diff with --staged should show staged changes vs HEAD
    let (stdout, _, success) = run_rustloc(&["diff", "--staged"]);

    assert!(success, "diff --staged should succeed");
    assert!(stdout.contains("Diff: HEAD"));
    assert!(stdout.contains("index"));
    // Total row shows file count in row name (same layout as counts)
    assert!(stdout.contains("Total (") && stdout.contains("files)"));
}

#[test]
fn test_diff_workdir_cached_alias() {
    // --cached should work as an alias for --staged
    let (stdout, _, success) = run_rustloc(&["diff", "--cached"]);

    assert!(success, "diff --cached should succeed");
    assert!(stdout.contains("Diff: HEAD"));
    assert!(stdout.contains("index"));
}

#[test]
fn test_diff_workdir_json() {
    let (stdout, _, success) = run_rustloc(&["diff", "--output", "json"]);

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
    let (stdout, _, success) = run_rustloc(&["diff", "--by-file"]);

    assert!(success);
    // Should still show working tree diff
    assert!(stdout.contains("working tree"));
}

#[test]
fn test_diff_staged_with_commits_error() {
    // Using --staged with commit args should fail
    let (_, stderr, success) = run_rustloc(&["diff", "HEAD~1", "--staged"]);

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
    let (stdout, _, success) = run_rustloc(&["diff", "HEAD~5..HEAD", "--by-file", "--top", "2"]);

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
    let (stdout, _, success) = run_rustloc(&[
        "diff",
        "HEAD~5..HEAD",
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
