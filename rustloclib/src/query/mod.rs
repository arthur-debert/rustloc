//! Query processing: filter, aggregate, and sort data.
//!
//! This module handles the third stage of the pipeline - transforming raw
//! counting results into a query-ready format. It provides:
//!
//! - **Options**: Configuration for filtering and sorting (`LineTypes`, `Ordering`)
//! - **QuerySet**: Processed data ready for presentation
//!
//! ## Example
//!
//! ```rust,ignore
//! use rustloclib::query::{CountQuerySet, LineTypes, Ordering, Aggregation};
//!
//! let queryset = CountQuerySet::from_result(
//!     &result,
//!     Aggregation::ByCrate,
//!     LineTypes::all(),
//!     Ordering::by_code(),
//! );
//! ```

pub mod options;
pub mod queryset;

pub use options::{Aggregation, LineTypes, OrderBy, OrderDirection, Ordering};
pub use queryset::{CountQuerySet, DiffQuerySet, QueryItem};
