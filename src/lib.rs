//! icetable - CLI for Apache Iceberg table management
//!
//! This library provides a unified interface for inspecting, validating,
//! converting, and managing Apache Iceberg tables across storage systems
//! (local, S3, GCS, Azure).
//!
//! # Stability and Versioning
//!
//! The public API is split into two tiers:
//!
//! - **`v1::*` modules** - Stable public API following semantic versioning
//! - **`core::*` modules** - Internal implementation, may change without notice
//!
//! For external usage, always prefer the `v1` module to ensure stability.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use icetable::{create_object_store, ObjectStoreExt};
//! use icetable::Result;
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
//! use icetable::v1::formats::{FormatHandler, FormatHandlerRegistry};
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
//! use icetable::v1::transform::{TransformPipeline, FilterStep, CustomTransformStep};
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

// Internal modules (implementation details)
pub mod cli;
pub mod config;
pub mod core;
pub mod error;
pub mod utils;

// Stable public API (v1.x)
pub mod v1;

// Convenience re-exports for backward compatibility
// Note: Prefer using v1::* for stable API
pub use core::{FormatHandler, FormatHandlerFactory, ObjectStoreExt, Storage, create_object_store};
pub use error::{Error, Result};
