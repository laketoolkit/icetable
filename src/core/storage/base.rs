//! Base generic storage backend implementation
//!
//! This module provides a generic `BaseStorageBackend<P>` that implements the
//! `StorageBackend` trait using `object_store` and a `CloudPathParser`.
//! This eliminates code duplication across S3, GCS, and Azure backends.

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use object_store::ObjectStore;
use std::sync::Arc;

use super::path_parser::CloudPathParser;
use super::traits::*;
use crate::error::{Error, Result};

/// Generic storage backend that works with any CloudPathParser
///
/// This struct wraps an `object_store` instance and uses a path parser
/// to handle cloud-specific path formats. It implements all StorageBackend
/// methods generically, eliminating the need for duplicated code across
/// different cloud providers.
///
/// # Type Parameters
/// * `P` - The CloudPathParser implementation (S3PathParser, GcsPathParser, etc.)
pub struct BaseStorageBackend<P: CloudPathParser> {
    /// The underlying object store (S3, GCS, Azure, etc.)
    store: Arc<dyn ObjectStore>,
    /// Path parser for this storage type
    parser: P,
    /// Storage type name for identification
    storage_type_name: &'static str,
}

impl<P: CloudPathParser> BaseStorageBackend<P> {
    /// Create a new base storage backend
    ///
    /// # Arguments
    /// * `store` - The object store implementation
    /// * `parser` - The path parser for this storage type
    /// * `storage_type_name` - Name of the storage type (e.g., "s3", "gcs", "azure")
    pub fn new(store: Arc<dyn ObjectStore>, parser: P, storage_type_name: &'static str) -> Self {
        Self {
            store,
            parser,
            storage_type_name,
        }
    }

    /// Parse a path using the configured parser
    fn parse_path(&self, path: &str) -> Result<(String, String)> {
        self.parser.parse(path)
    }

    /// Convert object_store error to our Error type with context
    fn map_store_error(&self, error: object_store::Error, path: &str) -> Error {
        match error {
            object_store::Error::NotFound { .. } => Error::FileNotFound {
                path: std::path::PathBuf::from(path),
            },
            e => Error::General(format!(
                "Failed to access {} object at {}: {}",
                self.storage_type_name, path, e
            )),
        }
    }
}

