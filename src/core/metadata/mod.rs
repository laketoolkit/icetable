//! Metadata management module
//!
//! Provides abstractions for transactional metadata operations on table formats.
//! This module encapsulates the repetitive logic of writing snapshots, manifests,
//! and updating table metadata for both Delta Lake and Iceberg tables.

mod traits;

#[cfg(feature = "iceberg")]
mod iceberg;

#[cfg(feature = "delta")]
mod delta;

pub use traits::*;

#[cfg(feature = "iceberg")]
pub use iceberg::IcebergMetadataService;

#[cfg(feature = "delta")]
pub use delta::DeltaMetadataService;
