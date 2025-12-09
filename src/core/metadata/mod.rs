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

pub use traits::*;

// Primary exports
pub use reader::{MetadataReader, MetadataLoadResult, StaticMetadataReader};
#[cfg(feature = "rest-catalog")]
pub use reader::CatalogMetadataReader;
pub use writer::{SnapshotWriter, PreparedSnapshot};
// DataFileInfo is exported via `pub use traits::*` above

pub use iceberg::IcebergMetadataService;
pub use refs::RefInfo;
pub use iceberg_conflict::{ConflictCheckResult, ConflictDetector, check_and_fail_on_conflict};
pub use iceberg_validator::{ValidationResult, validate_metadata, validate_or_error};
