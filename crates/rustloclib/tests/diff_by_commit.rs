//! Behavior of `diff_by_commit` against real temporary Git repositories.
//!
//! These tests pin the Git semantics the Spec fixes: range membership for
//! two-dot, three-dot, and single-revision forms; `git rev-list` traversal
//! order as the default row order; first-parent comparison for merges; the
//! empty tree for roots; a row for empty and fully-filtered commits; churn
//! totals; distinct file counts; accumulated skipped changes; and a clear
//! error — never a silent omission — for a shallow clone's missing parent.
//!
//! Fixtures deliberately carry no root `Cargo.toml`, so project
//! classification stays empty and each tree's analysis is file-local — the
//! Cargo-aware path has its own coverage in `project_classification.rs`.

use std::path::Path;
use std::process::Command;

use rustloclib::{diff_by_commit, DiffOptions, FilterConfig, LanguageSelection, LineTypes};
use tempfile::TempDir;

/// Run git in `dir` with a pinned identity and timestamp so commit hashes
/// and timestamps are deterministic per test run. `date` chooses the
/// author+committer date, which the equal-timestamp ordering test relies on.
fn git_at(dir: &Path, date: &str, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .expect("git must be available");
    assert!(
        output.status.success(),
        "git {args:?} failed in {dir:?}: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn git(dir: &Path, args: &[&str]) {
    git_at(dir, "2024-01-01T00:00:00Z", args);
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git must be available");
    assert!(output.status.success(), "git {args:?} failed in {dir:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn init_repo() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "--initial-branch=main"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    dir
}

fn commit_all(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", message]);
}

fn rev(dir: &Path, spec: &str) -> String {
    git_stdout(dir, &["rev-parse", spec])
}

fn short(hash: &str) -> String {
    hash.chars().take(8).collect()
}

/// Three commits on `main`: c1 adds `a.rs` (1 code line), c2 adds `b.rs`
/// (2 code lines), c3 grows `a.rs` by 1 code line.
fn linear_repo() -> TempDir {
    let dir = init_repo();
    let p = dir.path();
    std::fs::write(p.join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(p, "one");
    std::fs::write(p.join("b.rs"), "fn b() {}\nfn b2() {}\n").unwrap();
    commit_all(p, "two");
    std::fs::write(p.join("a.rs"), "fn a() {}\nfn a2() {}\n").unwrap();
    commit_all(p, "three");
    dir
}

#[test]
fn two_dot_selects_right_side_commits_in_traversal_order() {
    let dir = linear_repo();
    let p = dir.path();

    let result = diff_by_commit(p, "HEAD~2..HEAD", DiffOptions::new()).unwrap();

    // Children before parents: three, then two. All timestamps are equal, so
    // this order is the parent constraint, exactly what git rev-list emits.
    let rows: Vec<(&str, &str)> = result
        .commits
        .iter()
        .map(|c| (c.hash.as_str(), c.subject.as_str()))
        .collect();
    assert_eq!(
        rows,
        vec![
            (short(&rev(p, "HEAD")).as_str(), "three"),
            (short(&rev(p, "HEAD~1")).as_str(), "two"),
        ]
    );

    // Per-commit stats: "three" adds 1 code line to a.rs, "two" adds 2.
    assert_eq!(result.commits[0].diff.added.code, 1);
    assert_eq!(result.commits[1].diff.added.code, 2);

    // Totals sum the rows; two distinct files were touched.
    assert_eq!(result.total.added.code, 3);
    assert_eq!(result.file_count, 2);
}

#[test]
fn single_revision_selects_commits_reachable_from_head_only() {
    let dir = linear_repo();
    let p = dir.path();

    let result = diff_by_commit(p, "HEAD~2", DiffOptions::new()).unwrap();

    let subjects: Vec<&str> = result.commits.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(subjects, vec!["three", "two"]);
}

#[test]
fn three_dot_selects_from_the_merge_base_like_git_diff() {
    // main: base -> m1; feature (from base): f1. `feature...main` must select
    // only main's side (m1), not `git log`'s symmetric union with f1.
    let dir = init_repo();
    let p = dir.path();
    std::fs::write(p.join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(p, "base");
    git(p, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(p.join("f.rs"), "fn f() {}\n").unwrap();
    commit_all(p, "f1");
    git(p, &["checkout", "-q", "main"]);
    std::fs::write(p.join("m.rs"), "fn m() {}\n").unwrap();
    commit_all(p, "m1");

    let result = diff_by_commit(p, "feature...main", DiffOptions::new()).unwrap();

    let subjects: Vec<&str> = result.commits.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(subjects, vec!["m1"]);
    assert_eq!(result.commits[0].diff.added.code, 1);
}

#[test]
fn merge_commits_use_the_first_parent_and_totals_measure_churn() {
    // base -> m1 on main; f1 on feature; merge feature into main. The range
    // base..main selects {merge, m1, f1}. The merge's first-parent comparison
    // re-reports f1's addition, so the churn total counts f.rs twice while
    // the distinct file count still sees each file once.
    let dir = init_repo();
    let p = dir.path();
    std::fs::write(p.join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(p, "base");
    git(p, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(p.join("f.rs"), "fn f() {}\n").unwrap();
    commit_all(p, "f1");
    git(p, &["checkout", "-q", "main"]);
    std::fs::write(p.join("m.rs"), "fn m() {}\n").unwrap();
    commit_all(p, "m1");
    git_at(
        p,
        "2024-01-02T00:00:00Z",
        &["merge", "-q", "--no-ff", "-m", "merge feature", "feature"],
    );

    let result = diff_by_commit(p, "main~2..main", DiffOptions::new()).unwrap();

    // The merge is the newest commit, so traversal puts it first; the merged
    // branch's commit is also selected as its own row.
    let by_subject: std::collections::HashMap<&str, i64> = result
        .commits
        .iter()
        .map(|c| (c.subject.as_str(), c.diff.net_code()))
        .collect();
    assert_eq!(result.commits[0].subject, "merge feature");
    assert_eq!(by_subject["merge feature"], 1); // f.rs, vs first parent m1
    assert_eq!(by_subject["m1"], 1);
    assert_eq!(by_subject["f1"], 1);

    // Churn: 3 added code lines even though the endpoint trees differ by 2.
    assert_eq!(result.total.added.code, 3);
    assert_eq!(result.file_count, 2); // f.rs and m.rs, each once
}

#[test]
fn a_selected_root_commit_diffs_against_the_empty_tree() {
    // An orphan branch makes main's root selectable: `side..main` reaches all
    // of main, root included.
    let dir = linear_repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--orphan", "side"]);
    git(p, &["rm", "-rqf", "."]);
    git(p, &["commit", "-q", "--allow-empty", "-m", "side"]);

    let result = diff_by_commit(p, "side..main", DiffOptions::new()).unwrap();

    let root = result.commits.last().unwrap();
    assert_eq!(root.subject, "one");
    // The root's whole tree counts as additions: a.rs with 1 code line.
    assert_eq!(root.diff.added.code, 1);
    assert_eq!(root.diff.removed.code, 0);
}

#[test]
fn empty_commits_still_produce_a_zero_row() {
    let dir = linear_repo();
    let p = dir.path();
    git(p, &["commit", "-q", "--allow-empty", "-m", "empty marker"]);

    let result = diff_by_commit(p, "HEAD~1..HEAD", DiffOptions::new()).unwrap();

    assert_eq!(result.commits.len(), 1);
    assert_eq!(result.commits[0].subject, "empty marker");
    assert_eq!(result.commits[0].diff, Default::default());
    assert_eq!(result.file_count, 0);
}

#[test]
fn commits_outside_the_language_selection_get_a_zero_row_and_skip_counters() {
    // One commit touches only a non-analyzed file: it must keep its row (zero
    // stats), stay out of the distinct file count, and feed the skipped-change
    // accumulators across the range.
    let dir = linear_repo();
    let p = dir.path();
    std::fs::write(p.join("notes.md"), "one\ntwo\nthree\n").unwrap();
    commit_all(p, "docs only");
    std::fs::write(p.join("notes.md"), "one\n").unwrap();
    commit_all(p, "docs trimmed");

    let result = diff_by_commit(p, "HEAD~2..HEAD", DiffOptions::new()).unwrap();

    let subjects: Vec<&str> = result.commits.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(subjects, vec!["docs trimmed", "docs only"]);
    assert!(result.commits.iter().all(|c| c.diff == Default::default()));
    assert_eq!(result.file_count, 0);
    // Accumulated across both commits: +3 then -2.
    assert_eq!(result.non_rust_added, 3);
    assert_eq!(result.non_rust_removed, 2);
}

#[test]
fn glob_filtered_changes_zero_the_row_without_dropping_it() {
    let dir = linear_repo();
    let p = dir.path();

    let options = DiffOptions::new().filter(
        FilterConfig::new()
            .languages(LanguageSelection::new(rustloclib::default_languages()))
            .exclude("b.rs")
            .unwrap(),
    );
    let result = diff_by_commit(p, "HEAD~2..HEAD", options).unwrap();

    // "two" only adds b.rs; excluded by glob, so its row is zero but present.
    let by_subject: std::collections::HashMap<&str, u64> = result
        .commits
        .iter()
        .map(|c| (c.subject.as_str(), c.diff.added.code))
        .collect();
    assert_eq!(by_subject["three"], 1);
    assert_eq!(by_subject["two"], 0);
    assert_eq!(result.file_count, 1); // only a.rs analyzed
}

#[test]
fn churn_totals_count_lines_added_and_then_removed_on_both_sides() {
    // c2 adds two lines, c3 removes them again: the endpoint diff is empty,
    // but per-commit totals must report both movements.
    let dir = init_repo();
    let p = dir.path();
    std::fs::write(p.join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(p, "one");
    std::fs::write(p.join("a.rs"), "fn a() {}\nfn tmp() {}\nfn tmp2() {}\n").unwrap();
    commit_all(p, "add tmp");
    std::fs::write(p.join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(p, "drop tmp");

    let result = diff_by_commit(p, "HEAD~2..HEAD", DiffOptions::new()).unwrap();

    assert_eq!(result.total.added.code, 2);
    assert_eq!(result.total.removed.code, 2);
    assert_eq!(result.total.net_code(), 0);
    assert_eq!(result.file_count, 1);
}

#[test]
fn same_endpoint_range_selects_no_commits() {
    let dir = linear_repo();
    let result = diff_by_commit(dir.path(), "HEAD..HEAD", DiffOptions::new()).unwrap();
    assert!(result.commits.is_empty());
    assert_eq!(result.total, Default::default());
    assert_eq!(result.file_count, 0);
}

#[test]
fn subjects_come_from_the_first_paragraph_with_fallback() {
    let dir = init_repo();
    let p = dir.path();
    std::fs::write(p.join("base.rs"), "fn base() {}\n").unwrap();
    commit_all(p, "base");
    std::fs::write(p.join("a.rs"), "fn a() {}\n").unwrap();
    git(p, &["add", "-A"]);
    git(
        p,
        &[
            "commit",
            "-qm",
            "Wrapped title line\ncontinues here\n\nBody paragraph.",
        ],
    );
    std::fs::write(p.join("a.rs"), "fn a() {}\nfn b() {}\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "--allow-empty-message", "-m", ""]);

    let result = diff_by_commit(p, "HEAD~2..HEAD", DiffOptions::new()).unwrap();

    let subjects: Vec<&str> = result.commits.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(
        subjects,
        vec!["(no subject)", "Wrapped title line continues here"]
    );
}

#[test]
fn hashes_are_the_first_eight_lowercase_hex_characters() {
    let dir = linear_repo();
    let p = dir.path();

    let result = diff_by_commit(p, "HEAD~1..HEAD", DiffOptions::new()).unwrap();

    let hash = &result.commits[0].hash;
    assert_eq!(hash.len(), 8);
    assert_eq!(*hash, short(&rev(p, "HEAD")));
    assert!(hash
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
}

#[test]
fn line_type_filtering_applies_to_commit_rows() {
    let dir = linear_repo();
    let p = dir.path();

    let options = DiffOptions::new().line_types(LineTypes::none());
    let result = diff_by_commit(p, "HEAD~1..HEAD", options).unwrap();

    assert_eq!(result.commits.len(), 1);
    assert_eq!(result.commits[0].diff.added.code, 0);
    assert_eq!(result.total.added.code, 0);
}

#[test]
fn an_unresolvable_revspec_is_a_clear_error() {
    let dir = linear_repo();
    let err = diff_by_commit(dir.path(), "no-such-ref..HEAD", DiffOptions::new()).unwrap_err();
    assert!(
        err.to_string().contains("Could not resolve revision"),
        "unexpected error: {err}"
    );
}

#[test]
fn a_shallow_clones_missing_parent_is_an_error_not_a_dropped_row() {
    // A depth-2 clone has the last two commits; the older one is a shallow
    // graft whose recorded parent object is absent. Selecting it must fail
    // loudly: silently skipping it would misreport the range.
    let origin = linear_repo();
    let clone_dir = tempfile::tempdir().unwrap();
    let clone_path = clone_dir.path().join("clone");
    let url = format!("file://{}", origin.path().display());
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "clone",
            "-q",
            "--depth",
            "2",
            &url,
            clone_path.to_str().unwrap(),
        ])
        .output()
        .expect("git must be available");
    assert!(
        output.status.success(),
        "shallow clone failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let p = clone_path.as_path();
    git(p, &["config", "commit.gpgsign", "false"]);

    // An orphan branch gives the range a resolvable excluded side while the
    // included side still reaches the grafted commit.
    git(p, &["checkout", "-q", "--orphan", "side"]);
    git(p, &["rm", "-rqf", "."]);
    git(p, &["commit", "-q", "--allow-empty", "-m", "side"]);

    let err = diff_by_commit(p, "side..main", DiffOptions::new()).unwrap_err();
    assert!(
        err.to_string().contains("shallow"),
        "the error should point at the shallow history: {err}"
    );
}
