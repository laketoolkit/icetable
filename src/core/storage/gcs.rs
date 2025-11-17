//! Google Cloud Storage backend using object_store

use async_trait::async_trait;
use object_store::gcp::GoogleCloudStorageBuilder;
use std::sync::Arc;

use super::base::BaseStorageBackend;
use super::path_parser::{CloudPathParser, GcsPathParser};
use super::traits::StorageBackend;
use crate::error::{Error, Result};

/// Google Cloud Storage backend
pub struct GcsBackend {
    inner: BaseStorageBackend<GcsPathParser>,
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
        log::debug!("Creating GCS backend for path: {}", path);
        let parser = GcsPathParser;
        let (bucket, _) = parser.parse(path)?;
        log::debug!("Extracted bucket: {}", bucket);

        let store = GoogleCloudStorageBuilder::from_env()
            .with_bucket_name(&bucket)
            .build()
            .map_err(|e| Error::Configuration {
                message: format!("Failed to create GCS backend: {}\nPath: {}\nMake sure GOOGLE_SERVICE_ACCOUNT or GOOGLE_SERVICE_ACCOUNT_KEY is set.", e, path),
            })?;

        log::debug!("GCS backend created successfully");
        Ok(Self {
            inner: BaseStorageBackend::new(Arc::new(store), parser, "gcs"),
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
            inner: BaseStorageBackend::new(Arc::new(store), GcsPathParser, "gcs"),
        })
    }

    /// Parse GCS path to extract bucket and key (for backward compatibility)
    ///
    /// Expected format: gs://bucket/key/path
    pub(crate) fn parse_gcs_path(path: &str) -> Result<(String, String)> {
        GcsPathParser.parse(path)
    }
}

// Delegate all StorageBackend methods to the inner BaseStorageBackend
#[async_trait]
impl StorageBackend for GcsBackend {
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
    fn test_parse_gcs_path() {
        let (bucket, key) =
            GcsBackend::parse_gcs_path("gs://my-bucket/path/to/file.parquet").unwrap();
        assert_eq!(bucket, "my-bucket");
        assert_eq!(key, "path/to/file.parquet");
    }

    #[test]
    fn test_parse_gcs_path_invalid() {
        assert!(GcsBackend::parse_gcs_path("gs://bucket-only").is_err());
        assert!(GcsBackend::parse_gcs_path("/local/path").is_err());
    }
}
