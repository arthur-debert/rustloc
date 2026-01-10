//! Input options for LOC counting and diffing operations.
//!
//! This module contains all configuration types that control what data
//! the library computes and returns.

use serde::{Deserialize, Serialize};

/// Filter for which line types to include in results.
///
/// When a line type is disabled, it will be zero in all returned stats,
/// and totals will only sum the enabled types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineTypes {
    /// Include code lines
    pub code: bool,
    /// Include blank lines
    pub blank: bool,
    /// Include doc comment lines
    pub docs: bool,
    /// Include regular comment lines
    pub comments: bool,
}

impl Default for LineTypes {
    fn default() -> Self {
        Self::all()
    }
}

impl LineTypes {
    /// Include all line types (default)
    pub fn all() -> Self {
        Self {
            code: true,
            blank: true,
            docs: true,
            comments: true,
        }
    }

    /// Include no line types
    pub fn none() -> Self {
        Self {
            code: false,
            blank: false,
            docs: false,
            comments: false,
        }
    }

    /// Include only code lines
    pub fn code_only() -> Self {
        Self {
            code: true,
            blank: false,
            docs: false,
            comments: false,
        }
    }

    /// Builder: set code inclusion
    pub fn with_code(mut self, include: bool) -> Self {
        self.code = include;
        self
    }

    /// Builder: set blank inclusion
    pub fn with_blank(mut self, include: bool) -> Self {
        self.blank = include;
        self
    }

    /// Builder: set docs inclusion
    pub fn with_docs(mut self, include: bool) -> Self {
        self.docs = include;
        self
    }

    /// Builder: set comments inclusion
    pub fn with_comments(mut self, include: bool) -> Self {
        self.comments = include;
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
    fn test_line_types_default() {
        let lt = LineTypes::default();
        assert!(lt.code);
        assert!(lt.blank);
        assert!(lt.docs);
        assert!(lt.comments);
    }

    #[test]
    fn test_line_types_none() {
        let lt = LineTypes::none();
        assert!(!lt.code);
        assert!(!lt.blank);
        assert!(!lt.docs);
        assert!(!lt.comments);
    }

    #[test]
    fn test_line_types_builder() {
        let lt = LineTypes::none().with_code(true).with_docs(true);
        assert!(lt.code);
        assert!(!lt.blank);
        assert!(lt.docs);
        assert!(!lt.comments);
    }
}