#[async_trait]
impl<P: CloudPathParser> StorageBackend for BaseStorageBackend<P> {
    fn storage_type(&self) -> &str {
        self.storage_type_name
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        let (_, key) = self.parse_path(path)?;
        let obj_path = object_store::path::Path::from(key);

        match self.store.head(&obj_path).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(Error::General(format!(
                "Failed to check {} object: {}",
                self.storage_type_name, e
            ))),
        }
    }

    async fn head(&self, path: &str) -> Result<ObjectMetadata> {
        log::debug!("{} HEAD: {}", self.storage_type_name.to_uppercase(), path);
        let (_, key) = self.parse_path(path)?;
        let obj_path = object_store::path::Path::from(key);

        let meta = self
            .store
            .head(&obj_path)
            .await
            .map_err(|e| self.map_store_error(e, path))?;

        Ok(ObjectMetadata {
            path: path.to_string(),
            size: meta.size as u64,
            last_modified: meta.last_modified,
            e_tag: meta.e_tag,
            content_type: None,
        })
    }

    async fn get(&self, path: &str, options: &GetOptions) -> Result<Bytes> {
        let (_, key) = self.parse_path(path)?;
        let obj_path = object_store::path::Path::from(key);

        if let Some((start, end)) = options.range {
            // Use range request
            log::debug!(
                "{} GET RANGE: {} (range {}-{})",
                self.storage_type_name.to_uppercase(),
                path,
                start,
                end
            );
            let range = start..end;
            self.store.get_range(&obj_path, range).await.map_err(|e| {
                Error::General(format!(
                    "Failed to get {} object range: {}",
                    self.storage_type_name, e
                ))
            })
        } else {
            // Get full object
            log::debug!(
                "{} GET FULL: {}",
                self.storage_type_name.to_uppercase(),
                path
            );
            let result = self
                .store
                .get(&obj_path)
                .await
                .map_err(|e| self.map_store_error(e, path))?;

            result.bytes().await.map_err(|e| {
                Error::General(format!(
                    "Failed to read {} object bytes: {}",
                    self.storage_type_name, e
                ))
            })
        }
    }

    async fn put(&self, path: &str, data: Bytes, _options: &PutOptions) -> Result<()> {
        let (_, key) = self.parse_path(path)?;
        let obj_path = object_store::path::Path::from(key);

        self.store.put(&obj_path, data.into()).await.map_err(|e| {
            Error::General(format!(
                "Failed to put {} object: {}",
                self.storage_type_name, e
            ))
        })?;

        Ok(())
    }

    async fn list(&self, options: &ListOptions) -> Result<ListResult> {
        let prefix = options.prefix.as_deref().unwrap_or("");

        // Parse prefix to extract key (if it's a full cloud path)
        let key = if prefix.starts_with(self.parser.scheme()) {
            let (_, k) = self.parse_path(prefix)?;
            k
        } else {
            prefix.to_string()
        };

        let obj_path = if key.is_empty() {
            None
        } else {
            Some(object_store::path::Path::from(key.as_str()))
        };

        let mut objects = Vec::new();
        let mut stream = self.store.list(obj_path.as_ref());

        while let Some(result) = stream.next().await {
            let meta = result.map_err(|e| {
                Error::General(format!(
                    "Failed to list {} objects: {}",
                    self.storage_type_name, e
                ))
            })?;

            // Reconstruct full cloud path
            let full_path = format!("{}{}", self.parser.scheme(), meta.location);

            objects.push(ObjectMetadata {
                path: full_path,
                size: meta.size as u64,
                last_modified: meta.last_modified,
                e_tag: meta.e_tag,
                content_type: None,
            });

            // Respect max_results limit
            if let Some(max) = options.max_results {
                if objects.len() >= max {
                    break;
                }
            }
        }

        Ok(ListResult {
            objects,
            prefixes: Vec::new(),
            continuation_token: None,
        })
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let (_, key) = self.parse_path(path)?;
        let obj_path = object_store::path::Path::from(key);

        self.store.delete(&obj_path).await.map_err(|e| {
            Error::General(format!(
                "Failed to delete {} object: {}",
                self.storage_type_name, e
            ))
        })?;

        Ok(())
    }

    async fn copy(&self, from: &str, to: &str) -> Result<()> {
        let (_, from_key) = self.parse_path(from)?;
        let (_, to_key) = self.parse_path(to)?;

        let from_path = object_store::path::Path::from(from_key);
        let to_path = object_store::path::Path::from(to_key);

        self.store.copy(&from_path, &to_path).await.map_err(|e| {
            Error::General(format!(
                "Failed to copy {} object: {}",
                self.storage_type_name, e
            ))
        })?;

        Ok(())
    }

    fn supports_multipart(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::storage::path_parser::S3PathParser;

    // Mock tests to verify the structure compiles
    // Real integration tests would require actual cloud credentials

    #[test]
    fn test_base_backend_creation() {
        // This test just verifies that the generic structure works
        // We can't create a real backend without credentials, but we can
        // verify the type system works
        use object_store::memory::InMemory;

        let store = Arc::new(InMemory::new());
        let parser = S3PathParser;
        let _backend = BaseStorageBackend::new(store, parser, "s3");
        // If this compiles, the generic structure is correct
    }

    #[test]
    fn test_storage_type() {
        use object_store::memory::InMemory;

        let store = Arc::new(InMemory::new());
        let parser = S3PathParser;
        let backend = BaseStorageBackend::new(store, parser, "s3");
        assert_eq!(backend.storage_type(), "s3");
    }

    #[test]
    fn test_supports_multipart() {
        use object_store::memory::InMemory;

        let store = Arc::new(InMemory::new());
        let parser = S3PathParser;
        let backend = BaseStorageBackend::new(store, parser, "s3");
        assert!(backend.supports_multipart());
    }
}
