//! Metadata management module
//!
//! Provides abstractions for reading, writing, and validating Iceberg table metadata.
//!
//! # Architecture
//!
//! ```text
//! MetadataReader (trait)
//! ├── StaticMetadataReader  - reads from storage by scanning metadata/
//! └── CatalogMetadataReader - reads via catalog API
//!
//! SnapshotWriter
//! └── Writes manifests, manifest lists, builds snapshots
//!     (Does NOT commit - use TableCommitter for that)
//! ```

mod traits;

// New modular architecture
mod reader;
mod writer;

// Iceberg-specific modules
mod iceberg;
mod iceberg_conflict;
mod iceberg_operations;
pub mod iceberg_partition;
mod iceberg_validator;
mod refs;
mod refs_scanner;

// Export types from traits (but not the reader::MetadataReader to avoid collision)
pub use traits::{
    DataFileChanges, DataFileInfo, MaintenanceResult, MetadataServiceReader, MetadataServiceWriter,
    OperationType, SnapshotInfo, TableServiceReader, TableServiceWriter,
};

// Primary exports
#[cfg(feature = "rest-catalog")]
pub use reader::CatalogMetadataReader;
pub use reader::{MetadataLoadResult, MetadataReader, StaticMetadataReader};
pub use writer::{PreparedSnapshot, SnapshotWriter};
// DataFileInfo is exported via `pub use traits::*` above

pub use iceberg::IcebergMetadataService;
pub use iceberg_conflict::{ConflictCheckResult, ConflictDetector, check_and_fail_on_conflict};
pub use iceberg_validator::{ValidationResult, validate_metadata, validate_or_error};
pub use refs::RefInfo;
