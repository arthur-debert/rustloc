//! Input options for LOC counting and diffing operations.
//!
//! This module contains all configuration types that control what data
//! the library computes and returns.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

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
        assert!(OrderBy::from_str("invalid").is_err());
    }
}
