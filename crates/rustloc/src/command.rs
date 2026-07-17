//! Typed command requests — the parsing boundary.
//!
//! This module is the **only** place that interprets raw [`ArgMatches`] as
//! command logic. It converts CLI syntax into typed request values
//! ([`CountRequest`] / [`DiffRequest`]) that [`crate::application`] can
//! orchestrate without knowing clap exists.
//!
//! Two other modules touch `ArgMatches`, and neither is a competing reader:
//!
//! - [`crate::filter_args`] owns both ends of the synthetic `--<field>-<op>`
//!   grid — it registers the 42 hidden args and reads them back. Its
//!   `extract` is called *from here* ([`QueryRequest::from_matches`]), so the
//!   grid stays a detail of the module that invents it rather than 42 cases
//!   spelled out at this boundary.
//! - `crate::presentation` reads the single injected `_output_mode` arg at the
//!   render boundary. That is a render decision, not command logic.
//!
//! Handlers take `&ArgMatches` only to hand it straight to the constructors
//! below. So the rule to enforce is not "no other module names the type" but:
//! **CLI syntax becomes typed values here, and nothing downstream of
//! [`crate::application`] re-derives command logic from matches.**
//!
//! The split matters for two reasons:
//!
//! - **Testability.** Orchestration takes a request struct, so tests construct
//!   one directly instead of building an `ArgMatches` by hand.
//! - **Strictness.** Conversions that can fail (ordering, languages, globs,
//!   revspec/staged combinations) fail *here*, at parse time, with a usage
//!   error — rather than being swallowed downstream and silently replaced by a
//!   default.
//!
//! Presentation is deliberately absent: nothing here reads the output mode.
//! That decision belongs to `crate::presentation`, at the render boundary.

use std::path::{Path, PathBuf};

use clap::ArgMatches;
use rustloclib::{
    available_languages, default_languages, Aggregation, FilterConfig, LanguageName,
    LanguageSelection, LineTypes, OrderBy, OrderDirection, Ordering, Predicate, WorkdirDiffMode,
};

/// Parse an `--ordering` value (`code`, `-code`, `+label`) into an [`Ordering`].
///
/// Wired as a clap `value_parser`, so an unknown field or a bare direction
/// prefix is rejected as a **usage error before dispatch**. This is deliberate:
/// an earlier version parsed ordering inside the handler and fell back to
/// `Ordering::default()` on error, so `-o -coed` silently sorted by label and
/// exited 0 — the user got plausible-looking output for a request the tool
/// never honoured.
///
/// Direction rules: an explicit `-` (descending) or `+` (ascending) prefix wins;
/// otherwise numeric fields default to descending and `label` to ascending,
/// which is the useful default in each case ("biggest first", "A-Z").
pub fn parse_ordering(s: &str) -> Result<Ordering, String> {
    let (direction, field) = if let Some(stripped) = s.strip_prefix('-') {
        (OrderDirection::Descending, stripped)
    } else if let Some(stripped) = s.strip_prefix('+') {
        (OrderDirection::Ascending, stripped)
    } else {
        let by: OrderBy = s.parse()?;
        let direction = if by == OrderBy::Label {
            OrderDirection::Ascending
        } else {
            OrderDirection::Descending
        };
        return Ok(Ordering { by, direction });
    };

    Ok(Ordering {
        by: field.parse()?,
        direction,
    })
}

/// What the count target actually is on disk.
///
/// Classified once, at parse time, so orchestration selects a library entry
/// point by matching a variant instead of re-running filesystem probes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CountPath {
    /// A Cargo workspace: a directory holding `Cargo.toml`, or the manifest itself.
    Workspace(PathBuf),
    /// A single source file.
    File(PathBuf),
    /// A plain directory with no manifest.
    Directory(PathBuf),
}

impl CountPath {
    /// Classify a path string. Never fails: a nonexistent path classifies as
    /// [`CountPath::Directory`] and the library reports the I/O error, which
    /// keeps the "no such directory" message the user already gets.
    pub fn classify(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        let is_workspace = (path.is_dir() && path.join("Cargo.toml").exists())
            || (path.is_file() && path.file_name() == Some("Cargo.toml".as_ref()));

        if is_workspace {
            Self::Workspace(path.to_path_buf())
        } else if path.is_file() {
            Self::File(path.to_path_buf())
        } else {
            Self::Directory(path.to_path_buf())
        }
    }

