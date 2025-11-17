//! Azure Blob Storage backend using object_store

use async_trait::async_trait;
use object_store::azure::MicrosoftAzureBuilder;
use std::sync::Arc;

use super::base::BaseStorageBackend;
use super::path_parser::{AzurePathParser, CloudPathParser};
use super::traits::StorageBackend;
use crate::error::{Error, Result};

/// Azure Blob Storage backend
pub struct AzureBackend {
    inner: BaseStorageBackend<AzurePathParser>,
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
        log::debug!("Creating Azure backend for path: {}", path);
        let parser = AzurePathParser;
        let (container, _) = parser.parse(path)?;
        log::debug!("Extracted container: {}", container);

        let store = MicrosoftAzureBuilder::from_env()
            .with_container_name(&container)
            .build()
            .map_err(|e| Error::Configuration {
                message: format!("Failed to create Azure backend: {}\nPath: {}\nMake sure AZURE_STORAGE_ACCOUNT_NAME and AZURE_STORAGE_ACCOUNT_KEY (or AZURE_STORAGE_SAS_TOKEN) are set.", e, path),
            })?;

        log::debug!("Azure backend created successfully");
        Ok(Self {
            inner: BaseStorageBackend::new(Arc::new(store), parser, "azure"),
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
            inner: BaseStorageBackend::new(Arc::new(store), AzurePathParser, "azure"),
        })
    }

    /// Parse Azure path to extract container and blob (for backward compatibility)
    ///
    /// Expected formats:
    /// - az://container/blob/path
    /// - azure://container/blob/path
    pub(crate) fn parse_azure_path(path: &str) -> Result<(String, String)> {
        AzurePathParser.parse(path)
    }
}

// Delegate all StorageBackend methods to the inner BaseStorageBackend
#[async_trait]
impl StorageBackend for AzureBackend {
    fn storage_type(&self) -> &str {
        self.inner.storage_type()
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        self.inner.exists(path).await
    }

    async fn head(&self, path: &str) -> Result<super::traits::ObjectMetadata> {
        self.inner.head(path).await
    }

    async fn get(&self, path: &str, options: &super::traits::GetOptions) -> Result<bytes::Bytes> {
        self.inner.get(path, options).await
    }

    async fn put(
        &self,
        path: &str,
        data: bytes::Bytes,
        options: &super::traits::PutOptions,
    ) -> Result<()> {
        self.inner.put(path, data, options).await
    }

    async fn list(
        &self,
        options: &super::traits::ListOptions,
    ) -> Result<super::traits::ListResult> {
        self.inner.list(options).await
    }

    async fn delete(&self, path: &str) -> Result<()> {
        self.inner.delete(path).await
    }

    async fn copy(&self, from: &str, to: &str) -> Result<()> {
        self.inner.copy(from, to).await
    }

    fn supports_multipart(&self) -> bool {
        self.inner.supports_multipart()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_azure_path_az() {
        let (container, blob) =
            AzureBackend::parse_azure_path("az://my-container/path/to/file.parquet").unwrap();
        assert_eq!(container, "my-container");
        assert_eq!(blob, "path/to/file.parquet");
    }

    #[test]
    fn test_parse_azure_path_azure() {
        let (container, blob) =
            AzureBackend::parse_azure_path("azure://my-container/path/to/file.parquet").unwrap();
        assert_eq!(container, "my-container");
        assert_eq!(blob, "path/to/file.parquet");
    }

    #[test]
    fn test_parse_azure_path_invalid() {
        assert!(AzureBackend::parse_azure_path("az://container-only").is_err());
        assert!(AzureBackend::parse_azure_path("/local/path").is_err());
    }
}
