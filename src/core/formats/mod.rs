//! Format handlers for Apache Iceberg tables
//!
//! This module contains the FormatHandler trait implementation for Apache Iceberg.
//!

// Utilities for table formats
pub mod table_utils;

pub mod registry;
pub mod traits;

pub mod iceberg;

// Re-export core types
pub use registry::FormatHandlerRegistry;
pub use traits::{
    ColumnStats, FileMetadata, FormatHandler, FormatHandlerFactory, ReadOptions,
    ReadOptionsBuilder, TimeTravelOptions, ValidationReport, WriteOptions, WriteOptionsBuilder,
};

pub use iceberg::IcebergHandler;