    /// The underlying path, for library calls and error messages.
    pub fn as_path(&self) -> &Path {
        match self {
            Self::Workspace(p) | Self::File(p) | Self::Directory(p) => p,
        }
    }
}

/// Which two trees a diff compares.
///
/// Resolved at parse time so orchestration picks a library entry point by
/// matching a variant, and the mutually-exclusive-argument rules are enforced
/// once, where the CLI syntax is still in view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffEndpoints {
    /// A git revspec (`HEAD~5..HEAD`, `main...feature`, a single rev vs HEAD).
    /// Passed through to gix verbatim; we never parse revspec syntax ourselves.
    Revspec(String),
    /// The working tree against HEAD, in the given mode.
    Workdir(WorkdirDiffMode),
}

impl DiffEndpoints {
    /// Resolve the positional revs and `--staged` into one endpoint pair.
    ///
    /// Two positional args (`diff main feature`) are joined into `main..feature`
    /// so gix sees a single range expression. `--staged` is only meaningful for
    /// a working-tree diff, so combining it with revs is rejected rather than
    /// silently ignored.
    pub fn resolve(
        from: Option<&String>,
        to: Option<&String>,
        staged: bool,
    ) -> Result<Self, anyhow::Error> {
        let Some(from) = from else {
            return Ok(Self::Workdir(if staged {
                WorkdirDiffMode::Staged
            } else {
                WorkdirDiffMode::All
            }));
        };

        if staged {
            return Err(anyhow::anyhow!(
                "--staged/--cached can only be used without commit arguments"
            ));
        }

        let revspec = match to {
            Some(to) => {
                if from.contains("..") {
                    return Err(anyhow::anyhow!(
                        "Pass either a single range/revspec (e.g. `a..b`) \
                         or two revs as separate args (e.g. `a b`), not both."
                    ));
                }
                format!("{}..{}", from, to)
            }
            None => from.clone(),
        };
        Ok(Self::Revspec(revspec))
    }
}

/// The view controls count and diff share: what to read, how to group it, how
/// to sort and narrow it.
///
/// Shared because the semantics are genuinely identical on both sides — the
/// flags parse the same, mean the same, and feed the same query stage. What
/// differs between the commands (a count *path* vs a diff's *endpoints*) stays
/// out of here, in [`CountRequest`] / [`DiffRequest`].
#[derive(Debug, Clone)]
pub struct QueryRequest {
    /// Restrict a workspace count/diff to these crates. Empty means all.
    pub crates: Vec<String>,
    /// Language selection plus include/exclude globs.
    pub filter: FilterConfig,
    /// Result granularity.
    pub aggregation: Aggregation,
    /// Which line types the user asked to *see*. A display selection only: the
    /// canonical response always carries complete counts, so ordering and
    /// predicates always see real numbers.
    pub line_types: LineTypes,
    /// Sort field and direction.
    pub ordering: Ordering,
    /// Truncate to N rows after sorting.
    pub top: Option<usize>,
    /// Threshold filters from the `--<field>-<op> N` grid, AND-combined.
    pub predicates: Vec<Predicate>,
}

impl QueryRequest {
    /// Convert the shared count/diff flags out of `matches`.
    pub fn from_matches(matches: &ArgMatches) -> Result<Self, anyhow::Error> {
        Ok(Self {
            crates: matches
                .get_many::<String>("crates")
                .map(|v| v.cloned().collect())
                .unwrap_or_default(),
            filter: build_filter(matches)?,
            aggregation: aggregation_from_matches(matches),
            line_types: line_types_from_matches(matches),
            // clap already validated this via `parse_ordering`, so an absent
            // value means "not supplied", never "supplied but unparseable".
            ordering: matches
                .get_one::<Ordering>("ordering")
                .copied()
                .unwrap_or_default(),
            top: matches.get_one::<usize>("top").copied(),
            predicates: crate::filter_args::extract(matches),
        })
    }
}

