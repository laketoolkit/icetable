//! Stable Public API (v1.x)
//!
//! This module contains the stable public API for icetable that follows
//! semantic versioning guarantees. Items in this module are considered stable
//! and breaking changes will only occur in major version bumps.
//!
//! # Stability Contract
//!
//! - All types and traits in `v1::*` follow semantic versioning
//! - New methods may be added with default implementations
//! - Deprecated items will be kept for at least one minor version
//! - Internal modules (`core::*`, `cli::*`) may change without notice
//!
//! # Usage
//!
//! ```ignore
//! use icetable::v1::formats::{FormatHandlerRegistry, ReadOptions, WriteOptions};
//! use icetable::v1::storage::StorageBackendFactory;
//! use icetable::v1::Result;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Create storage backend
//!     let storage = StorageBackendFactory::create_backend("data.parquet").await?;
//!
//!     // Get format handler using registry
//!     let handler = FormatHandlerRegistry::global()
//!         .create_handler(Path::new("data.parquet"), storage)
//!         .await?;
//!
//!     // Read with options
//!     let options = ReadOptions::builder()
//!         .limit(100)
//!         .build();
//!
//!     let batches = handler.read_batches(&options).await?;
//!     Ok(())
//! }
//! ```

pub mod formats {
    //! Stable format handling API
    //!
    //! # Core Types
    //!
    //! - [`FormatHandler`] - Trait for implementing format handlers
    //! - [`FormatHandlerRegistry`] - Plugin-style registry for format handlers
    //! - [`ReadOptions`] - Configuration for reading data
    //! - [`WriteOptions`] - Configuration for writing data
    //! - [`FileMetadata`] - Metadata about table files
    //! - [`ValidationReport`] - Validation results

    pub use crate::core::formats::{
        ColumnStats, FileMetadata, FormatHandler, FormatHandlerRegistry, ReadOptions,
        ReadOptionsBuilder, ValidationReport, WriteOptions, WriteOptionsBuilder,
    };
}

pub mod storage {
    //! Stable storage backend API
    //!
    //! # Core Types
    //!
    //! - [`StorageBackend`] - Trait for implementing storage backends
    //! - [`StorageBackendFactory`] - Factory for creating storage backends
    //! - [`ObjectMetadata`] - Metadata about stored objects

    pub use crate::core::storage::{ObjectMetadata, StorageBackend, StorageBackendFactory};

    /// Options for storage operations (stable)
    pub use crate::core::storage::{GetOptions, ListOptions, PutOptions};
}

pub mod transform {
    //! Stable transformation pipeline API
    //!
    //! # Core Types
    //!
    //! - [`TransformPipeline`] - Composable transformation pipeline
    //! - [`TransformStep`] - Trait for implementing custom transformations
    //! - [`TransformConfig`] - Configuration for transformations
    //!
    //! # Built-in Steps
    //!
    //! - [`FilterStep`] - Filter rows based on expressions
    //! - [`ProjectStep`] - Select specific columns
    //! - [`RenameStep`] - Rename columns
    //! - [`CastStep`] - Cast column types
    //! - [`CustomTransformStep`] - Custom transformation using closures

    pub use crate::core::operations::transform::{
        CastStep, CustomTransformStep, FilterStep, ProjectStep, RenameStep, TransformConfig,
        TransformPipeline, TransformStep,
    };
}

/// Error types (stable)
pub use crate::error::{Error, Result};

pub mod utils {
    //! Stable utility functions
    //!
    //! # Type Parsing
    //!
    //! - [`parse_data_type`] - Parse Arrow DataType from string

    pub use crate::utils::parse_data_type;
}

// Internal modules (unstable, may change)
#[doc(hidden)]
pub mod __internal {
    //! Internal implementation details
    //!
    //! ⚠️ WARNING: These are internal implementation details that may change
    //! without notice. Do not depend on these directly.

    pub use crate::cli;
    pub use crate::core;
}
