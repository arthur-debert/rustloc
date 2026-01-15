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
    assert!(stdout.contains("\"headers\""));
    assert!(stdout.contains("\"rows\""));
    assert!(stdout.contains("\"footer\""));
    assert!(stdout.contains("\"Code\""));
    assert!(stdout.contains("\"Tests\""));
    assert!(stdout.contains("\"Docs\""));
    assert!(stdout.contains("\"All\""));

    // Verify it's valid JSON with LOCTable structure
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");
    assert!(parsed.get("headers").is_some());
    assert!(parsed.get("footer").is_some());
    assert!(parsed["footer"].get("label").is_some());
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
    assert!(stdout.contains("Commit range"));
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
    // Check for the +x/-y/z format
    assert!(stdout.contains("+") && stdout.contains("/-"));
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
    assert!(stdout.contains("\"title\""));
    assert!(stdout.contains("\"headers\""));
    assert!(stdout.contains("\"footer\""));
    // Diff title contains commit range
    assert!(stdout.contains("HEAD~5"));
    assert!(stdout.contains("HEAD"));

    // Verify it's valid JSON with LOCTable structure
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");
    assert!(parsed.get("title").is_some());
    assert!(parsed.get("headers").is_some());
    assert!(parsed.get("footer").is_some());
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
    // Should show diff format (+x/-y/z)
    assert!(stdout.contains("+") && stdout.contains("/-"));
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
    assert!(stdout.contains("\"title\""));
    assert!(stdout.contains("\"headers\""));
    assert!(stdout.contains("\"footer\""));
    // Title contains HEAD and working tree
    assert!(stdout.contains("HEAD"));
    assert!(stdout.contains("working tree"));

    // Verify it's valid JSON with LOCTable structure
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");
    assert!(parsed.get("title").is_some());
    assert!(parsed["title"].as_str().unwrap().contains("HEAD"));
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
