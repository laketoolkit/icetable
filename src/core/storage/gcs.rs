//! Google Cloud Storage backend using object_store

use async_trait::async_trait;
use bytes::Bytes;
use object_store::gcp::GoogleCloudStorageBuilder;
use object_store::ObjectStore;
use std::sync::Arc;

use crate::core::storage::traits::*;
use crate::error::{Error, Result};

/// Google Cloud Storage backend
pub struct GcsBackend {
    store: Arc<dyn ObjectStore>,
}

impl GcsBackend {
    /// Create a new GCS backend from environment variables
    ///
    /// Expects the following environment variables:
    /// - GOOGLE_SERVICE_ACCOUNT (path to service account key file)
    /// - Or GOOGLE_SERVICE_ACCOUNT_KEY (JSON content directly)
    /// - Or Application Default Credentials (ADC)
    ///
    /// The bucket name is extracted from the gs:// path
    pub async fn new(path: &str) -> Result<Self> {
        let (bucket, _) = Self::parse_gcs_path(path)?;

        let store = GoogleCloudStorageBuilder::from_env()
            .with_bucket_name(&bucket)
            .build()
            .map_err(|e| Error::Configuration {
                message: format!("Failed to create GCS backend: {}", e),
            })?;

        Ok(Self {
            store: Arc::new(store),
        })
    }

    /// Create a new GCS backend with explicit configuration
    pub async fn with_config(bucket: String) -> Result<Self> {
        let store = GoogleCloudStorageBuilder::new()
            .with_bucket_name(bucket)
            .build()
            .map_err(|e| Error::Configuration {
                message: format!("Failed to create GCS backend: {}", e),
            })?;

        Ok(Self {
            store: Arc::new(store),
        })
    }

    /// Parse GCS path to extract bucket and key
    ///
    /// Expected format: gs://bucket/key/path
    fn parse_gcs_path(path: &str) -> Result<(String, String)> {
        let without_scheme = path
            .strip_prefix("gs://")
            .ok_or_else(|| Error::Configuration {
                message: format!("Invalid GCS path (must start with gs://): {}", path),
            })?;

        let parts: Vec<&str> = without_scheme.splitn(2, '/').collect();
        if parts.len() != 2 {
            return Err(Error::Configuration {
                message: format!("Invalid GCS path (must be gs://bucket/key): {}", path),
            });
        }

        Ok((parts[0].to_string(), parts[1].to_string()))
    }
}

#[async_trait]
impl StorageBackend for GcsBackend {
    fn storage_type(&self) -> &str {
        "gcs"
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        let (_, key) = Self::parse_gcs_path(path)?;
        let obj_path = object_store::path::Path::from(key);

        match self.store.head(&obj_path).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(Error::General(format!("Failed to check GCS object: {}", e))),
        }
    }

    async fn head(&self, path: &str) -> Result<ObjectMetadata> {
        let (_, key) = Self::parse_gcs_path(path)?;
        let obj_path = object_store::path::Path::from(key);

        let meta = self.store.head(&obj_path).await.map_err(|e| {
            if matches!(e, object_store::Error::NotFound { .. }) {
                Error::FileNotFound {
                    path: std::path::PathBuf::from(path),
                }
            } else {
                Error::General(format!("Failed to get GCS object metadata: {}", e))
            }
        })?;

        Ok(ObjectMetadata {
            path: path.to_string(),
            size: meta.size as u64,
            last_modified: meta.last_modified,
            e_tag: meta.e_tag,
            content_type: None,
        })
    }

    async fn get(&self, path: &str, options: &GetOptions) -> Result<Bytes> {
        let (_, key) = Self::parse_gcs_path(path)?;
        let obj_path = object_store::path::Path::from(key);

        if let Some((start, end)) = options.range {
            // Use range request
            let range = start..end;
            self.store
                .get_range(&obj_path, range)
                .await
                .map_err(|e| Error::General(format!("Failed to get GCS object range: {}", e)))
        } else {
            // Get full object
            let result = self.store.get(&obj_path).await.map_err(|e| {
                if matches!(e, object_store::Error::NotFound { .. }) {
                    Error::FileNotFound {
                        path: std::path::PathBuf::from(path),
                    }
                } else {
                    Error::General(format!("Failed to get GCS object: {}", e))
                }
            })?;

            result
                .bytes()
                .await
                .map_err(|e| Error::General(format!("Failed to read GCS object bytes: {}", e)))
        }
    }

    async fn put(&self, path: &str, data: Bytes, _options: &PutOptions) -> Result<()> {
        let (_, key) = Self::parse_gcs_path(path)?;
        let obj_path = object_store::path::Path::from(key);

        self.store
            .put(&obj_path, data.into())
            .await
            .map_err(|e| Error::General(format!("Failed to put GCS object: {}", e)))?;

        Ok(())
    }

    async fn list(&self, options: &ListOptions) -> Result<ListResult> {
        let prefix = options.prefix.as_deref().unwrap_or("");
        let (_bucket, key) = if prefix.starts_with("gs://") {
            Self::parse_gcs_path(prefix)?
        } else {
            ("".to_string(), prefix.to_string())
        };

        let obj_path = if key.is_empty() {
            None
        } else {
            Some(object_store::path::Path::from(key.as_str()))
        };

        let mut objects = Vec::new();
        let mut stream = self.store.list(obj_path.as_ref());

        use futures::StreamExt;
        while let Some(result) = stream.next().await {
            let meta = result.map_err(|e| Error::General(format!("Failed to list GCS objects: {}", e)))?;

            objects.push(ObjectMetadata {
                path: format!("gs://{}", meta.location),
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
        let (_, key) = Self::parse_gcs_path(path)?;
        let obj_path = object_store::path::Path::from(key);

        self.store
            .delete(&obj_path)
            .await
            .map_err(|e| Error::General(format!("Failed to delete GCS object: {}", e)))?;

        Ok(())
    }

    async fn copy(&self, from: &str, to: &str) -> Result<()> {
        let (_, from_key) = Self::parse_gcs_path(from)?;
        let (_, to_key) = Self::parse_gcs_path(to)?;

        let from_path = object_store::path::Path::from(from_key);
        let to_path = object_store::path::Path::from(to_key);

        self.store
            .copy(&from_path, &to_path)
            .await
            .map_err(|e| Error::General(format!("Failed to copy GCS object: {}", e)))?;

        Ok(())
    }

    fn supports_multipart(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gcs_path() {
        let (bucket, key) = GcsBackend::parse_gcs_path("gs://my-bucket/path/to/file.parquet").unwrap();
        assert_eq!(bucket, "my-bucket");
        assert_eq!(key, "path/to/file.parquet");
    }

    #[test]
    fn test_parse_gcs_path_invalid() {
        assert!(GcsBackend::parse_gcs_path("gs://bucket-only").is_err());
        assert!(GcsBackend::parse_gcs_path("/local/path").is_err());
    }
}
