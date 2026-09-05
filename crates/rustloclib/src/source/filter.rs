//! File filtering and discovery with glob pattern support.
//!
//! Discovery walks a tree and returns every file an enabled language backend
//! supports. [`FilterConfig`] then narrows that candidate set with include and
//! exclude globs.
//!
//! ## Globs match the path form the report prints
//!
//! A glob is written the way a report labels its rows: relative to the root of
//! what was analyzed — the Cargo workspace root, the counted directory, the
//! counted file's own directory, or, for diffs, the repository root, since git
//! already reports repository-relative paths. [`FilterConfig::relative_to`]
//! records that root and [`FilterConfig::matches`] strips it before matching,
//! so `-e 'crates/docs/**'` selects the same files under `count` as under
//! `diff`. A path that does not lie under the root is matched in the form it
//! arrives in.
//!
//! A glob that matches none of the candidates narrows nothing (exclude) or
//! everything (include), which reads like an empty project rather than a
//! mistyped pattern. [`FilterConfig::unmatched_globs`] names those globs so a
//! caller can say so.

use std::path::{Path, PathBuf};

use glob::Pattern;
use walkdir::WalkDir;

use crate::data::{BackendRegistry, LanguageSelection};
use crate::error::RustlocError;
use crate::Result;

/// A compiled glob paired with the text the user typed.
///
/// The source text is kept so a warning can quote the pattern back exactly as
/// written; `Pattern`'s own `Display` is a reconstruction, not the input.
#[derive(Debug, Clone)]
struct GlobPattern {
    source: String,
    pattern: Pattern,
}

impl GlobPattern {
    fn new(source: &str) -> Result<Self> {
        let pattern = Pattern::new(source).map_err(|e| RustlocError::InvalidGlob {
            pattern: source.to_string(),
            message: e.to_string(),
        })?;
        Ok(Self {
            source: source.to_string(),
            pattern,
        })
    }
}

/// Configuration for file filtering.
#[derive(Debug, Clone, Default)]
pub struct FilterConfig {
    /// Glob patterns to include (if empty, include all supported source files)
    include: Vec<GlobPattern>,
    /// Glob patterns to exclude
    exclude: Vec<GlobPattern>,
    /// Language backend groups to analyze.
    languages: LanguageSelection,
    /// Root the globs are written relative to; see the module docs.
    root: Option<PathBuf>,
}

impl FilterConfig {
    /// Create a new empty filter config (includes all supported source files).
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an include pattern.
    pub fn include(mut self, pattern: &str) -> Result<Self> {
        self.include.push(GlobPattern::new(pattern)?);
        Ok(self)
    }

    /// Add an exclude pattern.
    pub fn exclude(mut self, pattern: &str) -> Result<Self> {
        self.exclude.push(GlobPattern::new(pattern)?);
        Ok(self)
    }

    /// Add multiple include patterns.
    pub fn include_many(mut self, patterns: &[&str]) -> Result<Self> {
        for pattern in patterns {
            self = self.include(pattern)?;
        }
        Ok(self)
    }

    /// Add multiple exclude patterns.
    pub fn exclude_many(mut self, patterns: &[&str]) -> Result<Self> {
        for pattern in patterns {
            self = self.exclude(pattern)?;
        }
        Ok(self)
    }

    /// Set the language backend groups this filter accepts.
    pub fn languages(mut self, languages: LanguageSelection) -> Self {
        self.languages = languages;
        self
    }

    /// Match globs against paths relative to `root`.
    ///
    /// Every counting entry point sets this to the root its report labels rows
    /// against, which is what makes one written glob mean the same thing in a
    /// count and in a diff. Without a root, globs match whatever path form the
    /// caller supplies — the diff path, where git already yields
    /// repository-relative paths.
    pub fn relative_to(mut self, root: impl AsRef<Path>) -> Self {
        self.root = Some(root.as_ref().to_path_buf());
        self
    }

    /// Whether an enabled language backend can analyze this path at all.
    ///
    /// This is the support half of [`Self::matches`], separated because a diff
    /// counts the lines of unsupported files that changed instead of dropping
    /// them.
    pub fn supports(&self, path: &Path) -> bool {
        BackendRegistry::new().supports_path_with_languages(path, &self.languages)
    }

    /// Check if a path matches the filter criteria.
    ///
    /// A path matches if:
    /// 1. It is supported by a registered language backend
    /// 2. It matches at least one include pattern (or include is empty)
    /// 3. It doesn't match any exclude pattern
    ///
    /// Globs see the path relative to [`Self::relative_to`]'s root.
    pub fn matches(&self, path: &Path) -> bool {
        if !self.supports(path) {
            return false;
        }

        let candidate = self.glob_target(path);
        let candidate = candidate.to_string_lossy();

        // Check excludes first
        if self.exclude.iter().any(|p| p.pattern.matches(&candidate)) {
            return false;
        }

        // If no include patterns, include all
        if self.include.is_empty() {
            return true;
        }

        // Must match at least one include pattern
        self.include.iter().any(|p| p.pattern.matches(&candidate))
    }

