//! Core traits for storage backends
//!
//! This module defines the [`StorageBackend`] trait, which provides a unified
//! interface for accessing different storage systems including local filesystem,
//! Amazon S3, Google Cloud Storage, and Azure Blob Storage.
//!
//! # Architecture
//!
//! The storage abstraction allows icetable to work with tables stored anywhere:
//!
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │              StorageBackend trait               │
//! └─────────────────────────────────────────────────┘
//!                        ▲
//!        ┌───────────────┼───────────────┐
//!        │               │               │
//! ┌──────┴─────┐  ┌──────┴─────┐  ┌──────┴─────┐
//! │ LocalBackend│  │  S3Backend │  │ GcsBackend │
//! └────────────┘  └────────────┘  └────────────┘
//! ```
//!
//! # Implementing a New Backend
//!
//! To add support for a new storage system:
//!
//! 1. Implement the [`StorageBackend`] trait
//! 2. Handle authentication in your constructor
//! 3. Map storage-specific errors to [`crate::error::Error`]
//! 4. Register in [`crate::core::storage::StorageBackendFactory`]
//!
//! # Example
//!
//! ```ignore
//! use icetable::core::storage::{StorageBackend, StorageBackendFactory};
//!
//! // Create backend from URL (auto-detects type)
//! let backend = StorageBackendFactory::create_backend("s3://my-bucket/tables/events").await?;
//!
//! // Check if file exists
//! if backend.exists("metadata/v1.metadata.json").await? {
//!     let data = backend.get("metadata/v1.metadata.json", &GetOptions::default()).await?;
//! }
//!
//! // List files with prefix
//! let files = backend.list(&ListOptions {
//!     prefix: Some("data/".to_string()),
//!     ..Default::default()
//! }).await?;
//! ```

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};

use crate::error::Result;

/// Metadata about a stored object
#[derive(Debug, Clone)]
pub struct ObjectMetadata {
    /// Full path/key of the object
    pub path: String,

    /// Size in bytes
    pub size: u64,

    /// Last modified timestamp
    pub last_modified: DateTime<Utc>,

    /// ETag or version identifier
    pub e_tag: Option<String>,

    /// Content type
    pub content_type: Option<String>,
}

/// Options for listing objects
#[derive(Debug, Clone, Default)]
pub struct ListOptions {
    /// Prefix to filter by
    pub prefix: Option<String>,

    /// Delimiter for hierarchical listing
    pub delimiter: Option<String>,

    /// Maximum number of results
    pub max_results: Option<usize>,

    /// Continuation token for pagination
    pub continuation_token: Option<String>,
}

/// Result of a list operation
#[derive(Debug, Clone)]
pub struct ListResult {
    /// Objects found
    pub objects: Vec<ObjectMetadata>,

    /// Common prefixes (directories)
    pub prefixes: Vec<String>,

    /// Continuation token for next page
    pub continuation_token: Option<String>,
}

/// Options for reading objects
#[derive(Debug, Clone, Default)]
pub struct GetOptions {
    /// Byte range to read (start, end)
    pub range: Option<(u64, u64)>,

    /// If-None-Match precondition (ETag)
    pub if_none_match: Option<String>,

    /// If-Modified-Since precondition
    pub if_modified_since: Option<DateTime<Utc>>,
}

/// Options for writing objects
#[derive(Debug, Clone, Default)]
pub struct PutOptions {
    /// Content type
    pub content_type: Option<String>,

    /// Additional metadata
    pub metadata: std::collections::HashMap<String, String>,

    /// If-None-Match precondition (for conditional writes)
    pub if_none_match: Option<String>,
}

/// The core trait for storage backend abstraction
///
/// This trait provides a unified interface for accessing different storage
/// systems. All operations are async to support efficient I/O.
///
/// Implementations should handle authentication, retries, and error mapping
/// to TableTools error types.
#[async_trait]
pub trait StorageBackend: Send + Sync {
    /// Get the storage type (e.g., "local", "s3", "gcs", "azure")
    fn storage_type(&self) -> &str;

