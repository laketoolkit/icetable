//! Storage backends and object store utilities
//!
//! This module provides a unified interface for accessing storage systems
//! using Apache Arrow's [`object_store`] crate directly for maximum
//! compatibility with the Arrow/Parquet ecosystem.
//!
//! # Quick Start
//!
//! ```ignore
//! use icetable::core::storage::{create_object_store, ObjectStoreExt, to_path};
//!
//! // Create a store from a URL
//! let store = create_object_store("s3://my-bucket/tables").await?;
//!
//! // Use extension methods for convenience
//! let exists = store.exists(&to_path("metadata/v1.metadata.json")).await?;
//! let data = store.get_bytes(&to_path("data/file.parquet")).await?;
//! ```
//!
//! # Supported Storage Systems
//!
//! - Local filesystem: `/path/to/table` or `file:///path/to/table`
//! - Amazon S3: `s3://bucket/prefix`
//! - Google Cloud Storage: `gs://bucket/prefix`
//! - Azure Blob Storage: `az://container/prefix`

pub mod ext;
pub mod factory;

// Re-export API
pub use ext::{ObjectStoreExt, from_path, to_path};
pub use factory::{Storage, create_object_store, detect_storage_type, parse_storage_url};

// Re-export object_store types for convenience
pub use object_store::{
    GetOptions as ObjGetOptions, ObjectMeta, ObjectStore, PutOptions as ObjPutOptions, PutPayload,
    path::Path as StoragePath,
};