    /// The globs, as the user wrote them, that none of `candidates` matched.
    ///
    /// `candidates` is the set of files the command actually considered —
    /// every supported source file it discovered, before include and exclude
    /// narrowed it. A glob absent from every candidate did not filter, it
    /// missed: usually a path form the run does not use (an absolute path, or
    /// a directory the walk never entered). Include and exclude globs are
    /// reported together and in the order they were configured.
    pub fn unmatched_globs<'a>(
        &self,
        candidates: impl IntoIterator<Item = &'a Path>,
    ) -> Vec<String> {
        let globs: Vec<&GlobPattern> = self.include.iter().chain(self.exclude.iter()).collect();
        if globs.is_empty() {
            return Vec::new();
        }

        let mut hit = vec![false; globs.len()];
        for path in candidates {
            let candidate = self.glob_target(path);
            let candidate = candidate.to_string_lossy();
            for (glob, hit) in globs.iter().zip(hit.iter_mut()) {
                *hit = *hit || glob.pattern.matches(&candidate);
            }
            // Every glob has proved itself; the rest of the walk cannot
            // change the answer.
            if hit.iter().all(|hit| *hit) {
                break;
            }
        }

        globs
            .iter()
            .zip(hit)
            .filter(|(_, hit)| !*hit)
            .map(|(glob, _)| glob.source.clone())
            .collect()
    }

    /// The path form globs are matched against: relative to the configured
    /// root when the path lies under it, and the path as given otherwise.
    fn glob_target<'a>(&'a self, path: &'a Path) -> &'a Path {
        match &self.root {
            Some(root) => path.strip_prefix(root).unwrap_or(path),
            None => path,
        }
    }
}

/// Check if a directory should be skipped during traversal.
fn should_skip_dir(name: &str) -> bool {
    // Skip hidden directories and target/
    name.starts_with('.') || name == "target"
}

