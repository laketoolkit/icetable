//! Metadata management module
//!
//! Provides abstractions for transactional metadata operations on Iceberg tables.
//! This module encapsulates the repetitive logic of writing snapshots, manifests,
//! and updating table metadata.

mod traits;

#[cfg(feature = "iceberg")]
mod iceberg;

pub use traits::*;

#[cfg(feature = "iceberg")]
pub use iceberg::IcebergMetadataService;
