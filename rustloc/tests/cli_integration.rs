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
    // Check for context column headers in default view (code, tests, docs, all)
    assert!(stdout.contains("Code"));
    assert!(stdout.contains("Tests"));
    assert!(stdout.contains("Docs"));
    assert!(stdout.contains("All"));
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
    assert!(total["all"].is_u64(), "all should be a number");
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
    assert!(stdout.contains("rustloclib::data::counter"));
    assert!(stdout.contains("rustloclib::data::diff"));
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
    assert!(stdout.contains("Base commit or range"));
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
    assert!(stdout.contains("All"));
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
