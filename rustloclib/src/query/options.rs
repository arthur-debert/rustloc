//! Input options for LOC counting and diffing operations.
//!
//! This module contains all configuration types that control what data
//! the library computes and returns.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Filter for which line types to include in results.
///
/// The 7 line types are:
/// - `code`: Production code logic lines
/// - `tests`: Test code logic lines
/// - `examples`: Example code logic lines
/// - `docs`: Documentation comments (anywhere)
/// - `comments`: Regular comments (anywhere)
/// - `blanks`: Blank lines (anywhere)
/// - `total`: Total line count (precomputed sum of all types)
///
/// When a line type is disabled, it will be zeroed in returned stats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineTypes {
    /// Include production code logic lines
    pub code: bool,
    /// Include test code logic lines
    pub tests: bool,
    /// Include example code logic lines
    pub examples: bool,
    /// Include documentation comment lines
    pub docs: bool,
    /// Include regular comment lines
    pub comments: bool,
    /// Include blank lines
    pub blanks: bool,
    /// Include total line count (precomputed)
    pub total: bool,
}

impl Default for LineTypes {
    fn default() -> Self {
        // Default shows code, tests, docs, and total
        Self {
            code: true,
            tests: true,
            examples: false,
            docs: true,
            comments: false,
            blanks: false,
            total: true,
        }
    }
}

impl LineTypes {
    /// Create with no types enabled (for building up).
    /// Note: `total` defaults to true since it's always useful to see totals.
    pub fn new() -> Self {
        Self {
            code: false,
            tests: false,
            examples: false,
            docs: false,
            comments: false,
            blanks: false,
            total: true, // All is on by default
        }
    }

    /// Include all line types
    pub fn everything() -> Self {
        Self {
            code: true,
            tests: true,
            examples: true,
            docs: true,
            comments: true,
            blanks: true,
            total: true,
        }
    }

    /// Include no line types at all (not even total)
    pub fn none() -> Self {
        Self {
            code: false,
            tests: false,
            examples: false,
            docs: false,
            comments: false,
            blanks: false,
            total: false,
        }
    }

    /// Include only production code (plus total)
    pub fn code_only() -> Self {
        Self::new().with_code()
    }

    /// Include only test code (plus total)
    pub fn tests_only() -> Self {
        Self::new().with_tests()
    }

    /// Include only example code (plus total)
    pub fn examples_only() -> Self {
        Self::new().with_examples()
    }

    /// Include all logic lines (code + tests + examples + all)
    pub fn logic_only() -> Self {
        Self {
            code: true,
            tests: true,
            examples: true,
            docs: false,
            comments: false,
            blanks: false,
            total: true,
        }
    }

    /// Builder: enable code
    pub fn with_code(mut self) -> Self {
        self.code = true;
        self
    }

    /// Builder: enable tests
    pub fn with_tests(mut self) -> Self {
        self.tests = true;
        self
    }

    /// Builder: enable examples
    pub fn with_examples(mut self) -> Self {
        self.examples = true;
        self
    }

    /// Builder: enable docs
    pub fn with_docs(mut self) -> Self {
        self.docs = true;
        self
    }

    /// Builder: enable comments
    pub fn with_comments(mut self) -> Self {
        self.comments = true;
        self
    }

    /// Builder: enable blanks
    pub fn with_blanks(mut self) -> Self {
        self.blanks = true;
        self
    }

    /// Builder: enable total
    pub fn with_total(mut self) -> Self {
        self.total = true;
        self
    }

    /// Builder: disable total
    pub fn without_total(mut self) -> Self {
        self.total = false;
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

/// Field to order results by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OrderBy {
    /// Order by label/path (lexicographical)
    #[default]
    Label,
    /// Order by code line count
    Code,
    /// Order by test line count
    Tests,
    /// Order by example line count
    Examples,
    /// Order by docs line count
    Docs,
    /// Order by comments line count
    Comments,
    /// Order by blanks line count
    Blanks,
    /// Order by total line count
    Total,
}

impl FromStr for OrderBy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "label" | "name" | "path" => Ok(OrderBy::Label),
            "code" => Ok(OrderBy::Code),
            "tests" | "test" => Ok(OrderBy::Tests),
            "examples" | "example" => Ok(OrderBy::Examples),
            "docs" | "doc" => Ok(OrderBy::Docs),
            "comments" | "comment" => Ok(OrderBy::Comments),
            "blanks" | "blank" => Ok(OrderBy::Blanks),
            "total" => Ok(OrderBy::Total),
            _ => Err(format!("Unknown order field: {}", s)),
        }
    }
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OrderDirection {
    /// Ascending (A-Z, smallest first)
    #[default]
    Ascending,
    /// Descending (Z-A, largest first)
    Descending,
}

