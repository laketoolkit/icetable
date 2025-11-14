//! Format handlers for different table formats
//!
//! This module contains implementations of the FormatHandler trait for various
//! table formats including Parquet, Arrow IPC, Delta Lake, and Iceberg.

pub mod traits;

// Format implementations - these will be implemented by Rust-Developer
pub mod arrow;
pub mod parquet;

#[cfg(feature = "delta")]
pub mod delta;

#[cfg(feature = "iceberg")]
pub mod iceberg;

// Re-export core types
pub use traits::{
    ColumnStats, FileMetadata, FormatHandler, FormatHandlerFactory, ReadOptions, ValidationReport,
    WriteOptions,
};

// Re-export format handlers
pub use arrow::ArrowHandler;
pub use parquet::ParquetHandler;

#[cfg(feature = "delta")]
pub use delta::DeltaHandler;

#[cfg(feature = "iceberg")]
pub use iceberg::IcebergHandler;
