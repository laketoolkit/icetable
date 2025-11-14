//! Storage backend implementations
//!
//! This module contains implementations of the StorageBackend trait for various
//! storage systems including local filesystem, S3, GCS, and Azure Blob Storage.

pub mod traits;

// Storage implementations - these will be implemented by Rust-Developer
pub mod azure;
pub mod gcs;
pub mod local;
pub mod s3;

// Re-export core types
pub use traits::{
    GetOptions, ListOptions, ListResult, ObjectMetadata, PutOptions, StorageBackend,
    StorageBackendFactory,
};

// Re-export storage backends
pub use azure::AzureBackend;
pub use gcs::GcsBackend;
pub use local::LocalBackend;
pub use s3::S3Backend;
