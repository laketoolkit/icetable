//! Metadata management module
//!
//! Provides abstractions for transactional metadata operations on Iceberg tables.
//! This module encapsulates the repetitive logic of writing snapshots, manifests,
//! and updating table metadata.

mod traits;

mod iceberg;
mod iceberg_conflict;
mod iceberg_operations;
mod iceberg_partition;
mod iceberg_validator;
mod iceberg_writer;

pub use traits::*;

pub use iceberg::{IcebergMetadataService, RefInfo};
pub use iceberg_conflict::{check_and_fail_on_conflict, ConflictCheckResult, ConflictDetector};
pub use iceberg_validator::{validate_metadata, validate_or_error, ValidationResult};
pub use iceberg_writer::IcebergSnapshotWriter;
