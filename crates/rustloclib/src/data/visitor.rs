//! Compatibility entry points for Rust source LOC analysis.
//!
//! The public visitor API predates the language backend registry. It now wraps
//! the parser-backed Rust classifier so existing callers keep the same entry
//! points while Rust parsing stays owned by the Rust backend.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::error::RustlocError;
use crate::Result;

use super::backend::{FileAnalysis, LanguageId, LineClass, LogicContext};
use super::rust::analyze_rust_source;
use super::stats::Locs;

/// Backwards-compatible name for the logic context used by the Rust visitor.
pub type VisitorContext = LogicContext;

/// A reader-backed compatibility wrapper for Rust LOC analysis.
///
/// The wrapper reads the source and delegates classification to Rustloc's
/// parser-backed Rust analyzer. Logic lines are counted in the supplied
/// context unless Rust item attributes classify the item as test-only.
pub struct Visitor<T: Read> {
    reader: T,
    context: VisitorContext,
}

impl Visitor<File> {
    /// Create a new visitor for a file at the given path.
    ///
    /// The context (code/tests/examples) is automatically determined from the path.
    pub fn new(file_path: impl AsRef<Path>) -> Result<Self> {
        let path = file_path.as_ref();
        let file = File::open(path).map_err(|e| RustlocError::FileRead {
            path: path.to_path_buf(),
            source: e,
        })?;
        let context = VisitorContext::from_file_path(path);

        Ok(Self {
            reader: file,
            context,
        })
    }
}

impl<T: Read> Visitor<T> {
    /// Create a visitor from any reader with a specified context.
    ///
    /// This is useful for testing without actual files.
    pub fn from_reader(reader: T, context: VisitorContext) -> Self {
        Self { reader, context }
    }

    /// Visit the file and return LOC statistics.
    pub fn visit_file(self) -> Locs {
        self.visit_file_analysis().0
    }

    /// Visit the file and return LOC statistics plus per-line classes.
    pub fn visit_file_analysis(mut self) -> (Locs, Vec<LineClass>) {
        let mut source = String::new();
        let _ = self.reader.read_to_string(&mut source);
        let analysis = analyze_rust_source(&source, self.context);
        (analysis.stats, analysis.line_classes)
    }
}

/// Gather LOC statistics for a file at the given path.
///
/// This is the primary entry point for analyzing a single Rust file. The
/// context (code/tests/examples) is automatically determined from the path.
///
/// # Example
///
/// ```rust
/// use rustloclib::gather_stats_for_path;
/// use std::fs;
/// use tempfile::tempdir;
///
/// let dir = tempdir().unwrap();
/// let file_path = dir.path().join("main.rs");
/// fs::write(&file_path, "fn main() {\n    println!(\"Hello\");\n}\n").unwrap();
///
/// let stats = gather_stats_for_path(&file_path).unwrap();
/// assert_eq!(stats.code, 3);
/// ```
pub fn gather_stats_for_path(path: impl AsRef<Path>) -> Result<Locs> {
    Ok(gather_analysis_for_path(path)?.stats)
}

/// Gather LOC statistics from a string of Rust source code.
///
/// The context determines how logic lines are categorized:
/// - `VisitorContext::Code` -> logic lines count as `code`
/// - `VisitorContext::Tests` -> logic lines count as `tests`
/// - `VisitorContext::Example` -> logic lines count as `examples`
///
/// Comments, docs, and blanks are always counted regardless of context.
///
/// # Example
///
/// ```rust
/// use rustloclib::{gather_stats, VisitorContext};
///
/// let source = r#"
/// fn main() {
///     println!("Hello");
/// }
/// "#;
///
/// let stats = gather_stats(source, VisitorContext::Code);
/// assert_eq!(stats.code, 3);
/// ```
pub fn gather_stats(source: &str, context: VisitorContext) -> Locs {
    gather_analysis(source, context).stats
}

pub(crate) fn gather_analysis_for_path(path: impl AsRef<Path>) -> Result<FileAnalysis> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path).map_err(|e| RustlocError::FileRead {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(gather_analysis(
        &source,
        VisitorContext::from_file_path(path),
    ))
}

pub(crate) fn gather_analysis(source: &str, context: VisitorContext) -> FileAnalysis {
    let mut analysis = analyze_rust_source(source, context);
    analysis.language = LanguageId::Rust;
    analysis
}

// Keep old names as aliases for backwards compatibility during transition.
#[doc(hidden)]
pub use gather_stats as parse_string;
#[doc(hidden)]
pub use gather_stats_for_path as parse_file;

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(file: &str) -> Locs {
        gather_stats(file, VisitorContext::Code)
    }

    #[test]
    fn empty_file() {
        let file = "\n";
        let stats = stats(file);

        assert_eq!(stats.blanks, 1);
        assert_eq!(stats.total(), 1);
    }

    #[test]
    fn single_code_line_without_trailing_newline() {
        let stats = stats("mod lib;");

        assert_eq!(stats.code, 1);
        assert_eq!(stats.total(), 1);
    }

    #[test]
    fn comments_and_blanks_are_context_independent() {
        let source = "// comment\n\n";

        let code_stats = gather_stats(source, VisitorContext::Code);
        let test_stats = gather_stats(source, VisitorContext::Tests);
        let example_stats = gather_stats(source, VisitorContext::Example);

        assert_eq!(code_stats.comments, 1);
        assert_eq!(test_stats.comments, 1);
        assert_eq!(example_stats.comments, 1);
        assert_eq!(code_stats.blanks, 1);
        assert_eq!(test_stats.blanks, 1);
        assert_eq!(example_stats.blanks, 1);
    }

    #[test]
    fn context_from_path() {
        assert_eq!(
            VisitorContext::from_file_path("src/lib.rs"),
            VisitorContext::Code
        );
        assert_eq!(
            VisitorContext::from_file_path("tests/integration.rs"),
            VisitorContext::Tests
        );
        assert_eq!(
            VisitorContext::from_file_path("examples/demo.rs"),
            VisitorContext::Example
        );
        assert_eq!(
            VisitorContext::from_file_path("src/tests.rs"),
            VisitorContext::Tests
        );
    }
}
