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

use num_format::{Locale, ToFormattedString};
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

fn active_format_u64(value: u64) -> String {
    let locale = sys_locale::get_locale()
        .and_then(|name| {
            let normalized = name
                .split(['.', '@'])
                .next()
                .unwrap_or(name.as_str())
                .to_string();
            Locale::from_name(&normalized)
                .or_else(|_| {
                    Locale::from_name(normalized.split(['-', '_']).next().unwrap_or(&normalized))
                })
                .ok()
        })
        .unwrap_or(Locale::en);
    value.to_formatted_string(&locale)
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .output()
        .expect("git executable should start");
    assert!(
        output.status.success(),
        "git {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn query_repo() -> TempDir {
    let dir = TempDir::new().expect("query repo");
    let root = dir.path();
    git(root, &["init", "-q"]);
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"query-repo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
    std::fs::write(root.join("src/small.rs"), "pub fn small() {}\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "baseline"]);

    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn a() {}\npub fn b() {}\npub fn c() {}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/small.rs"),
        "pub fn small() {}\n\n#[test]\nfn keeps_tests_visible() {}\n",
    )
    .unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "change"]);
    dir
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

#[test]
fn diff_shared_query_options_match_before_and_after_the_subcommand() {
    let dir = query_repo();
    let path = dir.path().to_str().unwrap();

    let prefix = rustloc(
        &[
            "--type",
            "code",
            "--lang",
            "rust",
            "--by-file",
            "--ordering",
            "-code",
            "--code-gte",
            "1",
            "--top",
            "2",
            "--number-fmt",
            "diff",
            "-p",
            path,
            "HEAD~1..HEAD",
            "--output",
            "text",
        ],
        workspace_root(),
        &[],
    );
    let suffix = rustloc(
        &[
            "diff",
            "-p",
            path,
            "HEAD~1..HEAD",
            "--type",
            "code",
            "--lang",
            "rust",
            "--by-file",
            "--ordering",
            "-code",
            "--code-gte",
            "1",
            "--top",
            "2",
            "--number-fmt",
            "--output",
            "text",
        ],
        workspace_root(),
        &[],
    );

    assert_eq!(
        prefix.status.code(),
        Some(0),
        "prefix stderr: {}",
        stderr(&prefix)
    );
    assert_eq!(
        suffix.status.code(),
        Some(0),
        "suffix stderr: {}",
        stderr(&suffix)
    );
    assert_eq!(stdout(&prefix), stdout(&suffix));
}

#[test]
fn commit_shared_query_options_match_before_and_after_the_subcommand() {
    let dir = query_repo();
    let path = dir.path().to_str().unwrap();

    let prefix = rustloc(
        &[
            "--type",
            "code",
            "--lang",
            "rust",
            "--by-file",
            "--ordering",
            "-code",
            "--code-gte",
            "1",
            "--top",
            "2",
            "--number-fmt",
            "commit",
            "-p",
            path,
            "HEAD",
            "--output",
            "text",
        ],
        workspace_root(),
        &[],
    );
    let suffix = rustloc(
        &[
            "commit",
            "-p",
            path,
            "HEAD",
            "--type",
            "code",
            "--lang",
            "rust",
            "--by-file",
            "--ordering",
            "-code",
            "--code-gte",
            "1",
            "--top",
            "2",
            "--number-fmt",
            "--output",
            "text",
        ],
        workspace_root(),
        &[],
    );

    assert_eq!(
        prefix.status.code(),
        Some(0),
        "prefix stderr: {}",
        stderr(&prefix)
    );
    assert_eq!(
        suffix.status.code(),
        Some(0),
        "suffix stderr: {}",
        stderr(&suffix)
    );
    assert_eq!(stdout(&prefix), stdout(&suffix));
}

#[test]
fn count_only_options_fail_for_diff_and_commit_regardless_of_position() {
    let dir = query_repo();
    let path = dir.path().to_str().unwrap();

    for args in [
        vec!["--shows-ratio", "diff", "-p", path, "HEAD~1..HEAD"],
        vec!["diff", "-p", path, "HEAD~1..HEAD", "--shows-ratio"],
        vec!["--shows-ratio", "commit", "-p", path, "HEAD"],
        vec!["commit", "-p", path, "HEAD", "--shows-ratio"],
    ] {
        let output = rustloc(&args, workspace_root(), &[]);
        assert_ne!(
            output.status.code(),
            Some(0),
            "{args:?} should fail, stdout={} stderr={}",
            stdout(&output),
            stderr(&output),
        );
        assert!(
            output.stdout.is_empty(),
            "{args:?} should not write successful output"
        );
        assert!(
            stderr(&output).contains("--shows-ratio")
                || stderr(&output).contains("unexpected argument"),
            "{args:?} should name the count-only option, got: {}",
            stderr(&output)
        );
    }
}

