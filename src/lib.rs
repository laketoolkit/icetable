//! TableTools - Universal CLI for tabular data
//!
//! This library provides a unified interface for inspecting, validating,
//! converting, and managing tabular data files across different formats
//! (Parquet, Arrow, Iceberg, Delta Lake) and storage systems (local, S3, GCS, Azure).
//!
//! # Architecture
//!
//! The library is organized into several layers:
//!
//! - **Core**: Format handlers, storage backends, and operations
//! - **CLI**: Command-line interface and output formatting
//! - **Utils**: Caching, progress tracking, and telemetry
//!
//! # Usage
//!
//! ```rust,no_run
//! use tabletools::core::{FormatHandlerFactory, StorageBackendFactory};
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> tabletools::error::Result<()> {
//!     // Create storage backend
//!     let storage = StorageBackendFactory::create_backend("s3://bucket/data.parquet").await?;
//!
//!     // Create format handler
//!     let handler = FormatHandlerFactory::create_handler(
//!         Path::new("s3://bucket/data.parquet"),
//!         storage
//!     ).await?;
//!
//!     // Read schema
//!     let schema = handler.read_schema().await?;
//!     println!("Schema: {:?}", schema);
//!
//!     Ok(())
//! }
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod error;
pub mod core;
pub mod cli;
pub mod utils;

// Re-export commonly used types
pub use error::{Error, Result};
pub use core::{FormatHandler, FormatHandlerFactory, StorageBackend, StorageBackendFactory};
