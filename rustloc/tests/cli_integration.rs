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
    assert!(stdout.contains("File count:"));
    assert!(stdout.contains("Main"));
    assert!(stdout.contains("Tests"));
    assert!(stdout.contains("Examples"));
    assert!(stdout.contains("Code"));
    assert!(stdout.contains("Blank"));
}

#[test]
fn test_json_output() {
    let (stdout, _, success) = run_rustloc(&[".", "--output", "json"]);

    assert!(success);
    assert!(stdout.contains("\"file_count\""));
    assert!(stdout.contains("\"totals\""));
    assert!(stdout.contains("\"main\""));
    assert!(stdout.contains("\"tests\""));
    assert!(stdout.contains("\"examples\""));

    // Verify it's valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");
    assert!(parsed.get("file_count").is_some());
    assert!(parsed.get("totals").is_some());
}

#[test]
fn test_csv_output() {
    let (stdout, _, success) = run_rustloc(&[".", "--output", "csv"]);

    assert!(success);
    assert!(stdout.contains("type,name,code,blanks,docs,comments,total"));
    assert!(stdout.contains("main,total,"));
    assert!(stdout.contains("tests,total,"));
    assert!(stdout.contains("total,total,"));
}

#[test]
fn test_by_crate_output() {
    let (stdout, _, success) = run_rustloc(&[".", "--by-crate"]);

    assert!(success);
    assert!(stdout.contains("By-crate breakdown:"));
    assert!(stdout.contains("rustloclib"));
    assert!(stdout.contains("rustloc"));
    assert!(stdout.contains("Total ("));
}

#[test]
fn test_by_module_output() {
    let (stdout, _, success) = run_rustloc(&[".", "--by-module"]);

    assert!(success);
    assert!(stdout.contains("By-module breakdown:"));
    assert!(stdout.contains("rustloclib::counter"));
    assert!(stdout.contains("rustloclib::diff"));
    assert!(stdout.contains("rustloc"));
    assert!(stdout.contains("Total ("));
}

#[test]
fn test_crate_filter() {
    let (stdout, _, success) = run_rustloc(&[".", "--crate", "rustloc"]);

    assert!(success);
    assert!(stdout.contains("File count: 2")); // Only rustloc crate has 2 files
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
    assert!(stdout.contains("Files changed:"));
    assert!(stdout.contains("Main"));
    assert!(stdout.contains("Tests"));
    assert!(stdout.contains("Examples"));
    // Check for the +x/-y/z format
    assert!(stdout.contains("+") && stdout.contains("/-"));
}

#[test]
fn test_diff_with_separate_commits() {
    // Test using two separate commit arguments
    let (stdout, _, success) = run_rustloc(&["diff", "HEAD~5", "HEAD"]);

    assert!(success);
    assert!(stdout.contains("Diff:"));
    assert!(stdout.contains("Files changed:"));
}

#[test]
fn test_diff_json_output() {
    let (stdout, _, success) = run_rustloc(&["diff", "HEAD~5..HEAD", "--output", "json"]);

    assert!(success);
    assert!(stdout.contains("\"from_commit\""));
    assert!(stdout.contains("\"to_commit\""));
    assert!(stdout.contains("\"files_changed\""));
    assert!(stdout.contains("\"totals\""));
    assert!(stdout.contains("\"added\""));
    assert!(stdout.contains("\"removed\""));
    assert!(stdout.contains("\"net\""));

    // Verify it's valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");
    assert!(parsed.get("from_commit").is_some());
    assert!(parsed.get("to_commit").is_some());
    assert!(parsed.get("files_changed").is_some());
}

#[test]
fn test_diff_csv_output() {
    let (stdout, _, success) = run_rustloc(&["diff", "HEAD~5..HEAD", "--output", "csv"]);

    assert!(success);
    assert!(stdout.contains("type,name,change,code_added,code_removed,code_net"));
    assert!(stdout.contains("main,total,"));
    assert!(stdout.contains("tests,total,"));
    assert!(stdout.contains("total,total,"));
}

#[test]
fn test_diff_by_file() {
    let (stdout, _, success) = run_rustloc(&["diff", "HEAD~5..HEAD", "--by-file"]);

    assert!(success);
    assert!(stdout.contains("By-file breakdown:"));
    assert!(stdout.contains("Change"));
    // Should show file change markers
    assert!(stdout.contains("M") || stdout.contains("+") || stdout.contains("-"));
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
    assert!(stdout.contains("Files changed: 0"));
}