#[test]
fn root_count_path_fails_with_explicit_diff_or_commit() {
    let dir = query_repo();
    let path = dir.path().to_str().unwrap();

    for args in [
        vec!["/tmp", "diff", "-p", path, "HEAD~1..HEAD"],
        vec!["/tmp", "commit", "-p", path, "HEAD"],
        vec!["/tmp", "commit", "HEAD"],
    ] {
        let output = rustloc(&args, workspace_root(), &[]);
        assert_ne!(
            output.status.code(),
            Some(0),
            "{args:?} should fail, stdout={} stderr={}",
            stdout(&output),
            stderr(&output),
        );
        assert!(
            output.stdout.is_empty(),
            "{args:?} should not write successful output"
        );
    }
}

#[test]
fn bare_and_explicit_count_match_with_shared_query_options() {
    let dir = TempDir::new().expect("count workspace");
    let root = dir.path();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"count-workspace\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn a() {}\npub fn b() {}\n\n#[test]\nfn t() {}\n",
    )
    .unwrap();
    let path = root.to_str().unwrap();

    let bare = rustloc(
        &[
            "--type",
            "code",
            "--lang",
            "rust",
            "--by-file",
            "--ordering",
            "-code",
            "--code-gte",
            "1",
            "--top",
            "1",
            "--number-fmt",
            path,
            "--output",
            "text",
        ],
        workspace_root(),
        &[],
    );
    let explicit = rustloc(
        &[
            "count",
            path,
            "--type",
            "code",
            "--lang",
            "rust",
            "--by-file",
            "--ordering",
            "-code",
            "--code-gte",
            "1",
            "--top",
            "1",
            "--number-fmt",
            "--output",
            "text",
        ],
        workspace_root(),
        &[],
    );

    assert_eq!(bare.status.code(), Some(0), "stderr: {}", stderr(&bare));
    assert_eq!(
        explicit.status.code(),
        Some(0),
        "stderr: {}",
        stderr(&explicit)
    );
    assert_eq!(stdout(&bare), stdout(&explicit));
}

#[test]
fn root_help_does_not_advertise_path_before_explicit_commands() {
    let output = rustloc(&["--help"], workspace_root(), &[]);
    let help = stdout(&output);

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(
        !help.contains("[PATH] [COMMAND]"),
        "root help should not advertise a count path before a subcommand:\n{help}"
    );
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

/// Process-only contract: Clapfig resolves `SearchPath::Cwd` from the child
/// process's working directory, so a project-local `rustloc.toml` can enable
/// the count table ratio row without a CLI flag.
#[test]
fn rustloc_toml_enables_count_table_ratios() {
    let dir = TempDir::new().expect("config fixture");
    std::fs::write(dir.path().join("only.rs"), "pub fn only() {}\n").unwrap();
    std::fs::write(dir.path().join("rustloc.toml"), "shows_ratios = true\n").unwrap();

    let output = rustloc(&[".", "--output", "text"], dir.path(), &[]);
    let rendered = stdout(&output);

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(output.stderr.is_empty());
    assert!(
        rendered.contains("Ratio") && rendered.contains("100.0%"),
        "rustloc.toml should enable ratios:\n{rendered}"
    );
}

/// Process-only contract: Clapfig's cwd-based config discovery can enable
/// locale grouping without a CLI flag.
#[test]
fn rustloc_toml_enables_number_formatting() {
    let dir = TempDir::new().expect("config fixture");
    let mut source = String::new();
    for i in 0..3805 {
        source.push_str(&format!("pub fn f_{i}() {{}}\n"));
    }
    std::fs::write(dir.path().join("only.rs"), source).unwrap();
    std::fs::write(dir.path().join("rustloc.toml"), "number_fmt = true\n").unwrap();

    let output = rustloc(
        &[".", "--by-file", "--type", "code", "--output", "text"],
        dir.path(),
        &[],
    );
    let rendered = stdout(&output);

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(output.stderr.is_empty());
    assert!(
        rendered.contains(&active_format_u64(3805)),
        "rustloc.toml should enable number formatting:\n{rendered}"
    );
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
