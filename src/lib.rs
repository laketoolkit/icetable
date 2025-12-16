//! icetable - CLI for Apache Iceberg table management
//!
//! This library provides a unified interface for inspecting, validating,
//! converting, and managing Apache Iceberg tables across storage systems
//! (local, S3, GCS, Azure).
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use icetable::{create_object_store, ObjectStoreExt, Result};
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
//!
//! # Extending with Custom Formats
//!
//! ```rust,ignore
//! use icetable::{FormatHandler, FormatHandlerRegistry};
//!
//! // Register a custom format handler
//! FormatHandlerRegistry::global().register("xml", 75, |path, storage| {
//!     Ok(Box::new(XmlHandler::new(path, storage)?))
//! });
//! ```
//!
//! # Custom Transformations
//!
//! ```rust,ignore
//! use icetable::transform::{TransformPipeline, FilterStep, CustomTransformStep};
//!
//! let pipeline = TransformPipeline::new()
//!     .add_step(FilterStep::new("age > 18"))
//!     .add_step(CustomTransformStep::new("deduplicate", |batch| {
//!         // Your custom logic here
//!         Ok(batch)
//!     }));
//!
//! let transformed = pipeline.apply(batch)?;
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

// Internal implementation modules
// CLI module is hidden from docs - it's only for the icetable binary, not for library users
#[doc(hidden)]
pub mod cli;

pub mod core;
pub mod error;

// Utils module is hidden from docs - internal CLI utilities
#[doc(hidden)]
pub mod utils;

// =============================================================================
// Public API - Direct exports from root
// =============================================================================

// Error handling
pub use error::{Error, Result, ResultExt};

// Configuration
pub use core::config;

// Storage
pub use core::storage::{
    ObjectMeta, ObjectStoreExt, Storage, create_object_store, detect_storage_type,
};

// Format handling
pub use core::formats::{
    ColumnStats, FileMetadata, FormatHandler, FormatHandlerFactory, FormatHandlerRegistry,
    ReadOptions, ReadOptionsBuilder, ValidationReport, WriteOptions, WriteOptionsBuilder,
};

// Transformations
pub mod transform {
    //! Data transformation pipeline
    //!
    //! Provides composable transformations for Arrow RecordBatches.
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
