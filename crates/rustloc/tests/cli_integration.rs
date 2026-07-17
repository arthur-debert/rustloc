//! Process-only contracts for the `rustloc` executable.
//!
//! This is the top, intentionally tiny layer of the test pyramid. Command
//! parsing and orchestration are tested as typed functions; clap routing,
//! handlers, Standout presentation, templates, themes, and serializers are
//! tested in-process through `App::run_to_string` in `src/pipeline_tests.rs`.
//! A test belongs here only when the operating-system process is observable:
//!
//! - the packaged binary starts and links;
//! - `main` maps Standout results to exit codes and stdout/stderr;
//! - Standout performs the final output-file write;
//! - the child process's ambient environment or working directory matters.
//!
//! Real files and Git repositories are ordinary application inputs, not a
//! reason to spawn the executable. Their behavior is covered below this layer.

use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

/// Invoke the already-built integration-test binary.
///
/// Using Cargo's `CARGO_BIN_EXE_*` path proves the real binary artifact while
/// avoiding a nested `cargo run` (and its build lock) for every assertion.
fn rustloc(args: &[&str], cwd: &Path, env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rustloc"));
    command.args(args).current_dir(cwd);
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().expect("rustloc executable should start")
}

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("rustloc crate should live below the workspace root")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Process-only contract: the built and linked executable reaches `main` and
/// writes its version to the real stdout stream.
#[test]
fn packaged_binary_starts_and_reports_its_version() {
    let output = rustloc(&["--version"], workspace_root(), &[]);

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(stdout(&output).starts_with("rustloc "));
    assert!(output.stderr.is_empty());
}

/// Process-only contract: a relative path is resolved from the child's actual
/// working directory, a seam `run_to_string` tests deliberately avoid mutating.
#[test]
fn dot_resolves_from_the_child_process_working_directory() {
    let dir = TempDir::new().expect("source fixture");
    std::fs::write(dir.path().join("only.rs"), "pub fn only() {}\n").unwrap();

    let output = rustloc(&[".", "--output", "json"], dir.path(), &[]);

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(output.stderr.is_empty());
    let response: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid count response");
    assert_eq!(response["file_count"], 1);
    assert_eq!(response["total"]["code"], 1);
}

/// Process-only contract: Standout's final writer creates the requested file
/// and does not duplicate those bytes to the executable's stdout stream.
#[test]
fn output_file_path_writes_the_file_and_suppresses_stdout() {
    let dir = TempDir::new().expect("output fixture");
    std::fs::write(dir.path().join("only.rs"), "pub fn only() {}\n").unwrap();
    let report = dir.path().join("report.json");
    let report_arg = report.to_string_lossy().into_owned();

    let output = rustloc(
        &[".", "--output", "json", "--output-file-path", &report_arg],
        dir.path(),
        &[],
    );

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(
        output.stdout.is_empty(),
        "unexpected stdout: {}",
        stdout(&output)
    );
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {}",
        stderr(&output)
    );
    let bytes = std::fs::read(&report).expect("Standout should write report.json");
    let response: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON file");
    assert_eq!(response["file_count"], 1);
}

/// Process-only contract: `main` maps clap/Standout parse errors to exit 2,
/// writes the diagnostic only to stderr, and leaves stdout safe for pipelines.
/// Parsing details and every route are covered by the in-process pipeline.
#[test]
fn usage_errors_exit_two_on_stderr_without_stdout() {
    let output = rustloc(
        &[".", "--by-crate", "--ordering", "coed"],
        workspace_root(),
        &[],
    );

    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.is_empty(),
        "unexpected stdout: {}",
        stdout(&output)
    );
    let error = stderr(&output);
    assert!(error.contains("error:"), "unexpected stderr: {error}");
    assert!(
        error.contains("Unknown order field: coed"),
        "unexpected stderr: {error}"
    );
}

/// Process-only contract: an application failure follows the executable's
/// established error mapping and stream ownership. The direct/pipeline layers
/// separately prove which invalid path produces the failure and its wording.
#[test]
fn application_errors_exit_two_on_stderr_without_stdout() {
    let dir = TempDir::new().expect("missing-path fixture");
    let missing = dir.path().join("does-not-exist");
    let missing_arg = missing.to_string_lossy().into_owned();

    let output = rustloc(&[&missing_arg], workspace_root(), &[]);

    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.is_empty(),
        "unexpected stdout: {}",
        stdout(&output)
    );
    assert!(stderr(&output).contains("Error:"));
}

/// Process-only contract: terminal capability is ambient child state. With
/// forced colour, semantic tags become the CSS theme's concrete ANSI styling
/// and no raw tag reaches the user's stream.
#[test]
fn term_output_is_ansi_when_colour_is_forced() {
    let output = rustloc(
        &[".", "--output", "term"],
        workspace_root(),
        &[("CLICOLOR_FORCE", "1")],
    );
    let rendered = stdout(&output);

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(
        rendered.contains('\x1b'),
        "forced term output had no ANSI: {rendered:?}"
    );
    assert!(
        rendered.contains("\x1b[36m") && rendered.contains("\x1b[1m"),
        "header lost its cyan/bold style: {rendered:?}"
    );
    assert!(
        !strip_sgr(&rendered).contains('['),
        "a semantic style tag leaked into terminal output: {rendered:?}"
    );
}

/// Strip the SGR sequences rustloc emits, leaving the text a person reads.
fn strip_sgr(input: &str) -> String {
    let mut visible = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(character) = chars.next() {
        if character != '\x1b' {
            visible.push(character);
            continue;
        }
        for character in chars.by_ref() {
            if character == 'm' {
                break;
            }
        }
    }
    visible
}
