//! Core operations for table manipulation
//!
//! This module contains the business logic for operations like inspect, validate,
//! diff, convert, and statistics. These operations use the FormatHandler and
//! StorageBackend abstractions to work with any supported format and storage.

pub mod convert;
pub mod diff;
pub mod inspect;
pub mod transform;
pub mod validate;

// Re-export operation types
pub use convert::ConvertOperation;
pub use diff::DiffOperation;
pub use inspect::InspectOperation;
pub use transform::{TransformConfig, apply_transforms};
pub use validate::ValidateOperation;
