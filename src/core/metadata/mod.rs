//! Metadata management module
//!
//! Provides abstractions for transactional metadata operations on Iceberg tables.
//! This module encapsulates the repetitive logic of writing snapshots, manifests,
//! and updating table metadata.

mod traits;

mod iceberg;
mod iceberg_operations;
mod iceberg_partition;
mod iceberg_writer;

pub use traits::*;

pub use iceberg::IcebergMetadataService;
pub use iceberg_writer::IcebergSnapshotWriter;
