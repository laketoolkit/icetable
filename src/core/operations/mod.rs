//! Core operations for table manipulation
//!
//! This module contains the business logic for operations like inspect, validate,
//! diff, convert, and statistics. These operations use the FormatHandler and
//! StorageBackend abstractions to work with any supported format and storage.

pub mod inspect;
pub mod validate;
pub mod diff;
pub mod convert;
pub mod stats;
pub mod query;

// Re-export operation types
pub use inspect::InspectOperation;
pub use validate::ValidateOperation;
pub use diff::DiffOperation;
pub use convert::ConvertOperation;
pub use stats::StatsOperation;
pub use query::QueryOperation;
