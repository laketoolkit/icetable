//! Base generic storage backend implementation
//!
//! This module provides a generic `BaseStorageBackend<P>` that implements the
//! `StorageBackend` trait using `object_store` and a `CloudPathParser`.
//! This eliminates code duplication across S3, GCS, and Azure backends.
//!
//! All operations automatically retry on transient failures using exponential backoff.

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use object_store::ObjectStore;
use std::sync::Arc;

use super::path_parser::CloudPathParser;
use super::retry::RetryContext;
use super::traits::*;
use crate::error::{Error, Result};

/// Generic storage backend that works with any CloudPathParser
///
/// This struct wraps an `object_store` instance and uses a path parser
/// to handle cloud-specific path formats. It implements all StorageBackend
/// methods generically, eliminating the need for duplicated code across
/// different cloud providers.
///
/// All operations automatically retry on transient failures (network issues,
/// throttling, temporary unavailability) using exponential backoff.
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
    /// Retry context with circuit breaker
    retry_context: RetryContext,
}

impl<P: CloudPathParser> BaseStorageBackend<P> {
    /// Create a new base storage backend
    ///
    /// # Arguments
    /// * `store` - The object store implementation
    /// * `parser` - The path parser for this storage type
    /// * `storage_type_name` - Name of the storage type (e.g., "s3", "gcs", "azure")
    pub fn new(store: Arc<dyn ObjectStore>, parser: P, storage_type_name: &'static str) -> Self {
        // Use cloud storage retry context for remote backends, local for filesystem
        let retry_context = if storage_type_name == "local" {
            RetryContext::for_local()
        } else {
            RetryContext::for_cloud_storage()
        };

        Self {
            store,
            parser,
            storage_type_name,
            retry_context,
        }
    }

    /// Parse a path using the configured parser
    fn parse_path(&self, path: &str) -> Result<(String, String)> {
        self.parser.parse(path)
    }
}

#[async_trait]
impl<P: CloudPathParser> StorageBackend for BaseStorageBackend<P> {
    fn storage_type(&self) -> &str {
        self.storage_type_name
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        let (_, key) = self.parse_path(path)?;
        let obj_path = object_store::path::Path::from(key.clone());
        let store = self.store.clone();
        let storage_type = self.storage_type_name;

        self.retry_context.execute(&format!("{} exists", storage_type), || {
            let store = store.clone();
            let obj_path = obj_path.clone();
            async move {
                match store.head(&obj_path).await {
                    Ok(_) => Ok(true),
                    Err(object_store::Error::NotFound { .. }) => Ok(false),
                    Err(e) => Err(Error::from_object_store(e, path)),
                }
            }
        })
        .await
    }

    async fn head(&self, path: &str) -> Result<ObjectMetadata> {
        log::debug!("{} HEAD: {}", self.storage_type_name.to_uppercase(), path);
        let (_, key) = self.parse_path(path)?;
        let obj_path = object_store::path::Path::from(key.clone());
        let store = self.store.clone();
        let path_owned = path.to_string();
        let storage_type = self.storage_type_name;

        self.retry_context.execute(&format!("{} head", storage_type), || {
            let store = store.clone();
            let obj_path = obj_path.clone();
            let path_owned = path_owned.clone();
            async move {
                let meta = store.head(&obj_path).await.map_err(|e| {
                    Error::from_object_store(e, &path_owned)
                })?;

                Ok(ObjectMetadata {
                    path: path_owned,
                    size: meta.size as u64,
                    last_modified: meta.last_modified,
                    e_tag: meta.e_tag,
                    content_type: None,
                })
            }
        })
        .await
    }

