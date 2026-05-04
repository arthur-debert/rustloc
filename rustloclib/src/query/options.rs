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

/// Numeric category that a filter `Predicate` operates on.
///
/// The seven variants correspond one-to-one with the seven counted line
/// types. `Total` is the sum of currently-enabled line types — it follows
/// the same semantics as `OrderBy::Total`, so a filter on `Total` honors
/// the active `LineTypes` rather than the precomputed `Locs::total` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Field {
    Code,
    Tests,
    Examples,
    Docs,
    Comments,
    Blanks,
    Total,
}

impl Field {
    /// Lower-case name used in CLI flag construction (e.g. `code-gte`).
    pub fn name(&self) -> &'static str {
        match self {
            Field::Code => "code",
            Field::Tests => "tests",
            Field::Examples => "examples",
            Field::Docs => "docs",
            Field::Comments => "comments",
            Field::Blanks => "blanks",
            Field::Total => "total",
        }
    }

    /// All seven variants in canonical order. Iteration order is the order
    /// the CLI generates flags, so it should be stable and predictable.
    pub fn all() -> &'static [Field] {
        &[
            Field::Code,
            Field::Tests,
            Field::Examples,
            Field::Docs,
            Field::Comments,
            Field::Blanks,
            Field::Total,
        ]
    }
}

/// Comparison operator for a filter `Predicate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    Gt,
    Gte,
    Eq,
    Ne,
    Lt,
    Lte,
}

impl Op {
    /// Lower-case name used in CLI flag construction (e.g. `code-gte`).
    pub fn name(&self) -> &'static str {
        match self {
            Op::Gt => "gt",
            Op::Gte => "gte",
            Op::Eq => "eq",
            Op::Ne => "ne",
            Op::Lt => "lt",
            Op::Lte => "lte",
        }
    }

    /// All six variants in canonical order.
    pub fn all() -> &'static [Op] {
        &[Op::Gt, Op::Gte, Op::Eq, Op::Ne, Op::Lt, Op::Lte]
    }

    /// Apply the operator to a pair of integer values.
    ///
    /// Inputs are `i64` so this works uniformly for raw line counts (always
    /// non-negative) and diff net values (which can be negative).
    pub fn evaluate(&self, lhs: i64, rhs: i64) -> bool {
        match self {
            Op::Gt => lhs > rhs,
            Op::Gte => lhs >= rhs,
            Op::Eq => lhs == rhs,
            Op::Ne => lhs != rhs,
            Op::Lt => lhs < rhs,
            Op::Lte => lhs <= rhs,
        }
    }
}

/// A single filter predicate of the form `<field> <op> <value>`.
///
/// Multiple predicates are combined with logical AND when applied via
/// `CountQuerySet::filter` / `DiffQuerySet::filter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Predicate {
    pub field: Field,
    pub op: Op,
    pub value: u64,
}

impl Predicate {
    pub fn new(field: Field, op: Op, value: u64) -> Self {
        Self { field, op, value }
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
    fn test_op_evaluate_all_variants() {
        assert!(Op::Gt.evaluate(10, 5));
        assert!(!Op::Gt.evaluate(5, 5));
        assert!(Op::Gte.evaluate(5, 5));
        assert!(Op::Gte.evaluate(10, 5));
        assert!(!Op::Gte.evaluate(4, 5));
        assert!(Op::Eq.evaluate(5, 5));
        assert!(!Op::Eq.evaluate(5, 6));
        assert!(Op::Ne.evaluate(5, 6));
        assert!(!Op::Ne.evaluate(5, 5));
        assert!(Op::Lt.evaluate(4, 5));
        assert!(!Op::Lt.evaluate(5, 5));
        assert!(Op::Lte.evaluate(5, 5));
        assert!(Op::Lte.evaluate(4, 5));
        assert!(!Op::Lte.evaluate(6, 5));
    }

    #[test]
    fn test_op_evaluate_handles_negative_diffs() {
        // Diff net values can be negative (lines removed > added).
        assert!(Op::Lt.evaluate(-100, 0));
        assert!(Op::Gte.evaluate(-50, -100));
    }

    #[test]
    fn test_field_all_and_op_all_lengths() {
        assert_eq!(Field::all().len(), 7);
        assert_eq!(Op::all().len(), 6);
        // 7 × 6 = 42 — the size of the synthetic CLI flag grid.
    }

    #[test]
    fn test_predicate_construction() {
        let p = Predicate::new(Field::Code, Op::Gte, 100);
        assert_eq!(p.field, Field::Code);
        assert_eq!(p.op, Op::Gte);
        assert_eq!(p.value, 100);
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
