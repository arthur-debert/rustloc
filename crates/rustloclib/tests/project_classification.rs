//! Project context applied through rustloclib's public counting and diff API.
//!
//! Every test here builds a real temporary Cargo project and asks
//! [`count_workspace`], [`diff_revspec`], or [`diff_workdir`] for numbers.
//! Nothing reaches into rust-analyzer: what these tests pin down is the
//! observable classification a caller gets, not how the module graph is
//! loaded.
//!
//! Two project facts decide whether a Rust file is production code, and both
//! are covered here. The first is the parent module declaration:
//! `archive_tests.rs` is ordinary-looking Rust, and only `archive.rs`'s
//! `#[cfg(all(test, unix))] #[path = "archive_tests.rs"] mod tests;` says it
//! belongs to the test build. The second is the Cargo target a file is
//! reached through: the root of a `[[test]]` target, and everything it
//! declares, is test-only wherever the manifest points, with no `cfg(test)`
//! declaration anywhere.

use std::path::{Path, PathBuf};
use std::process::Command;

use rustloclib::{
    count_directory, count_workspace, diff_revspec, diff_workdir, Aggregation, CountOptions,
    CountResult, DiffOptions, FilterConfig, LineTypes, Locs, WorkdirDiffMode,
};
use tempfile::TempDir;

/// Write `contents` to `root/relative`, creating parent directories.
fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// A single-package manifest named `name`.
fn manifest(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n")
}

/// Count a workspace per file, with every line type reported.
fn count_by_file(root: &Path) -> CountResult {
    count_workspace(
        root,
        CountOptions::new()
            .aggregation(Aggregation::ByFile)
            .line_types(LineTypes::everything())
            .filter(FilterConfig::new()),
    )
    .unwrap()
}

/// The per-file stats a count reports for `root/relative`.
fn file_stats(result: &CountResult, relative: &str) -> Locs {
    let wanted = PathBuf::from(relative);
    result
        .files
        .iter()
        .find(|file| file.path.ends_with(&wanted))
        .unwrap_or_else(|| {
            panic!(
                "no stats for {relative}; counted {:?}",
                result.files.iter().map(|f| &f.path).collect::<Vec<_>>()
            )
        })
        .stats
}

#[test]
fn a_module_declared_only_under_cfg_test_counts_as_tests() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write(root, "Cargo.toml", &manifest("direct"));
    write(
        root,
        "src/lib.rs",
        "pub fn run() {}\n\n#[cfg(test)]\nmod helpers;\n",
    );
    write(
        root,
        "src/helpers.rs",
        "pub fn helper() -> u32 {\n    7\n}\n",
    );

    let result = count_by_file(root);
    let helpers = file_stats(&result, "src/helpers.rs");

    assert_eq!(helpers.tests, 3);
    assert_eq!(helpers.code, 0);
}

#[test]
fn a_proiectio_shaped_path_module_counts_as_tests_while_its_parent_stays_production() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write(root, "Cargo.toml", &manifest("proiectio-shaped"));
    write(root, "src/lib.rs", "pub mod archive;\n");
    write(
        root,
        "src/archive.rs",
        "pub fn archive() -> bool {\n    true\n}\n\n\
         #[cfg(test)]\n\
         mod inline {\n    \
         #[test]\n    \
         fn inline_case() {}\n\
         }\n\n\
         #[cfg(all(test, unix))]\n\
         #[path = \"archive_tests.rs\"]\n\
         mod tests;\n",
    );
    write(
        root,
        "src/archive_tests.rs",
        "use super::archive;\n\n#[test]\nfn archives() {\n    assert!(archive());\n}\n",
    );

    let result = count_by_file(root);
    let archive_tests = file_stats(&result, "src/archive_tests.rs");
    let archive = file_stats(&result, "src/archive.rs");

    // Every logic line of the test-only file is test code, and its blank
    // line stays a blank.
    assert_eq!(archive_tests.tests, 5);
    assert_eq!(archive_tests.code, 0);
    assert_eq!(archive_tests.blanks, 1);

    // The parent keeps the production logic it really has, and the tests it
    // marks itself stay tests.
    assert_eq!(archive.code, 3);
    assert!(archive.tests > 0);
}