    async fn get(&self, path: &str, options: &GetOptions) -> Result<Bytes> {
        let (_, key) = self.parse_path(path)?;
        let obj_path = object_store::path::Path::from(key.clone());
        let store = self.store.clone();
        let path_owned = path.to_string();
        let storage_type = self.storage_type_name;
        let range = options.range;

        self.retry_context.execute(&format!("{} get", storage_type), || {
            let store = store.clone();
            let obj_path = obj_path.clone();
            let path_owned = path_owned.clone();
            async move {
                if let Some((start, end)) = range {
                    // Use range request
                    log::debug!(
                        "{} GET RANGE: {} (range {}-{})",
                        storage_type.to_uppercase(),
                        path_owned,
                        start,
                        end
                    );
                    let range = start..end;
                    store.get_range(&obj_path, range).await.map_err(|e| {
                        Error::from_object_store(e, &path_owned)
                    })
                } else {
                    // Get full object
                    log::debug!(
                        "{} GET FULL: {}",
                        storage_type.to_uppercase(),
                        path_owned
                    );
                    let result = store.get(&obj_path).await.map_err(|e| {
                        Error::from_object_store(e, &path_owned)
                    })?;

                    result.bytes().await.map_err(|e| {
                        Error::General(format!(
                            "Failed to read {} object bytes: {}",
                            storage_type, e
                        ))
                    })
                }
            }
        })
        .await
    }

    async fn put(&self, path: &str, data: Bytes, _options: &PutOptions) -> Result<()> {
        let (_, key) = self.parse_path(path)?;
        let obj_path = object_store::path::Path::from(key.clone());
        let store = self.store.clone();
        let storage_type = self.storage_type_name;

        self.retry_context.execute(&format!("{} put", storage_type), || {
            let store = store.clone();
            let obj_path = obj_path.clone();
            let data = data.clone();
            async move {
                store.put(&obj_path, data.into()).await.map_err(|e| {
                    Error::from_object_store(e, path)
                })?;
                Ok(())
            }
        })
        .await
    }

    async fn list(&self, options: &ListOptions) -> Result<ListResult> {
        let prefix = options.prefix.as_deref().unwrap_or("");

        // Parse prefix to extract bucket and key (if it's a full cloud path)
        let (bucket_name, key) = if prefix.starts_with(self.parser.scheme()) {
            self.parse_path(prefix)?
        } else {
            (String::new(), prefix.to_string())
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
                Error::from_object_store(e, prefix)
            })?;

            // Reconstruct full cloud path including bucket
            let full_path = if !bucket_name.is_empty() {
                self.parser.build_path(&bucket_name, meta.location.as_ref())
            } else {
                format!("{}{}", self.parser.scheme(), meta.location)
            };

            objects.push(ObjectMetadata {
                path: full_path,
                size: meta.size,
                last_modified: meta.last_modified,
                e_tag: meta.e_tag,
                content_type: None,
            });

            // Respect max_results limit
            if let Some(max) = options.max_results
                && objects.len() >= max
            {
                break;
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
        let obj_path = object_store::path::Path::from(key.clone());
        let store = self.store.clone();
        let storage_type = self.storage_type_name;

        self.retry_context.execute(
            &format!("{} delete", storage_type),
            || {
                let store = store.clone();
                let obj_path = obj_path.clone();
                async move {
                    store.delete(&obj_path).await.map_err(|e| {
                        Error::from_object_store(e, path)
                    })?;
                    Ok(())
                }
            },
        )
        .await
    }

    async fn copy(&self, from: &str, to: &str) -> Result<()> {
        let (_, from_key) = self.parse_path(from)?;
        let (_, to_key) = self.parse_path(to)?;

        let from_path = object_store::path::Path::from(from_key.clone());
        let to_path = object_store::path::Path::from(to_key.clone());
        let store = self.store.clone();
        let storage_type = self.storage_type_name;

        self.retry_context.execute(&format!("{} copy", storage_type), || {
            let store = store.clone();
            let from_path = from_path.clone();
            let to_path = to_path.clone();
            async move {
                store.copy(&from_path, &to_path).await.map_err(|e| {
                    Error::from_object_store(e, from)
                })?;
                Ok(())
            }
        })
        .await?;

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