/// Discover the files a filter could analyze under `root`, before its globs.
///
/// The walk applies the language selection but not the include/exclude globs,
/// so the caller holds the same candidate set that
/// [`FilterConfig::unmatched_globs`] judges the globs against. Narrow it with
/// [`FilterConfig::matches`].
pub fn discover_candidates(root: impl AsRef<Path>, filter: &FilterConfig) -> Result<Vec<PathBuf>> {
    let root = root.as_ref();

    if !root.exists() {
        return Err(RustlocError::PathNotFound(root.to_path_buf()));
    }

    let mut files = Vec::new();

    if root.is_file() {
        if filter.supports(root) {
            files.push(root.to_path_buf());
        }
        return Ok(files);
    }

    let walker = WalkDir::new(root).follow_links(true).into_iter();

    for entry in walker.filter_entry(|e| {
        // Always include the root directory
        if e.depth() == 0 {
            return true;
        }
        // For non-root entries, skip hidden dirs and target/
        if e.file_type().is_dir() {
            let name = e.file_name().to_str().unwrap_or("");
            return !should_skip_dir(name);
        }
        // Include files
        true
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();

        if path.is_file() && filter.supports(path) {
            files.push(path.to_path_buf());
        }
    }

    // Sort for deterministic output
    files.sort();

    Ok(files)
}

/// Discover candidates across several directories, deduplicated and sorted.
pub fn discover_candidates_in_dirs(dirs: &[&Path], filter: &FilterConfig) -> Result<Vec<PathBuf>> {
    let mut all_files = Vec::new();

    for dir in dirs {
        all_files.extend(discover_candidates(dir, filter)?);
    }

    all_files.sort();
    all_files.dedup();

    Ok(all_files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn create_test_files(dir: &Path) {
        // Create directory structure
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::create_dir_all(dir.join("src/utils")).unwrap();
        fs::create_dir_all(dir.join("tests")).unwrap();
        fs::create_dir_all(dir.join("examples")).unwrap();
        fs::create_dir_all(dir.join("target/debug")).unwrap();
        fs::create_dir_all(dir.join(".hidden")).unwrap();

        // Create files
        fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(dir.join("src/lib.rs"), "pub mod utils;").unwrap();
        fs::write(dir.join("src/utils/mod.rs"), "pub fn util() {}").unwrap();
        fs::write(dir.join("src/utils/helper.rs"), "pub fn help() {}").unwrap();
        fs::write(dir.join("tests/integration.rs"), "#[test] fn test() {}").unwrap();
        fs::write(dir.join("examples/demo.rs"), "fn main() {}").unwrap();
        fs::write(dir.join("target/debug/build.rs"), "// generated").unwrap();
        fs::write(dir.join(".hidden/secret.rs"), "// hidden").unwrap();
        fs::write(dir.join("README.md"), "# Readme").unwrap();
    }

    /// The candidate set a filter's globs get judged against.
    fn discovered(root: &Path, filter: &FilterConfig) -> Vec<PathBuf> {
        let mut files = discover_candidates(root, filter).unwrap();
        files.retain(|path| filter.matches(path));
        files
    }

    #[test]
    fn test_filter_matches_supported_source_files() {
        let filter = FilterConfig::new();

        assert!(filter.matches(Path::new("src/main.rs")));
        assert!(filter.matches(Path::new("lib.rs")));
        assert!(!filter.matches(Path::new("src/app.py")));
        assert!(!filter.matches(Path::new("tests/app.test.js")));
        assert!(!filter.matches(Path::new("tests/app.test.ts")));
        assert!(!filter.matches(Path::new("README.md")));
        assert!(!filter.matches(Path::new("Cargo.toml")));
    }

    #[test]
    fn test_filter_matches_selected_languages() {
        let python_filter = FilterConfig::new().languages(crate::data::LanguageSelection::new(&[
            crate::data::LanguageName::Python,
        ]));
        let typescript_filter =
            FilterConfig::new().languages(crate::data::LanguageSelection::new(&[
                crate::data::LanguageName::TypeScript,
            ]));

        assert!(python_filter.matches(Path::new("src/app.py")));
        assert!(!python_filter.matches(Path::new("src/main.rs")));
        assert!(!python_filter.matches(Path::new("src/app.ts")));
        assert!(typescript_filter.matches(Path::new("src/app.ts")));
        assert!(typescript_filter.matches(Path::new("src/component.tsx")));
        assert!(!typescript_filter.matches(Path::new("src/main.rs")));
        assert!(!typescript_filter.matches(Path::new("src/app.py")));
    }

    #[test]
    fn test_filter_matches_all_available_languages() {
        let filter = FilterConfig::new().languages(crate::data::LanguageSelection::all());

        assert!(filter.matches(Path::new("src/main.rs")));
        assert!(filter.matches(Path::new("src/app.py")));
        assert!(filter.matches(Path::new("tests/app.test.js")));
        assert!(filter.matches(Path::new("tests/app.test.ts")));
    }

    #[test]
    fn test_filter_with_include_pattern() {
        let filter = FilterConfig::new().include("**/utils/*.rs").unwrap();

        assert!(filter.matches(Path::new("src/utils/mod.rs")));
        assert!(filter.matches(Path::new("src/utils/helper.rs")));
        assert!(!filter.matches(Path::new("src/main.rs")));
        assert!(!filter.matches(Path::new("src/lib.rs")));
    }

    #[test]
    fn test_filter_with_exclude_pattern() {
        let filter = FilterConfig::new().exclude("**/tests/**").unwrap();

        assert!(filter.matches(Path::new("src/main.rs")));
        assert!(!filter.matches(Path::new("tests/integration.rs")));
        assert!(!filter.matches(Path::new("src/tests/test.rs")));
    }

    #[test]
    fn test_filter_with_multiple_patterns() {
        let filter = FilterConfig::new()
            .include_many(&["**/src/**", "**/tests/**"])
            .unwrap()
            .exclude("**/utils/**")
            .unwrap();

        assert!(filter.matches(Path::new("project/src/main.rs")));
        assert!(filter.matches(Path::new("project/tests/test.rs")));
        assert!(!filter.matches(Path::new("project/src/utils/helper.rs")));
        assert!(!filter.matches(Path::new("project/examples/demo.rs")));
    }

    /// The bug this module's path form exists to prevent: an absolute path
    /// makes a root-relative glob miss, so `-e 'src/**'` excluded nothing.
    #[test]
    fn test_globs_match_paths_relative_to_the_root() {
        let filter = FilterConfig::new()
            .exclude("src/utils/**")
            .unwrap()
            .relative_to("/ws");

        assert!(!filter.matches(Path::new("/ws/src/utils/helper.rs")));
        assert!(filter.matches(Path::new("/ws/src/main.rs")));
    }

    /// A path outside the root keeps its own form — a workspace member living
    /// beside the root still has to be matchable.
    #[test]
    fn test_a_path_outside_the_root_matches_as_given() {
        let filter = FilterConfig::new()
            .include("outside/**")
            .unwrap()
            .relative_to("/ws");

        assert!(filter.matches(Path::new("outside/src/lib.rs")));
        assert!(!filter.matches(Path::new("/elsewhere/src/lib.rs")));
    }

    /// The count and diff path forms agree: one written glob, one meaning.
    #[test]
    fn test_the_same_glob_selects_the_same_repository_relative_path() {
        let counted = FilterConfig::new()
            .exclude("crates/docs/**")
            .unwrap()
            .relative_to("/repo");
        let diffed = FilterConfig::new().exclude("crates/docs/**").unwrap();

        assert!(!counted.matches(Path::new("/repo/crates/docs/src/lib.rs")));
        assert!(!diffed.matches(Path::new("crates/docs/src/lib.rs")));
    }

    #[test]
    fn test_unmatched_globs_names_only_the_globs_that_found_nothing() {
        let filter = FilterConfig::new()
            .include("src/**")
            .unwrap()
            .exclude("generated/**")
            .unwrap()
            .relative_to("/ws");

        let candidates = [
            PathBuf::from("/ws/src/main.rs"),
            PathBuf::from("/ws/src/lib.rs"),
        ];

        assert_eq!(
            filter.unmatched_globs(candidates.iter().map(PathBuf::as_path)),
            vec!["generated/**".to_string()]
        );
    }

    #[test]
    fn test_unmatched_globs_quotes_the_pattern_as_written() {
        let filter = FilterConfig::new().include("crates/docs/**").unwrap();

        assert_eq!(
            filter.unmatched_globs([Path::new("crates/app/src/lib.rs")]),
            vec!["crates/docs/**".to_string()]
        );
    }

    #[test]
    fn test_a_filter_without_globs_has_nothing_unmatched() {
        let filter = FilterConfig::new();

        assert!(filter
            .unmatched_globs([Path::new("src/main.rs")])
            .is_empty());
    }

    #[test]
    fn test_discover_files() {
        let temp = tempdir().unwrap();
        create_test_files(temp.path());

        let filter = FilterConfig::new();
        let files = discovered(temp.path(), &filter);

        // Should find all .rs files except in target/ and .hidden/
        assert!(files.iter().any(|p| p.ends_with("src/main.rs")));
        assert!(files.iter().any(|p| p.ends_with("src/lib.rs")));
        assert!(files.iter().any(|p| p.ends_with("src/utils/mod.rs")));
        assert!(files.iter().any(|p| p.ends_with("tests/integration.rs")));
        assert!(files.iter().any(|p| p.ends_with("examples/demo.rs")));

        // Should not find files in target/ or .hidden/
        assert!(!files.iter().any(|p| p.to_string_lossy().contains("target")));
        assert!(!files
            .iter()
            .any(|p| p.to_string_lossy().contains(".hidden")));
    }

    #[test]
    fn test_discover_files_with_filter() {
        let temp = tempdir().unwrap();
        create_test_files(temp.path());

        let filter = FilterConfig::new()
            .exclude("**/tests/**")
            .unwrap()
            .exclude("**/examples/**")
            .unwrap();

        let files = discovered(temp.path(), &filter);

        // Should find src files only
        assert!(files.iter().any(|p| p.ends_with("src/main.rs")));
        assert!(!files.iter().any(|p| p.ends_with("tests/integration.rs")));
        assert!(!files.iter().any(|p| p.ends_with("examples/demo.rs")));
    }

    /// Candidates ignore the globs on purpose: they are what the globs get
    /// judged against, so an exclude must not erase its own evidence.
    #[test]
    fn test_candidates_ignore_the_globs() {
        let temp = tempdir().unwrap();
        create_test_files(temp.path());

        let filter = FilterConfig::new()
            .exclude("**/tests/**")
            .unwrap()
            .relative_to(temp.path());

        let candidates = discover_candidates(temp.path(), &filter).unwrap();

        assert!(candidates
            .iter()
            .any(|p| p.ends_with("tests/integration.rs")));
        assert!(filter
            .unmatched_globs(candidates.iter().map(PathBuf::as_path))
            .is_empty());
    }

    #[test]
    fn test_discover_single_file() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("test.rs");
        fs::write(&file_path, "fn test() {}").unwrap();

        let filter = FilterConfig::new();
        let files = discovered(&file_path, &filter);

        assert_eq!(files.len(), 1);
        assert_eq!(files[0], file_path);
    }

    #[test]
    fn test_discover_files_nonexistent() {
        let filter = FilterConfig::new();
        let result = discover_candidates("/nonexistent/path", &filter);

        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_glob_pattern() {
        let result = FilterConfig::new().include("[invalid");

        assert!(result.is_err());
        if let Err(RustlocError::InvalidGlob { pattern, .. }) = result {
            assert_eq!(pattern, "[invalid");
        } else {
            panic!("Expected InvalidGlob error");
        }
    }
}