#[test]
fn a_module_shared_with_production_stays_production() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write(root, "Cargo.toml", &manifest("shared"));
    write(
        root,
        "src/lib.rs",
        "pub mod shared;\n\n#[cfg(test)]\nmod only_tests;\n",
    );
    write(
        root,
        "src/shared.rs",
        "pub fn shared() -> u32 {\n    1\n}\n",
    );
    write(
        root,
        "src/only_tests.rs",
        "#[test]\nfn uses_shared() {\n    assert_eq!(crate::shared::shared(), 1);\n}\n",
    );

    let result = count_by_file(root);

    assert_eq!(file_stats(&result, "src/shared.rs").code, 3);
    assert_eq!(file_stats(&result, "src/shared.rs").tests, 0);
    assert_eq!(file_stats(&result, "src/only_tests.rs").tests, 4);
    assert_eq!(file_stats(&result, "src/only_tests.rs").code, 0);
}

#[test]
fn cargo_test_targets_count_as_tests_even_outside_a_tests_directory() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write(
        root,
        "Cargo.toml",
        &format!(
            "{}\n[[test]]\nname = \"custom\"\npath = \"src/custom_target.rs\"\n",
            manifest("targets")
        ),
    );
    write(root, "src/lib.rs", "pub fn run() -> u32 {\n    1\n}\n");
    write(
        root,
        "src/custom_target.rs",
        "mod support;\n\n#[test]\nfn runs() {\n    support::prepare();\n}\n",
    );
    // A crate root resolves `mod support;` beside itself, so the helper is
    // `src/support.rs` — a file the library crate never declares.
    write(root, "src/support.rs", "pub fn prepare() {}\n");

    let result = count_by_file(root);

    // These files sit in `src/` and match no filename convention: only the
    // `[[test]]` target declaration makes them test code.
    assert_eq!(file_stats(&result, "src/custom_target.rs").tests, 5);
    assert_eq!(file_stats(&result, "src/custom_target.rs").code, 0);
    assert_eq!(file_stats(&result, "src/support.rs").tests, 1);
    assert_eq!(file_stats(&result, "src/support.rs").code, 0);
    assert_eq!(file_stats(&result, "src/lib.rs").code, 3);
}

#[test]
fn every_workspace_member_is_classified_by_the_same_project_load() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"alpha\", \"beta\"]\nresolver = \"2\"\n",
    );
    for member in ["alpha", "beta"] {
        write(root, &format!("{member}/Cargo.toml"), &manifest(member));
        write(
            root,
            &format!("{member}/src/lib.rs"),
            "pub fn run() {}\n\n#[cfg(test)]\nmod cases;\n",
        );
        write(
            root,
            &format!("{member}/src/cases.rs"),
            "#[test]\nfn case() {}\n",
        );
    }

    let result = count_by_file(root);

    assert_eq!(file_stats(&result, "alpha/src/cases.rs").tests, 2);
    assert_eq!(file_stats(&result, "alpha/src/cases.rs").code, 0);
    assert_eq!(file_stats(&result, "beta/src/cases.rs").tests, 2);
    assert_eq!(file_stats(&result, "beta/src/cases.rs").code, 0);
}

#[test]
fn counting_runs_no_build_script_and_no_proc_macro() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let sentinel = root.join("build-script-ran");
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"consumer\", \"macros\"]\nresolver = \"2\"\n",
    );

    // A build script that leaves evidence behind if it is ever executed.
    write(
        root,
        "consumer/Cargo.toml",
        &format!(
            "{}build = \"build.rs\"\n\n[dependencies]\nmacros = {{ path = \"../macros\" }}\n",
            manifest("consumer")
        ),
    );
    write(
        root,
        "consumer/build.rs",
        &format!(
            "fn main() {{\n    std::fs::write(\"{}\", \"ran\").unwrap();\n}}\n",
            sentinel.display()
        ),
    );
    write(
        root,
        "consumer/src/lib.rs",
        "pub fn run() {}\n\n#[cfg(test)]\nmod cases;\n",
    );
    write(root, "consumer/src/cases.rs", "#[test]\nfn case() {}\n");

    // A proc-macro crate: expanding it would require building and loading a
    // dylib, which needs a proc-macro server.
    write(
        root,
        "macros/Cargo.toml",
        &format!("{}\n[lib]\nproc-macro = true\n", manifest("macros")),
    );
    write(
        root,
        "macros/src/lib.rs",
        "use proc_macro::TokenStream;\n\n\
         #[proc_macro]\n\
         pub fn nothing(_input: TokenStream) -> TokenStream {\n    \
         TokenStream::new()\n\
         }\n",
    );

    let result = count_by_file(root);

    assert_eq!(file_stats(&result, "consumer/src/cases.rs").tests, 2);
    assert!(
        !sentinel.exists(),
        "the build script ran; project classification must not execute build scripts"
    );
    assert!(
        !root.join("target").exists(),
        "a compilation artifact directory appeared; nothing may be built to classify a project"
    );
}

