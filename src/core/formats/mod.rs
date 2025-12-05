//! Format handlers for table formats (Delta Lake, Iceberg)
//!
//! This module contains implementations of the FormatHandler trait for
//! table formats: Delta Lake and Apache Iceberg.

// Utilities for table formats
pub mod table_utils;

pub mod registry;
pub mod traits;

#[cfg(feature = "delta")]
pub mod delta;

#[cfg(feature = "delta")]
pub mod delta_storage_options;

pub mod iceberg;

// Re-export core types
pub use registry::FormatHandlerRegistry;
pub use traits::{
    ColumnStats, FileMetadata, FormatHandler, FormatHandlerFactory, ReadOptions,
    ReadOptionsBuilder, TimeTravelOptions, ValidationReport, WriteOptions, WriteOptionsBuilder,
};

#[cfg(feature = "delta")]
pub use delta::DeltaHandler;

pub use iceberg::IcebergHandler;
