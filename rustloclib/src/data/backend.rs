//! Language backend seam for source analysis.
//!
//! Backends classify source files into rustloc's shared [`Locs`] model. The
//! first implementation only registers Rust, but count and diff code route
//! through this registry so future Python or generic adapters can plug in
//! without changing aggregation, query, or output code.

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

/// Registry of language backends available to the analyzer.
#[derive(Debug, Default)]
pub struct BackendRegistry {
    rust: RustBackend,
}

impl BackendRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn backend_for_path(&self, path: &Path) -> Option<&dyn LanguageBackend> {
        let backends: [&dyn LanguageBackend; 1] = [&self.rust];
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

        assert!(!registry.supports_path(Path::new("src/lib.py")));
        assert!(registry.backend_for_path(Path::new("src/lib.py")).is_none());
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
