//! Input options for LOC counting and diffing operations.
//!
//! This module contains all configuration types that control what data
//! the library computes and returns.

use serde::{Deserialize, Serialize};

/// Filter for which code contexts to include in results.
///
/// When a context is disabled, it will be zeroed in returned stats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contexts {
    /// Include production code
    pub code: bool,
    /// Include test code
    pub tests: bool,
    /// Include example code
    pub examples: bool,
}

impl Default for Contexts {
    fn default() -> Self {
        Self::all()
    }
}

impl Contexts {
    /// Include all contexts (default)
    pub fn all() -> Self {
        Self {
            code: true,
            tests: true,
            examples: true,
        }
    }

    /// Include no contexts
    pub fn none() -> Self {
        Self {
            code: false,
            tests: false,
            examples: false,
        }
    }

    /// Include only production code
    pub fn code_only() -> Self {
        Self {
            code: true,
            tests: false,
            examples: false,
        }
    }

    /// Include only test code
    pub fn tests_only() -> Self {
        Self {
            code: false,
            tests: true,
            examples: false,
        }
    }

    /// Include only example code
    pub fn examples_only() -> Self {
        Self {
            code: false,
            tests: false,
            examples: true,
        }
    }

    /// Builder: set code inclusion
    pub fn with_code(mut self, include: bool) -> Self {
        self.code = include;
        self
    }

    /// Builder: set tests inclusion
    pub fn with_tests(mut self, include: bool) -> Self {
        self.tests = include;
        self
    }

    /// Builder: set examples inclusion
    pub fn with_examples(mut self, include: bool) -> Self {
        self.examples = include;
        self
    }
}

/// Aggregation level for results.
///
/// Controls what granularity of breakdown is included in results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Aggregation {
    /// Only return totals
    #[default]
    Total,
    /// Include per-crate breakdown
    ByCrate,
    /// Include per-module breakdown
    ByModule,
    /// Include per-file breakdown
    ByFile,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contexts_default() {
        let ctx = Contexts::default();
        assert!(ctx.code);
        assert!(ctx.tests);
        assert!(ctx.examples);
    }

    #[test]
    fn test_contexts_none() {
        let ctx = Contexts::none();
        assert!(!ctx.code);
        assert!(!ctx.tests);
        assert!(!ctx.examples);
    }

    #[test]
    fn test_contexts_builder() {
        let ctx = Contexts::none().with_code(true).with_tests(true);
        assert!(ctx.code);
        assert!(ctx.tests);
        assert!(!ctx.examples);
    }

    #[test]
    fn test_contexts_code_only() {
        let ctx = Contexts::code_only();
        assert!(ctx.code);
        assert!(!ctx.tests);
        assert!(!ctx.examples);
    }
}
