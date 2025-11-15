//! Format handlers for different table formats
//!
//! This module contains implementations of the FormatHandler trait for various
//! table formats including Parquet, Arrow IPC, CSV, JSON, Delta Lake, and Iceberg.

pub mod registry;
pub mod traits;

// Format implementations
pub mod arrow;
pub mod csv;
pub mod json;
pub mod parquet;

#[cfg(feature = "delta")]
pub mod delta;

#[cfg(feature = "iceberg")]
pub mod iceberg;

// Re-export core types
pub use registry::FormatHandlerRegistry;
pub use traits::{
    ColumnStats, FileMetadata, FormatHandler, FormatHandlerFactory, ReadOptions,
    ReadOptionsBuilder, ValidationReport, WriteOptions, WriteOptionsBuilder,
};

// Re-export format handlers
pub use arrow::ArrowHandler;
pub use csv::CsvHandler;
pub use json::JsonHandler;
pub use self::parquet::ParquetHandler;

#[cfg(feature = "delta")]
pub use delta::DeltaHandler;

#[cfg(feature = "iceberg")]
pub use iceberg::IcebergHandler;