#[test]
fn a_project_that_cannot_be_loaded_keeps_the_file_local_result() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    // No manifest at all: there is no module graph to consult, so the count
    // has to fall back on what the files themselves say.
    write(
        root,
        "src/lib.rs",
        "pub fn run() {}\n\n#[cfg(test)]\nmod cases;\n",
    );
    write(root, "src/cases.rs", "#[test]\nfn case() {}\n");

    let result = count_directory(root, &FilterConfig::new()).unwrap();
    let cases = result
        .files
        .iter()
        .find(|file| file.path.ends_with("cases.rs"))
        .unwrap()
        .stats;

    // `#[test] fn case() {}` is locally marked, so the syntax backend already
    // calls both lines tests — and nothing else changed.
    assert_eq!(cases.tests, 2);
    assert_eq!(cases.code, 0);
    assert_eq!(result.total.code, 1);
}

// ---------------------------------------------------------------------------
// Diffs
// ---------------------------------------------------------------------------

/// Run `git` in `dir` and fail loudly on a non-zero exit.
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to run git {args:?}: {e}"));
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Initialize a git repository with a deterministic identity.
fn init_repo(dir: &Path) {
    git(dir, &["init", "--initial-branch=main"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test"]);
}

/// Commit everything in `dir` and return the full commit hash.
fn commit(dir: &Path, message: &str) -> String {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-m", message]);
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// Per-file diff options with every line type reported.
fn diff_by_file() -> DiffOptions {
    DiffOptions::new()
        .aggregation(Aggregation::ByFile)
        .line_types(LineTypes::everything())
}

#[test]
fn an_added_test_only_file_adds_test_lines_not_code_lines() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    init_repo(root);
    write(root, "Cargo.toml", &manifest("added"));
    write(root, "src/lib.rs", "pub mod archive;\n");
    write(
        root,
        "src/archive.rs",
        "pub fn archive() -> bool {\n    true\n}\n",
    );
    let before = commit(root, "production only");

    write(
        root,
        "src/archive.rs",
        "pub fn archive() -> bool {\n    true\n}\n\n\
         #[cfg(all(test, unix))]\n\
         #[path = \"archive_tests.rs\"]\n\
         mod tests;\n",
    );
    write(
        root,
        "src/archive_tests.rs",
        "use super::archive;\n\n#[test]\nfn archives() {\n    assert!(archive());\n}\n",
    );
    let after = commit(root, "add the test module");

    let result = diff_revspec(root, &format!("{before}..{after}"), diff_by_file()).unwrap();
    let added_file = result
        .files
        .iter()
        .find(|file| file.path.ends_with("archive_tests.rs"))
        .expect("archive_tests.rs should appear in the diff");

    assert_eq!(added_file.diff.added.tests, 5);
    assert_eq!(added_file.diff.added.code, 0);
    // The parent's three new lines are its own `cfg(test)` declaration, which
    // the syntax backend already reads as test code; no production line moved.
    assert_eq!(result.total.added.code, 0);
    assert_eq!(result.total.added.tests, 8);
}

#[test]
fn a_deleted_test_only_file_removes_test_lines_not_code_lines() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    init_repo(root);
    write(root, "Cargo.toml", &manifest("deleted"));
    write(root, "src/lib.rs", "pub mod archive;\n");
    write(
        root,
        "src/archive.rs",
        "pub fn archive() -> bool {\n    true\n}\n\n\
         #[cfg(test)]\n\
         #[path = \"archive_tests.rs\"]\n\
         mod tests;\n",
    );
    write(
        root,
        "src/archive_tests.rs",
        "use super::archive;\n\n#[test]\nfn archives() {\n    assert!(archive());\n}\n",
    );
    let before = commit(root, "with the test module");

    std::fs::remove_file(root.join("src/archive_tests.rs")).unwrap();
    write(
        root,
        "src/archive.rs",
        "pub fn archive() -> bool {\n    true\n}\n",
    );
    let after = commit(root, "drop the test module");

    let result = diff_revspec(root, &format!("{before}..{after}"), diff_by_file()).unwrap();
    let deleted_file = result
        .files
        .iter()
        .find(|file| file.path.ends_with("archive_tests.rs"))
        .expect("archive_tests.rs should appear in the diff");

    assert_eq!(deleted_file.diff.removed.tests, 5);
    assert_eq!(deleted_file.diff.removed.code, 0);
}

#[test]
fn a_modified_file_is_classified_against_the_revision_each_side_belongs_to() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    init_repo(root);
    write(root, "Cargo.toml", &manifest("moved"));
    write(root, "src/lib.rs", "pub mod util;\n");
    write(root, "src/util.rs", "pub fn util() -> u32 {\n    1\n}\n");
    let before = commit(root, "util is production");

    // The declaration moves behind `cfg(test)` and the file's body changes
    // in the same revision: the old side is production, the new side is not.
    write(root, "src/lib.rs", "#[cfg(test)]\nmod util;\n");
    write(root, "src/util.rs", "pub fn util() -> u32 {\n    2\n}\n");
    let after = commit(root, "util is test-only");

    let result = diff_revspec(root, &format!("{before}..{after}"), diff_by_file()).unwrap();
    let modified = result
        .files
        .iter()
        .find(|file| file.path.ends_with("util.rs"))
        .expect("util.rs should appear in the diff");

    assert_eq!(modified.diff.removed.code, 1);
    assert_eq!(modified.diff.removed.tests, 0);
    assert_eq!(modified.diff.added.tests, 1);
    assert_eq!(modified.diff.added.code, 0);
}

