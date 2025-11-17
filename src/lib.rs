//! TableTools - Universal CLI for tabular data
//!
//! This library provides a unified interface for inspecting, validating,
//! converting, and managing tabular data files across different formats
//! (Parquet, Arrow, CSV, JSON, Iceberg, Delta Lake) and storage systems (local, S3, GCS, Azure).
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
//! use tablectl::v1::formats::{FormatHandlerRegistry, ReadOptions};
//! use tablectl::v1::storage::StorageBackendFactory;
//! use tablectl::v1::Result;
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Create storage backend
//!     let storage = StorageBackendFactory::create_backend("data.parquet").await?;
//!
//!     // Get format handler using registry (supports plugin formats)
//!     let handler = FormatHandlerRegistry::global()
//!         .create_handler(Path::new("data.parquet"), storage)
//!         .await?;
//!
//!     // Read with options using builder pattern
//!     let options = ReadOptions::builder()
//!         .limit(100)
//!         .build();
//!
//!     let batches = handler.read_batches(&options).await?;
//!     println!("Read {} batches", batches.len());
//!
//!     Ok(())
//! }
//! ```
//!
//! # Extending with Custom Formats
//!
//! ```rust,ignore
//! use tablectl::v1::formats::{FormatHandler, FormatHandlerRegistry};
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
//! use tablectl::v1::transform::{TransformPipeline, FilterStep, CustomTransformStep};
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
pub mod core;
pub mod error;
pub mod utils;

// Stable public API (v1.x)
pub mod v1;

// Convenience re-exports for backward compatibility
// Note: Prefer using v1::* for stable API
pub use core::{FormatHandler, FormatHandlerFactory, StorageBackend, StorageBackendFactory};
pub use error::{Error, Result};
