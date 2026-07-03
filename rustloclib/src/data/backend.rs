//! Language backend seam for source analysis.
//!
//! Backends classify source files into rustloc's shared [`Locs`] model. The
//! Rust remains the deepest backend, while the generic backend gives other
//! common source files file-level code/test/example classification until
//! language-specific backends are added.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Result, RustlocError};

use super::stats::Locs;
use super::visitor::{gather_stats, gather_stats_for_path};

/// Language identified by a backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LanguageId {
    Rust,
    Python,
    External(String),
    Unknown,
}

/// Context for executable or logical code lines.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogicContext {
    /// Production code.
    Code,
    /// Test code.
    Tests,
    /// Example code.
    Example,
}

impl LogicContext {
    /// Determine the default logic context from a file path.
    ///
    /// These rules are Rust-oriented today and are used by the Rust backend.
    /// Future language backends can choose their own path and syntax rules.
    pub fn from_file_path(path: impl AsRef<Path>) -> Self {
        for component in path.as_ref().components() {
            match component {
                std::path::Component::Normal(os_str)
                    if os_str == "tests" || os_str == "tests.rs" =>
                {
                    return Self::Tests;
                }
                std::path::Component::Normal(os_str) if os_str == "examples" => {
                    return Self::Example;
                }
                _ => {}
            }
        }

        Self::Code
    }
}

/// Classification for a single source line.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineClass {
    Logic(LogicContext),
    Docs,
    Comments,
    Blanks,
}

impl LineClass {
    /// Record this line classification into aggregate stats.
    pub fn record(self, stats: &mut Locs) {
        stats.total += 1;
        match self {
            Self::Logic(LogicContext::Code) => stats.code += 1,
            Self::Logic(LogicContext::Tests) => stats.tests += 1,
            Self::Logic(LogicContext::Example) => stats.examples += 1,
            Self::Docs => stats.docs += 1,
            Self::Comments => stats.comments += 1,
            Self::Blanks => stats.blanks += 1,
        }
    }
}

/// Normalized analysis for one source file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileAnalysis {
    pub language: LanguageId,
    pub stats: Locs,
}

/// Backend interface for language-specific source analysis.
pub trait LanguageBackend: Sync {
    fn supports_path(&self, path: &Path) -> bool;

    fn analyze_path(&self, path: &Path) -> Result<FileAnalysis> {
        let source = std::fs::read_to_string(path).map_err(|e| RustlocError::FileRead {
            path: path.to_path_buf(),
            source: e,
        })?;
        self.analyze_source(path, &source)
    }

    fn analyze_source(&self, path: &Path, source: &str) -> Result<FileAnalysis>;
}

/// Rust language backend.
#[derive(Debug, Default)]
pub struct RustBackend;

impl LanguageBackend for RustBackend {
    fn supports_path(&self, path: &Path) -> bool {
        path.extension().is_some_and(|ext| ext == "rs")
    }

    fn analyze_path(&self, path: &Path) -> Result<FileAnalysis> {
        Ok(FileAnalysis {
            language: LanguageId::Rust,
            stats: gather_stats_for_path(path)?,
        })
    }

    fn analyze_source(&self, path: &Path, source: &str) -> Result<FileAnalysis> {
        let context = LogicContext::from_file_path(path);
        Ok(FileAnalysis {
            language: LanguageId::Rust,
            stats: gather_stats(source, context),
        })
    }
}

/// File-level backend for common source languages without semantic parsing.
#[derive(Debug, Default)]
pub struct GenericBackend;

#[derive(Debug, Clone, Copy)]
struct GenericLanguage {
    id: &'static str,
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
}

impl GenericLanguage {
    fn for_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        let language = match ext.as_str() {
            "py" | "pyw" => Self {
                id: "Python",
                line_comments: &["#"],
                block_comment: None,
            },
            "sh" | "bash" | "zsh" | "fish" => Self {
                id: "Shell",
                line_comments: &["#"],
                block_comment: None,
            },
            "rb" => Self {
                id: "Ruby",
                line_comments: &["#"],
                block_comment: None,
            },
            "js" | "jsx" => Self {
                id: "JavaScript",
                line_comments: &["//"],
                block_comment: Some(("/*", "*/")),
            },
            "ts" | "tsx" => Self {
                id: "TypeScript",
                line_comments: &["//"],
                block_comment: Some(("/*", "*/")),
            },
            "go" => Self {
                id: "Go",
                line_comments: &["//"],
                block_comment: Some(("/*", "*/")),
            },
            "java" => Self {
                id: "Java",
                line_comments: &["//"],
                block_comment: Some(("/*", "*/")),
            },
            "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => Self {
                id: "C-like",
                line_comments: &["//"],
                block_comment: Some(("/*", "*/")),
            },
            "cs" => Self {
                id: "CSharp",
                line_comments: &["//"],
                block_comment: Some(("/*", "*/")),
            },
            "php" => Self {
                id: "PHP",
                line_comments: &["//", "#"],
                block_comment: Some(("/*", "*/")),
            },
            "swift" => Self {
                id: "Swift",
                line_comments: &["//"],
                block_comment: Some(("/*", "*/")),
            },
            "kt" | "kts" => Self {
                id: "Kotlin",
                line_comments: &["//"],
                block_comment: Some(("/*", "*/")),
            },
            "scala" => Self {
                id: "Scala",
                line_comments: &["//"],
                block_comment: Some(("/*", "*/")),
            },
            "css" | "scss" | "less" => Self {
                id: "CSS",
                line_comments: &[],
                block_comment: Some(("/*", "*/")),
            },
            _ => return None,
        };
        Some(language)
    }
}

