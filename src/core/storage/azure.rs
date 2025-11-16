//! Azure Blob Storage backend using object_store

use async_trait::async_trait;
use bytes::Bytes;
use object_store::azure::MicrosoftAzureBuilder;
use object_store::ObjectStore;
use std::sync::Arc;

use crate::core::storage::traits::*;
use crate::error::{Error, Result};

/// Azure Blob Storage backend
pub struct AzureBackend {
    store: Arc<dyn ObjectStore>,
}

impl AzureBackend {
    /// Create a new Azure backend from environment variables
    ///
    /// Expects the following environment variables:
    /// - AZURE_STORAGE_ACCOUNT_NAME
    /// - AZURE_STORAGE_ACCOUNT_KEY or AZURE_STORAGE_SAS_TOKEN
    /// - Or AZURE_STORAGE_USE_EMULATOR for local development
    ///
    /// The container name is extracted from the az:// or azure:// path
    pub async fn new(path: &str) -> Result<Self> {
        let (container, _) = Self::parse_azure_path(path)?;

        let store = MicrosoftAzureBuilder::from_env()
            .with_container_name(&container)
            .build()
            .map_err(|e| Error::Configuration {
                message: format!("Failed to create Azure backend: {}", e),
            })?;

        Ok(Self {
            store: Arc::new(store),
        })
    }

    /// Create a new Azure backend with explicit configuration
    pub async fn with_config(account: String, container: String) -> Result<Self> {
        let store = MicrosoftAzureBuilder::new()
            .with_account(account)
            .with_container_name(container)
            .build()
            .map_err(|e| Error::Configuration {
                message: format!("Failed to create Azure backend: {}", e),
            })?;

        Ok(Self {
            store: Arc::new(store),
        })
    }

    /// Parse Azure path to extract container and blob
    ///
    /// Expected formats:
    /// - az://container/blob/path
    /// - azure://container/blob/path
    fn parse_azure_path(path: &str) -> Result<(String, String)> {
        let without_scheme = path
            .strip_prefix("az://")
            .or_else(|| path.strip_prefix("azure://"))
            .ok_or_else(|| Error::Configuration {
                message: format!("Invalid Azure path (must start with az:// or azure://): {}", path),
            })?;

        let parts: Vec<&str> = without_scheme.splitn(2, '/').collect();
        if parts.len() != 2 {
            return Err(Error::Configuration {
                message: format!("Invalid Azure path (must be az://container/blob): {}", path),
            });
        }

        Ok((parts[0].to_string(), parts[1].to_string()))
    }
}

#[async_trait]
impl StorageBackend for AzureBackend {
    fn storage_type(&self) -> &str {
        "azure"
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        let (_, blob) = Self::parse_azure_path(path)?;
        let obj_path = object_store::path::Path::from(blob);

        match self.store.head(&obj_path).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(Error::General(format!("Failed to check Azure blob: {}", e))),
        }
    }

    async fn head(&self, path: &str) -> Result<ObjectMetadata> {
        let (_, blob) = Self::parse_azure_path(path)?;
        let obj_path = object_store::path::Path::from(blob);

        let meta = self.store.head(&obj_path).await.map_err(|e| {
            if matches!(e, object_store::Error::NotFound { .. }) {
                Error::FileNotFound {
                    path: std::path::PathBuf::from(path),
                }
            } else {
                Error::General(format!("Failed to get Azure blob metadata: {}", e))
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
        let (_, blob) = Self::parse_azure_path(path)?;
        let obj_path = object_store::path::Path::from(blob);

        if let Some((start, end)) = options.range {
            // Use range request
            let range = start..end;
            self.store
                .get_range(&obj_path, range)
                .await
                .map_err(|e| Error::General(format!("Failed to get Azure blob range: {}", e)))
        } else {
            // Get full blob
            let result = self.store.get(&obj_path).await.map_err(|e| {
                if matches!(e, object_store::Error::NotFound { .. }) {
                    Error::FileNotFound {
                        path: std::path::PathBuf::from(path),
                    }
                } else {
                    Error::General(format!("Failed to get Azure blob: {}", e))
                }
            })?;

            result
                .bytes()
                .await
                .map_err(|e| Error::General(format!("Failed to read Azure blob bytes: {}", e)))
        }
    }

    async fn put(&self, path: &str, data: Bytes, _options: &PutOptions) -> Result<()> {
        let (_, blob) = Self::parse_azure_path(path)?;
        let obj_path = object_store::path::Path::from(blob);

        self.store
            .put(&obj_path, data.into())
            .await
            .map_err(|e| Error::General(format!("Failed to put Azure blob: {}", e)))?;

        Ok(())
    }

    async fn list(&self, options: &ListOptions) -> Result<ListResult> {
        let prefix = options.prefix.as_deref().unwrap_or("");
        let (_container, blob) = if prefix.starts_with("az://") || prefix.starts_with("azure://") {
            Self::parse_azure_path(prefix)?
        } else {
            ("".to_string(), prefix.to_string())
        };

        let obj_path = if blob.is_empty() {
            None
        } else {
            Some(object_store::path::Path::from(blob.as_str()))
        };

        let mut objects = Vec::new();
        let mut stream = self.store.list(obj_path.as_ref());

        use futures::StreamExt;
        while let Some(result) = stream.next().await {
            let meta = result.map_err(|e| Error::General(format!("Failed to list Azure blobs: {}", e)))?;

            objects.push(ObjectMetadata {
                path: format!("az://{}", meta.location),
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
        let (_, blob) = Self::parse_azure_path(path)?;
        let obj_path = object_store::path::Path::from(blob);

        self.store
            .delete(&obj_path)
            .await
            .map_err(|e| Error::General(format!("Failed to delete Azure blob: {}", e)))?;

        Ok(())
    }

    async fn copy(&self, from: &str, to: &str) -> Result<()> {
        let (_, from_blob) = Self::parse_azure_path(from)?;
        let (_, to_blob) = Self::parse_azure_path(to)?;

        let from_path = object_store::path::Path::from(from_blob);
        let to_path = object_store::path::Path::from(to_blob);

        self.store
            .copy(&from_path, &to_path)
            .await
            .map_err(|e| Error::General(format!("Failed to copy Azure blob: {}", e)))?;

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
    fn test_parse_azure_path_az() {
        let (container, blob) = AzureBackend::parse_azure_path("az://my-container/path/to/file.parquet").unwrap();
        assert_eq!(container, "my-container");
        assert_eq!(blob, "path/to/file.parquet");
    }

    #[test]
    fn test_parse_azure_path_azure() {
        let (container, blob) = AzureBackend::parse_azure_path("azure://my-container/path/to/file.parquet").unwrap();
        assert_eq!(container, "my-container");
        assert_eq!(blob, "path/to/file.parquet");
    }

    #[test]
    fn test_parse_azure_path_invalid() {
        assert!(AzureBackend::parse_azure_path("az://container-only").is_err());
        assert!(AzureBackend::parse_azure_path("/local/path").is_err());
    }
}