/// A fully typed `count` invocation.
#[derive(Debug, Clone)]
pub struct CountRequest {
    /// What to count, already classified.
    pub path: CountPath,
    /// Shared view controls.
    pub query: QueryRequest,
}

impl CountRequest {
    /// Convert `matches` into a typed count request.
    pub fn from_matches(matches: &ArgMatches) -> Result<Self, anyhow::Error> {
        let path = matches
            .get_one::<String>("path")
            .map(|s| s.as_str())
            .unwrap_or(".");

        Ok(Self {
            path: CountPath::classify(path),
            query: QueryRequest::from_matches(matches)?,
        })
    }
}

/// A fully typed `diff` invocation.
#[derive(Debug, Clone)]
pub struct DiffRequest {
    /// Repository to diff in.
    pub repo: PathBuf,
    /// The two trees being compared.
    pub endpoints: DiffEndpoints,
    /// Shared view controls.
    pub query: QueryRequest,
}

impl DiffRequest {
    /// Convert `matches` into a typed diff request.
    pub fn from_matches(matches: &ArgMatches) -> Result<Self, anyhow::Error> {
        let repo = matches
            .get_one::<String>("path")
            .map(|s| s.as_str())
            .unwrap_or(".");

        Ok(Self {
            repo: PathBuf::from(repo),
            endpoints: DiffEndpoints::resolve(
                matches.get_one::<String>("from"),
                matches.get_one::<String>("to"),
                matches.get_flag("staged"),
            )?,
            query: QueryRequest::from_matches(matches)?,
        })
    }
}

/// The `--by-*` flags are mutually exclusive (clap enforces it), so the first
/// set flag wins and no flag means totals only.
fn aggregation_from_matches(matches: &ArgMatches) -> Aggregation {
    if matches.get_flag("by_file") {
        Aggregation::ByFile
    } else if matches.get_flag("by_module") {
        Aggregation::ByModule
    } else if matches.get_flag("by_crate") {
        Aggregation::ByCrate
    } else {
        Aggregation::Total
    }
}

/// Absent `--type` means "show everything" ([`LineTypes::default`]); otherwise
/// only the named types are displayed.
fn line_types_from_matches(matches: &ArgMatches) -> LineTypes {
    let types: Vec<&str> = matches
        .get_many::<String>("line_types")
        .map(|v| v.map(|s| s.as_str()).collect())
        .unwrap_or_default();

    if types.is_empty() {
        return LineTypes::default();
    }

    LineTypes {
        code: types.contains(&"code"),
        tests: types.contains(&"tests"),
        examples: types.contains(&"examples"),
        docs: types.contains(&"docs"),
        comments: types.contains(&"comments"),
        blanks: types.contains(&"blanks"),
        total: types.contains(&"total"),
    }
}

fn build_filter(matches: &ArgMatches) -> Result<FilterConfig, anyhow::Error> {
    let mut filter = FilterConfig::new().languages(languages_from_matches(matches)?);

    if let Some(includes) = matches.get_many::<String>("include") {
        for pattern in includes {
            filter = filter.include(pattern)?;
        }
    }

    if let Some(excludes) = matches.get_many::<String>("exclude") {
        for pattern in excludes {
            filter = filter.exclude(pattern)?;
        }
    }

    Ok(filter)
}

