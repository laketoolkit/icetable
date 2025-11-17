//! AWS S3 storage backend using object_store

use async_trait::async_trait;
use object_store::aws::AmazonS3Builder;
use std::sync::Arc;

use super::base::BaseStorageBackend;
use super::path_parser::{CloudPathParser, S3PathParser};
use super::traits::StorageBackend;
use crate::error::{Error, Result};

/// AWS S3 storage backend
pub struct S3Backend {
    inner: BaseStorageBackend<S3PathParser>,
}

impl S3Backend {
    /// Create a new S3 backend from environment variables and path
    ///
    /// Expects the following environment variables:
    /// - AWS_ACCESS_KEY_ID
    /// - AWS_SECRET_ACCESS_KEY
    /// - AWS_ENDPOINT_URL (optional, for MinIO or custom S3 endpoints)
    /// - AWS_REGION or AWS_DEFAULT_REGION (optional, defaults to us-east-1)
    ///
    /// The bucket name is extracted from the s3:// path
    pub async fn new(path: &str) -> Result<Self> {
        log::debug!("Creating S3 backend for path: {}", path);
        let parser = S3PathParser;
        let (bucket, _) = parser.parse(path)?;
        log::debug!("Extracted bucket: {}", bucket);

        // Build from environment, then explicitly set bucket
        // The bucket from the path takes precedence over any environment variable
        let mut builder = AmazonS3Builder::from_env().with_bucket_name(bucket);

        // For MinIO compatibility, configure endpoint and path style
        if let Ok(endpoint) = std::env::var("AWS_ENDPOINT_URL") {
            // Parse endpoint to extract host and port
            if let Ok(endpoint_url) = url::Url::parse(&endpoint) {
                if let Some(_host) = endpoint_url.host_str() {
                    builder = builder.with_endpoint(endpoint);

                    // MinIO requires path-style URLs (bucket in path, not subdomain)
                    builder = builder.with_virtual_hosted_style_request(false);

                    // Allow HTTP if endpoint is not HTTPS
                    if endpoint_url.scheme() == "http" {
                        builder = builder.with_allow_http(true);
                    }
                }
            }
        }

        log::debug!(
            "Building S3 store with endpoint: {:?}",
            std::env::var("AWS_ENDPOINT_URL").ok()
        );
        let store = builder.build().map_err(|e| Error::Configuration {
            message: format!("Failed to create S3 backend: {}\nPath: {}\nMake sure AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, and AWS_ENDPOINT_URL (for MinIO) are set.", e, path),
        })?;

        log::debug!("S3 backend created successfully");
        Ok(Self {
            inner: BaseStorageBackend::new(Arc::new(store), parser, "s3"),
        })
    }

    /// Create a new S3 backend with explicit configuration
    pub async fn with_config(bucket: String, region: String) -> Result<Self> {
        let store = AmazonS3Builder::new()
            .with_bucket_name(bucket)
            .with_region(region)
            .build()
            .map_err(|e| Error::Configuration {
                message: format!("Failed to create S3 backend: {}", e),
            })?;

        Ok(Self {
            inner: BaseStorageBackend::new(Arc::new(store), S3PathParser, "s3"),
        })
    }

    /// Parse S3 path to extract bucket and key (for backward compatibility)
    ///
    /// Expected format: s3://bucket/key/path
    pub(crate) fn parse_s3_path(path: &str) -> Result<(String, String)> {
        S3PathParser.parse(path)
    }
}

// Delegate all StorageBackend methods to the inner BaseStorageBackend
#[async_trait]
impl StorageBackend for S3Backend {
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
    fn test_parse_s3_path() {
        let (bucket, key) =
            S3Backend::parse_s3_path("s3://my-bucket/path/to/file.parquet").unwrap();
        assert_eq!(bucket, "my-bucket");
        assert_eq!(key, "path/to/file.parquet");
    }

    #[test]
    fn test_parse_s3_path_invalid() {
        assert!(S3Backend::parse_s3_path("s3://bucket-only").is_err());
        assert!(S3Backend::parse_s3_path("/local/path").is_err());
    }
}
