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
    assert!(stdout.contains("--format"));
    assert!(stdout.contains("--per-crate"));
    assert!(stdout.contains("--per-file"));
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
    let (stdout, _, success) = run_rustloc(&[".", "--format", "json"]);

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
    let (stdout, _, success) = run_rustloc(&[".", "--format", "csv"]);

    assert!(success);
    assert!(stdout.contains("type,name,code,blanks,docs,comments,total"));
    assert!(stdout.contains("main,total,"));
    assert!(stdout.contains("tests,total,"));
    assert!(stdout.contains("total,total,"));
}

#[test]
fn test_per_crate_output() {
    let (stdout, _, success) = run_rustloc(&[".", "--per-crate"]);

    assert!(success);
    assert!(stdout.contains("Per-crate breakdown:"));
    assert!(stdout.contains("rustloclib"));
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