    /// Check if an object exists
    async fn exists(&self, path: &str) -> Result<bool>;

    /// Get metadata for an object without downloading it
    async fn head(&self, path: &str) -> Result<ObjectMetadata>;

    /// Read an object's contents
    async fn get(&self, path: &str, options: &GetOptions) -> Result<Bytes>;

    /// Read a range of bytes from an object
    ///
    /// This is more efficient than reading the entire object when only
    /// a portion is needed (e.g., reading Parquet file footer).
    async fn get_range(&self, path: &str, start: u64, end: u64) -> Result<Bytes> {
        let options = GetOptions {
            range: Some((start, end)),
            ..Default::default()
        };
        self.get(path, &options).await
    }

    /// Write data to an object
    async fn put(&self, path: &str, data: Bytes, options: &PutOptions) -> Result<()>;

    /// List objects with a given prefix
    async fn list(&self, options: &ListOptions) -> Result<ListResult>;

    /// Delete an object
    async fn delete(&self, path: &str) -> Result<()>;

    /// Copy an object within the same storage backend
    async fn copy(&self, from: &str, to: &str) -> Result<()>;

    /// Check if this backend supports multipart uploads
    fn supports_multipart(&self) -> bool {
        false
    }

    /// Check if this backend supports atomic operations
    fn supports_atomic_operations(&self) -> bool {
        false
    }
}

/// Parse a path/URL to determine the storage backend type
pub fn parse_storage_url(path: &str) -> Result<(String, String)> {
    if let Some(url) = path.strip_prefix("s3://") {
        let parts: Vec<&str> = url.splitn(2, '/').collect();
        if parts.len() == 2 {
            Ok(("s3".to_string(), path.to_string()))
        } else {
            Err(crate::error::Error::Configuration {
                message: format!("Invalid S3 URL: {}", path),
            })
        }
    } else if let Some(_url) = path.strip_prefix("gs://") {
        Ok(("gcs".to_string(), path.to_string()))
    } else if path.strip_prefix("az://").is_some() || path.strip_prefix("azure://").is_some() {
        Ok(("azure".to_string(), path.to_string()))
    } else if let Some(url) = path.strip_prefix("file://") {
        Ok(("local".to_string(), url.to_string()))
    } else {
        // Assume local filesystem if no scheme
        Ok(("local".to_string(), path.to_string()))
    }
}

/// Factory for creating storage backends
pub struct StorageBackendFactory;

impl StorageBackendFactory {
    /// Create a storage backend based on the path/URL
    pub async fn create_backend(path: &str) -> Result<std::sync::Arc<dyn StorageBackend>> {
        let (storage_type, _) = parse_storage_url(path)?;

        match storage_type.as_str() {
            "local" => {
                let backend = crate::core::storage::LocalBackend::new()?;
                Ok(std::sync::Arc::new(backend))
            }
            "s3" => {
                let backend = crate::core::storage::S3Backend::new(path).await?;
                Ok(std::sync::Arc::new(backend))
            }
            "gcs" => {
                let backend = crate::core::storage::GcsBackend::new(path).await?;
                Ok(std::sync::Arc::new(backend))
            }
            "azure" => {
                let backend = crate::core::storage::AzureBackend::new(path).await?;
                Ok(std::sync::Arc::new(backend))
            }
            _ => Err(crate::error::Error::Configuration {
                message: format!("Unknown storage type: {}", storage_type),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_s3_url() {
        let (storage_type, _) = parse_storage_url("s3://bucket/key").unwrap();
        assert_eq!(storage_type, "s3");
    }

    #[test]
    fn test_parse_gcs_url() {
        let (storage_type, _) = parse_storage_url("gs://bucket/key").unwrap();
        assert_eq!(storage_type, "gcs");
    }

    #[test]
    fn test_parse_local_path() {
        let (storage_type, _) = parse_storage_url("/tmp/file.parquet").unwrap();
        assert_eq!(storage_type, "local");
    }

    #[test]
    fn test_invalid_s3_url() {
        let result = parse_storage_url("s3://bucket-only");
        assert!(result.is_err());
    }
}
