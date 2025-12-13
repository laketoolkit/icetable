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
//! use icetable::v1::storage::{create_object_store, ObjectStoreExt};
//! use icetable::v1::Result;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Create object store from URL or path
//!     let store = create_object_store("/path/to/table").await?;
//!
//!     // Use extension methods for convenient access
//!     let exists = store.exists_str("metadata/v1.metadata.json").await?;
//!     println!("Table exists: {}", exists);
//!
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
    //! - [`Storage`] - Type alias for `Arc<dyn ObjectStore>`
    //! - [`ObjectStoreExt`] - Extension trait with convenience methods
    //! - [`ObjectMeta`] - Metadata about stored objects
    //! - [`create_object_store`] - Create storage from URL or path

    pub use crate::core::storage::{
        ObjectMeta, ObjectStoreExt, Storage, create_object_store, detect_storage_type,
    };
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