impl LanguageBackend for GenericBackend {
    fn supports_path(&self, path: &Path) -> bool {
        GenericLanguage::for_path(path).is_some()
    }

    fn analyze_source(&self, path: &Path, source: &str) -> Result<FileAnalysis> {
        let language = GenericLanguage::for_path(path)
            .ok_or_else(|| RustlocError::NotRustFile(path.to_path_buf()))?;
        let context = generic_context_from_path(path);
        let mut stats = Locs::new();
        let mut in_block_comment = false;

        for line in source.lines() {
            classify_generic_line(line, language, &mut in_block_comment, context)
                .record(&mut stats);
        }

        Ok(FileAnalysis {
            language: LanguageId::External(language.id.to_string()),
            stats,
        })
    }
}

fn generic_context_from_path(path: &Path) -> LogicContext {
    let mut saw_example_dir = false;
    for component in path.components() {
        let Some(value) = component.as_os_str().to_str() else {
            continue;
        };
        let value = value.to_ascii_lowercase();
        match value.as_str() {
            "tests" | "test" | "__tests__" | "spec" | "specs" => return LogicContext::Tests,
            "examples" | "example" | "samples" | "sample" => saw_example_dir = true,
            _ => {}
        }
    }

    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.starts_with("spec_")
        || stem.ends_with("_spec")
        || filename.contains(".test.")
        || filename.contains(".spec.")
    {
        LogicContext::Tests
    } else if saw_example_dir || stem.starts_with("example_") || stem.ends_with("_example") {
        LogicContext::Example
    } else {
        LogicContext::Code
    }
}

fn classify_generic_line(
    line: &str,
    language: GenericLanguage,
    in_block_comment: &mut bool,
    context: LogicContext,
) -> LineClass {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return LineClass::Blanks;
    }

    if *in_block_comment {
        if let Some((_, end)) = language.block_comment {
            if trimmed.contains(end) {
                *in_block_comment = false;
            }
        }
        return LineClass::Comments;
    }

    if language
        .line_comments
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        return LineClass::Comments;
    }

    if let Some((start, end)) = language.block_comment {
        if trimmed.starts_with(start) {
            if !trimmed.contains(end) {
                *in_block_comment = true;
            }
            return LineClass::Comments;
        }
    }

    LineClass::Logic(context)
}

/// Registry of language backends available to the analyzer.
#[derive(Debug, Default)]
pub struct BackendRegistry {
    rust: RustBackend,
    generic: GenericBackend,
}

impl BackendRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn backend_for_path(&self, path: &Path) -> Option<&dyn LanguageBackend> {
        let backends: [&dyn LanguageBackend; 2] = [&self.rust, &self.generic];
        backends
            .into_iter()
            .find(|backend| backend.supports_path(path))
    }

    pub fn supports_path(&self, path: &Path) -> bool {
        self.backend_for_path(path).is_some()
    }

    pub fn analyze_source(&self, path: &Path, source: &str) -> Result<Option<FileAnalysis>> {
        self.backend_for_path(path)
            .map(|backend| backend.analyze_source(path, source))
            .transpose()
    }

    pub fn analyze_path(&self, path: &Path) -> Result<Option<FileAnalysis>> {
        self.backend_for_path(path)
            .map(|backend| backend.analyze_path(path))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_selects_rust_backend_for_rs_files() {
        let registry = BackendRegistry::new();

        assert!(registry.supports_path(Path::new("src/lib.rs")));
        assert!(registry.backend_for_path(Path::new("src/lib.rs")).is_some());
    }

    #[test]
    fn registry_returns_none_for_unsupported_files() {
        let registry = BackendRegistry::new();

        assert!(!registry.supports_path(Path::new("README.md")));
        assert!(registry.backend_for_path(Path::new("README.md")).is_none());
    }

    #[test]
    fn registry_selects_generic_backend_for_common_source_files() {
        let registry = BackendRegistry::new();

        assert!(registry.supports_path(Path::new("src/app.py")));
        assert!(registry.supports_path(Path::new("src/app.js")));
        assert!(registry.supports_path(Path::new("src/main.go")));
    }

    #[test]
    fn generic_backend_classifies_whole_test_files() {
        let registry = BackendRegistry::new();
        let source = r#"# module comment

def test_widget():
    assert True
"#;

        let analysis = registry
            .analyze_source(Path::new("tests/test_widget.py"), source)
            .unwrap()
            .unwrap();

        assert_eq!(
            analysis.language,
            LanguageId::External("Python".to_string())
        );
        assert_eq!(analysis.stats.comments, 1);
        assert_eq!(analysis.stats.blanks, 1);
        assert_eq!(analysis.stats.tests, 2);
        assert_eq!(analysis.stats.code, 0);
    }

    #[test]
    fn generic_backend_counts_c_like_block_comments() {
        let registry = BackendRegistry::new();
        let source = r#"/*
 * comment
 */
function run() {
  return true;
}
"#;

        let analysis = registry
            .analyze_source(Path::new("src/app.js"), source)
            .unwrap()
            .unwrap();

        assert_eq!(analysis.stats.comments, 3);
        assert_eq!(analysis.stats.code, 3);
    }

    #[test]
    fn rust_backend_preserves_same_file_test_classification() {
        let registry = BackendRegistry::new();
        let source = r#"pub fn prod() {}

#[cfg(test)]
mod tests {
    #[test]
    fn test_prod() {
        assert!(true);
    }
}
"#;

        let analysis = registry
            .analyze_source(Path::new("src/lib.rs"), source)
            .unwrap()
            .unwrap();

        assert_eq!(analysis.language, LanguageId::Rust);
        assert!(analysis.stats.code > 0);
        assert!(analysis.stats.tests > 0);
    }
}