#[test]
fn a_working_tree_diff_classifies_the_committed_and_working_sides_separately() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    init_repo(root);
    write(root, "Cargo.toml", &manifest("workdir"));
    write(root, "src/lib.rs", "pub mod util;\n");
    write(root, "src/util.rs", "pub fn util() -> u32 {\n    1\n}\n");
    commit(root, "util is production");

    write(root, "src/lib.rs", "#[cfg(test)]\nmod util;\n");
    write(root, "src/util.rs", "pub fn util() -> u32 {\n    2\n}\n");

    let result = diff_workdir(root, WorkdirDiffMode::All, diff_by_file()).unwrap();
    let modified = result
        .files
        .iter()
        .find(|file| file.path.ends_with("util.rs"))
        .expect("util.rs should appear in the diff");

    assert_eq!(modified.diff.removed.code, 1);
    assert_eq!(modified.diff.added.tests, 1);
    assert_eq!(modified.diff.added.code, 0);
}

#[test]
fn a_staged_diff_classifies_the_new_side_against_the_index_not_the_working_tree() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    init_repo(root);
    write(root, "Cargo.toml", &manifest("staged"));
    write(root, "src/lib.rs", "pub mod util;\n");
    write(root, "src/util.rs", "pub fn util() -> u32 {\n    1\n}\n");
    commit(root, "util is production");

    // Staged: util.rs moves into the test build and its body changes.
    write(root, "src/lib.rs", "#[cfg(test)]\nmod util;\n");
    write(root, "src/util.rs", "pub fn util() -> u32 {\n    2\n}\n");
    git(root, &["add", "src/lib.rs", "src/util.rs"]);

    // Unstaged on top: lib.rs declares util as production again. A staged
    // diff must not see this, or util.rs's new line reads as code.
    write(root, "src/lib.rs", "pub mod util;\n");

    let result = diff_workdir(root, WorkdirDiffMode::Staged, diff_by_file()).unwrap();
    let modified = result
        .files
        .iter()
        .find(|file| file.path.ends_with("util.rs"))
        .expect("util.rs should appear in the diff");

    assert_eq!(modified.diff.removed.code, 1);
    assert_eq!(modified.diff.added.tests, 1);
    assert_eq!(modified.diff.added.code, 0);
}

#[test]
fn a_revision_without_a_loadable_project_still_diffs() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    init_repo(root);
    // The first revision has no manifest at all, so its project cannot load.
    write(root, "src/lib.rs", "pub fn run() -> u32 {\n    1\n}\n");
    let before = commit(root, "no cargo project yet");

    write(root, "Cargo.toml", &manifest("late"));
    write(
        root,
        "src/lib.rs",
        "pub fn run() -> u32 {\n    2\n}\n\n#[cfg(test)]\nmod cases;\n",
    );
    write(root, "src/cases.rs", "#[test]\nfn case() {}\n");
    let after = commit(root, "add the manifest");

    let result = diff_revspec(root, &format!("{before}..{after}"), diff_by_file()).unwrap();

    // The old side keeps its file-local numbers instead of failing the
    // command, and the new side is classified normally.
    let cases = result
        .files
        .iter()
        .find(|file| file.path.ends_with("cases.rs"))
        .expect("cases.rs should appear in the diff");
    assert_eq!(cases.diff.added.tests, 2);
    assert_eq!(result.total.removed.code, 1);
}