/// Ordering configuration for results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ordering {
    /// Field to order by
    pub by: OrderBy,
    /// Sort direction
    pub direction: OrderDirection,
}

impl Default for Ordering {
    fn default() -> Self {
        Self {
            by: OrderBy::Label,
            direction: OrderDirection::Ascending,
        }
    }
}

impl Ordering {
    /// Create ordering by label ascending (default)
    pub fn by_label() -> Self {
        Self::default()
    }

    /// Create ordering by code count
    pub fn by_code() -> Self {
        Self {
            by: OrderBy::Code,
            direction: OrderDirection::Descending,
        }
    }

    /// Create ordering by test count
    pub fn by_tests() -> Self {
        Self {
            by: OrderBy::Tests,
            direction: OrderDirection::Descending,
        }
    }

    /// Create ordering by total count
    pub fn by_total() -> Self {
        Self {
            by: OrderBy::Total,
            direction: OrderDirection::Descending,
        }
    }

    /// Set sort direction to ascending
    pub fn ascending(mut self) -> Self {
        self.direction = OrderDirection::Ascending;
        self
    }

    /// Set sort direction to descending
    pub fn descending(mut self) -> Self {
        self.direction = OrderDirection::Descending;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_types_default() {
        let lt = LineTypes::default();
        assert!(lt.code);
        assert!(lt.tests);
        assert!(!lt.examples); // Not in default
        assert!(lt.docs);
        assert!(!lt.comments); // Not in default
        assert!(!lt.blanks); // Not in default
        assert!(lt.total);
    }

    #[test]
    fn test_line_types_none() {
        let lt = LineTypes::none();
        assert!(!lt.code);
        assert!(!lt.tests);
        assert!(!lt.examples);
        assert!(!lt.docs);
        assert!(!lt.comments);
        assert!(!lt.blanks);
        assert!(!lt.total);
    }

    #[test]
    fn test_line_types_everything() {
        let lt = LineTypes::everything();
        assert!(lt.code);
        assert!(lt.tests);
        assert!(lt.examples);
        assert!(lt.docs);
        assert!(lt.comments);
        assert!(lt.blanks);
        assert!(lt.total);
    }

    #[test]
    fn test_line_types_builder() {
        let lt = LineTypes::new().with_code().with_tests();
        assert!(lt.code);
        assert!(lt.tests);
        assert!(!lt.examples);
        assert!(!lt.docs);
        assert!(!lt.comments);
        assert!(!lt.blanks);
        assert!(lt.total); // All is on by default with new()
    }

    #[test]
    fn test_line_types_code_only() {
        let lt = LineTypes::code_only();
        assert!(lt.code);
        assert!(!lt.tests);
        assert!(!lt.examples);
        assert!(!lt.docs);
        assert!(!lt.comments);
        assert!(!lt.blanks);
        assert!(lt.total); // All is on by default
    }

    #[test]
    fn test_line_types_logic_only() {
        let lt = LineTypes::logic_only();
        assert!(lt.code);
        assert!(lt.tests);
        assert!(lt.examples);
        assert!(!lt.docs);
        assert!(!lt.comments);
        assert!(!lt.blanks);
        assert!(lt.total);
    }

    #[test]
    fn test_ordering_default() {
        let ordering = Ordering::default();
        assert_eq!(ordering.by, OrderBy::Label);
        assert_eq!(ordering.direction, OrderDirection::Ascending);
    }

    #[test]
    fn test_ordering_by_code() {
        let ordering = Ordering::by_code();
        assert_eq!(ordering.by, OrderBy::Code);
        assert_eq!(ordering.direction, OrderDirection::Descending);
    }

    #[test]
    fn test_ordering_direction_builder() {
        let ordering = Ordering::by_total().ascending();
        assert_eq!(ordering.by, OrderBy::Total);
        assert_eq!(ordering.direction, OrderDirection::Ascending);
    }

    #[test]
    fn test_order_by_from_str() {
        assert_eq!(OrderBy::from_str("code").unwrap(), OrderBy::Code);
        assert_eq!(OrderBy::from_str("tests").unwrap(), OrderBy::Tests);
        assert_eq!(OrderBy::from_str("total").unwrap(), OrderBy::Total);
        assert_eq!(OrderBy::from_str("label").unwrap(), OrderBy::Label);
        assert_eq!(OrderBy::from_str("docs").unwrap(), OrderBy::Docs);
        assert_eq!(OrderBy::from_str("comments").unwrap(), OrderBy::Comments);
        assert_eq!(OrderBy::from_str("blanks").unwrap(), OrderBy::Blanks);
        assert!(OrderBy::from_str("invalid").is_err());
    }
}
