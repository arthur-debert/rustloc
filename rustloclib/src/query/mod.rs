//! Query processing: filter, aggregate, and sort data.
//!
//! This module handles the third stage of the pipeline - transforming raw
//! counting results into a query-ready format. It provides:
//!
//! - **Options**: Configuration for aggregation, line-type selection, and
//!   sorting ([`Aggregation`], [`LineTypes`], [`Ordering`]).
//! - **Predicates**: Threshold filters built from a [`Field`] + [`Op`] pair
//!   ([`Predicate`]). Operators are `gt`/`gte`/`eq`/`ne`/`lt`/`lte`.
//! - **QuerySet**: Processed data ready for presentation, with chainable
//!   `.filter(&[Predicate])` and `.top(N)` methods on both [`CountQuerySet`]
//!   and [`DiffQuerySet`]. The two are independent and order matters —
//!   `.filter(...).top(N)` gives "top N of those matching the predicates"
//!   (what the CLI does); `.top(N).filter(...)` instead truncates first
//!   and then filters from that slice.
//!
//! Diff predicates are evaluated against the net change per row
//! (added − removed), so e.g. `Op::Lt` against `0` matches rows with more
//! lines removed than added.
//!
//! ## Example
//!
//! ```rust,ignore
//! use rustloclib::query::{
//!     Aggregation, CountQuerySet, Field, LineTypes, Op, Ordering, Predicate,
//! };
//!
//! let queryset = CountQuerySet::from_result(
//!     &result,
//!     Aggregation::ByFile,
//!     LineTypes::everything(),
//!     Ordering::by_code(),
//! )
//! .filter(&[Predicate::new(Field::Code, Op::Gte, 1000)])
//! .top(10);
//! ```

pub mod options;
pub mod queryset;

pub use options::{
    Aggregation, Field, LineTypes, Op, OrderBy, OrderDirection, Ordering, Predicate,
};
pub use queryset::{CountQuerySet, DiffQuerySet, QueryItem};