/// Absent `--lang` means the default backends; `all` is a shorthand for every
/// available backend group.
fn languages_from_matches(matches: &ArgMatches) -> Result<LanguageSelection, anyhow::Error> {
    let values: Vec<&str> = matches
        .get_many::<String>("languages")
        .map(|v| v.map(|s| s.as_str()).collect())
        .unwrap_or_default();

    if values.is_empty() {
        return Ok(LanguageSelection::new(default_languages()));
    }

    if values.iter().any(|value| value.eq_ignore_ascii_case("all")) {
        return Ok(LanguageSelection::new(available_languages()));
    }

    let mut languages = Vec::new();
    for value in values {
        languages.push(value.parse::<LanguageName>().map_err(anyhow::Error::msg)?);
    }
    Ok(LanguageSelection::new(&languages))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_plain_field_defaults_direction_per_field() {
        // Numeric fields read "biggest first"; label reads A-Z.
        assert_eq!(
            parse_ordering("code").unwrap(),
            Ordering {
                by: OrderBy::Code,
                direction: OrderDirection::Descending
            }
        );
        assert_eq!(
            parse_ordering("label").unwrap(),
            Ordering {
                by: OrderBy::Label,
                direction: OrderDirection::Ascending
            }
        );
    }

    #[test]
    fn ordering_explicit_prefix_wins_over_field_default() {
        assert_eq!(
            parse_ordering("+code").unwrap(),
            Ordering {
                by: OrderBy::Code,
                direction: OrderDirection::Ascending
            }
        );
        assert_eq!(
            parse_ordering("-label").unwrap(),
            Ordering {
                by: OrderBy::Label,
                direction: OrderDirection::Descending
            }
        );
    }

    #[test]
    fn ordering_rejects_unknown_field_in_every_prefix_form() {
        // The regression this guards: each of these used to fall back to
        // Ordering::default() and exit 0.
        for bad in ["coed", "-coed", "+coed", "", "-", "+"] {
            assert!(
                parse_ordering(bad).is_err(),
                "{bad:?} should be rejected, not defaulted"
            );
        }
    }

    #[test]
    fn count_path_classifies_workspace_dir_manifest_and_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Plain directory: no manifest.
        std::fs::create_dir(root.join("plain")).unwrap();
        assert!(matches!(
            CountPath::classify(root.join("plain")),
            CountPath::Directory(_)
        ));

        // A source file.
        std::fs::write(root.join("lib.rs"), "fn main() {}\n").unwrap();
        assert!(matches!(
            CountPath::classify(root.join("lib.rs")),
            CountPath::File(_)
        ));

        // Directory holding a manifest, and the manifest itself: both workspaces.
        std::fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        assert!(matches!(CountPath::classify(root), CountPath::Workspace(_)));
        assert!(matches!(
            CountPath::classify(root.join("Cargo.toml")),
            CountPath::Workspace(_)
        ));
    }

    #[test]
    fn count_path_classifies_missing_path_as_directory() {
        // The library owns the "no such directory" error; classification must
        // not pre-empt it with a different message.
        assert!(matches!(
            CountPath::classify("/definitely/not/here"),
            CountPath::Directory(_)
        ));
    }

    #[test]
    fn diff_endpoints_without_revs_is_a_workdir_diff() {
        assert_eq!(
            DiffEndpoints::resolve(None, None, false).unwrap(),
            DiffEndpoints::Workdir(WorkdirDiffMode::All)
        );
        assert_eq!(
            DiffEndpoints::resolve(None, None, true).unwrap(),
            DiffEndpoints::Workdir(WorkdirDiffMode::Staged)
        );
    }

    #[test]
    fn diff_endpoints_passes_revspecs_through_and_joins_two_revs() {
        let range = "HEAD~5..HEAD".to_string();
        assert_eq!(
            DiffEndpoints::resolve(Some(&range), None, false).unwrap(),
            DiffEndpoints::Revspec("HEAD~5..HEAD".to_string())
        );

        let (from, to) = ("main".to_string(), "feature".to_string());
        assert_eq!(
            DiffEndpoints::resolve(Some(&from), Some(&to), false).unwrap(),
            DiffEndpoints::Revspec("main..feature".to_string())
        );
    }

    #[test]
    fn diff_endpoints_rejects_staged_with_revs() {
        let from = "HEAD~1".to_string();
        let err = DiffEndpoints::resolve(Some(&from), None, true).unwrap_err();
        assert!(err.to_string().contains("--staged"));
    }

    #[test]
    fn diff_endpoints_rejects_range_plus_second_rev() {
        // `diff a..b c` is ambiguous — reject rather than guess.
        let (from, to) = ("a..b".to_string(), "c".to_string());
        let err = DiffEndpoints::resolve(Some(&from), Some(&to), false).unwrap_err();
        assert!(err.to_string().contains("not both"));
    }
}
