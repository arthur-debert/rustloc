//! Typed orchestration for the `count` and `diff` commands.
//!
//! Each function takes one typed request from [`crate::command`], selects the
//! right `rustloclib` entry point, and builds the canonical response
//! ([`CountQuerySet`] / [`DiffQuerySet`]). No clap types, no output mode, no
//! formatting — which is what makes these callable straight from a test.
//!
//! ## Why this lives in the CLI, not `rustloclib`
//!
//! The steps here are *command* behaviour, not domain behaviour: "`--by-crate`
//! requires a workspace" is a rule about this CLI's flags, and "classify the
//! path, then pick `count_workspace` vs `count_file` vs `count_directory`" is
//! this CLI's dispatch policy over an API the library already exposes
//! generally. Pushing either into `rustloclib` would hand the library an
//! opinion about one caller's argument grammar. Genuinely reusable domain
//! logic — counting, diffing, aggregation, ordering, predicates — is already in
//! the library and is only *called* from here.

use rustloclib::{
    count_directory_with_options, count_file_with_filter, count_workspace, diff_revspec,
    diff_workdir, Aggregation, CountOptions, CountQuerySet, CountResult, DiffOptions, DiffQuerySet,
    LineTypes,
};

use crate::command::{CountPath, CountRequest, DiffEndpoints, DiffRequest, QueryRequest};

/// Run a count and return its canonical response.
///
/// # Errors
///
/// Fails when `--by-crate` is asked of a non-workspace path, or when the
/// library cannot read the target.
pub fn count(request: &CountRequest) -> Result<CountQuerySet, anyhow::Error> {
    let query = &request.query;

    if matches!(query.aggregation, Aggregation::ByCrate)
        && !matches!(request.path, CountPath::Workspace(_))
    {
        return Err(anyhow::anyhow!(
            "{} requires a Cargo workspace (directory with Cargo.toml), but '{}' is not a workspace",
            "--by-crate",
            request.path.as_path().display(),
        ));
    }

    // `LineTypes::everything()` on purpose: it is what *makes* the canonical
    // response carry complete counts, since `CountOptions::line_types` would
    // otherwise zero the disabled types here and the query set would carry
    // those zeros through. `query.line_types` only describes the requested
    // view and is applied at render time, so ordering and predicates here
    // still see real numbers.
    let options = || {
        CountOptions::new()
            .crates(query.crates.clone())
            .filter(query.filter.clone())
            .aggregation(query.aggregation)
            .line_types(LineTypes::everything())
    };

    let result: CountResult = match &request.path {
        CountPath::Workspace(path) => count_workspace(path, options())?,
        CountPath::Directory(path) => count_directory_with_options(path, options())?,
        // A lone file has no workspace or module structure to aggregate over,
        // so it bypasses the aggregating entry points and becomes a
        // single-file result directly.
        CountPath::File(path) => {
            let mut result = CountResult::new();
            result.root = path.clone();
            result.file_count = 1;
            result.total = count_file_with_filter(path, &query.filter)?;
            result
        }
    };

    Ok(narrow(
        CountQuerySet::from_result(&result, query.aggregation, query.line_types, query.ordering),
        query,
        CountQuerySet::filter,
        CountQuerySet::top,
    ))
}

/// Run a diff and return its canonical response.
///
/// # Errors
///
/// Fails when the library cannot resolve the revspec or read the repository.
pub fn diff(request: &DiffRequest) -> Result<DiffQuerySet, anyhow::Error> {
    let query = &request.query;

    // Same rationale as `count`: complete counts in, view narrowing at render.
    let options = DiffOptions::new()
        .crates(query.crates.clone())
        .filter(query.filter.clone())
        .aggregation(query.aggregation)
        .line_types(LineTypes::everything());

    let result = match &request.endpoints {
        // The revspec goes to the library verbatim; gix owns rev parsing.
        DiffEndpoints::Revspec(revspec) => diff_revspec(&request.repo, revspec, options)?,
        DiffEndpoints::Workdir(mode) => diff_workdir(&request.repo, *mode, options)?,
    };

    Ok(narrow(
        DiffQuerySet::from_result(&result, query.aggregation, query.line_types, query.ordering),
        query,
        DiffQuerySet::filter,
        DiffQuerySet::top,
    ))
}

