//! Query operations using DataFusion SQL engine
//!
//! This module provides SQL query capabilities similar to DuckDB, where file paths
//! are referenced directly in SQL queries:
//!
//! # Example
//!
//! ```ignore
//! use tabletools::core::operations::query::QueryOperation;
//!
//! let operation = QueryOperation::new();
//! let result = operation.execute(
//!     "SELECT * FROM 'data/flights.parquet' WHERE year > 2020",
//!     Some(100)
//! ).await?;
//! ```

pub mod operation;
pub mod sql_parser;
// TODO: streaming_table module needs to be updated for DataFusion 50.3.0 APIs
// pub mod streaming_table;
pub mod table_registry;

// Re-export main types
pub use operation::{QueryOperation, QueryResult};
pub use sql_parser::{FileReference, SqlPathExtractor};
// TODO: Re-enable after updating streaming_table for DataFusion 50.3.0
// pub use streaming_table::{StreamingFormat, StreamingTableProvider};
pub use table_registry::QueryTableRegistry;
