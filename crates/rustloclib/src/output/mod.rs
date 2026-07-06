//! Output formatting: present data as tables.
//!
//! This module handles the fourth and final stage of the pipeline -
//! formatting query results for display. It provides:
//!
//! - **LOCTable**: Table-ready data structure with headers, rows, and footer
//! - **TableRow**: Individual row with label and formatted values
//!
//! LOCTable is a pure presentation layer - it only formats data into strings.
//! All filtering, aggregation, and sorting happens in the query stage.
//!
//! ## Example
//!
//! ```rust,ignore
//! use rustloclib::output::LOCTable;
//!
//! let table = LOCTable::from_count_queryset(&queryset);
//! // table.headers: ["Crate", "Code", "Tests", ..., "Total"]
//! // table.rows: [TableRow { label: "my-crate", values: ["100", "50", ...] }]
//! // table.footer: TableRow { label: "Total (5 files)", ... }
//! ```

pub mod table;

pub use table::{LOCTable, TableRow};