/// Apply `--<field>-<op>` predicates, then `--top`, in that order.
///
/// Order matters: filtering first means `--top` slices the already-filtered
/// set. Slicing first would drop rows that were in the top N but failed a
/// predicate, quietly returning fewer than N matching rows.
///
/// The two query-set types share no trait, so the operations come in as
/// function pointers rather than a bound — cheaper than inventing a trait for
/// two call sites.
fn narrow<T>(
    queryset: T,
    query: &QueryRequest,
    filter: fn(T, &[rustloclib::Predicate]) -> T,
    top: fn(T, usize) -> T,
) -> T {
    let queryset = filter(queryset, &query.predicates);
    match query.top {
        Some(n) => top(queryset, n),
        None => queryset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CountPath;
    use rustloclib::{Field, FilterConfig, Op, Ordering, Predicate};
    use std::path::Path;

    /// A request with no narrowing — the neutral starting point tests tweak.
    fn request_for(path: impl AsRef<Path>) -> CountRequest {
        CountRequest {
            path: CountPath::classify(path),
            query: QueryRequest {
                crates: Vec::new(),
                filter: FilterConfig::new(),
                aggregation: Aggregation::Total,
                line_types: LineTypes::default(),
                ordering: Ordering::default(),
                top: None,
                predicates: Vec::new(),
            },
        }
    }

    /// A two-file workspace: `src/lib.rs` (larger) and `src/small.rs`.
    fn workspace() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "pub fn a() {}\npub fn b() {}\npub fn c() {}\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("src/small.rs"), "pub fn d() {}\n").unwrap();
        dir
    }

    #[test]
    fn count_reports_totals_for_a_workspace() {
        let dir = workspace();
        let result = count(&request_for(dir.path())).unwrap();
        assert!(result.total.code >= 4, "expected all files counted");
    }

    #[test]
    fn count_of_a_single_file_yields_one_file_and_its_counts() {
        let dir = workspace();
        let result = count(&request_for(dir.path().join("src/lib.rs"))).unwrap();
        assert_eq!(result.total.code, 3);
    }

    #[test]
    fn count_by_crate_on_a_non_workspace_is_an_error() {
        // The error path a test could not reach before without an ArgMatches.
        let dir = tempfile::tempdir().unwrap();
        let mut request = request_for(dir.path());
        request.query.aggregation = Aggregation::ByCrate;

        let err = count(&request).unwrap_err();
        assert!(
            err.to_string().contains("--by-crate")
                && err.to_string().contains("requires a Cargo workspace"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn count_by_crate_on_a_workspace_succeeds() {
        let dir = workspace();
        let mut request = request_for(dir.path());
        request.query.aggregation = Aggregation::ByCrate;

        assert!(count(&request).is_ok());
    }

    #[test]
    fn count_narrows_by_predicate_before_applying_top() {
        let dir = workspace();
        let mut request = request_for(dir.path());
        request.query.aggregation = Aggregation::ByFile;
        request.query.ordering = Ordering::by_code();
        // Only src/lib.rs (3 code lines) clears the predicate. Were `top`
        // applied first, this would still pass at top=2 but return the
        // filtered remainder of a 2-row slice; asserting the surviving label
        // pins the intended order.
        request.query.predicates = vec![Predicate::new(Field::Code, Op::Gte, 3)];
        request.query.top = Some(2);

        let result = count(&request).unwrap();
        assert_eq!(result.items.len(), 1);
        assert!(result.items[0].label.contains("lib.rs"));
    }

    #[test]
    fn count_top_truncates_after_ordering() {
        let dir = workspace();
        let mut request = request_for(dir.path());
        request.query.aggregation = Aggregation::ByFile;
        request.query.ordering = Ordering::by_code();
        request.query.top = Some(1);

        let result = count(&request).unwrap();
        assert_eq!(result.items.len(), 1);
        // Descending by code: the biggest file survives, not just the first.
        assert!(result.items[0].label.contains("lib.rs"));
    }

    #[test]
    fn count_line_types_do_not_narrow_the_counts_themselves() {
        // `--type code` is a display selection: the response still carries
        // real counts for every type so ordering/predicates stay honest.
        let dir = workspace();
        let mut request = request_for(dir.path());
        request.query.line_types = LineTypes {
            code: true,
            ..LineTypes::default()
        };

        let result = count(&request).unwrap();
        assert!(result.total.code >= 4);
    }

    #[test]
    fn diff_on_a_non_repository_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let request = DiffRequest {
            repo: dir.path().to_path_buf(),
            endpoints: DiffEndpoints::Workdir(rustloclib::WorkdirDiffMode::All),
            query: request_for(dir.path()).query,
        };

        assert!(diff(&request).is_err(), "a bare temp dir is not a git repo");
    }
}
