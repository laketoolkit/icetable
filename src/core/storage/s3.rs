//! AWS S3 storage backend
//!
//! NOTE: This is a stub implementation for Phase 2.
//! Full S3 integration with object_store crate will be implemented in Phase 3.

use async_trait::async_trait;
use bytes::Bytes;

use crate::core::storage::traits::*;
use crate::error::{Error, Result};

/// AWS S3 storage backend (stub implementation)
pub struct S3Backend {
    bucket: String,
    region: String,
}

impl S3Backend {
    /// Create a new S3 backend with configuration
    pub async fn new(bucket: String, region: String) -> Result<Self> {
        // Stub implementation - just store config
        Ok(Self { bucket, region })
    }

    /// Create from environment variables
    pub async fn from_env() -> Result<Self> {
        let bucket =
            std::env::var("AWS_S3_BUCKET").unwrap_or_else(|_| "default-bucket".to_string());
        let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());
        Self::new(bucket, region).await
    }
}

#[async_trait]
impl StorageBackend for S3Backend {
    fn storage_type(&self) -> &str {
        "s3"
    }

    async fn exists(&self, _path: &str) -> Result<bool> {
        Err(Error::UnsupportedFeature {
            feature: "S3Backend operations (Phase 3)".to_string(),
        })
    }

    async fn head(&self, _path: &str) -> Result<ObjectMetadata> {
        Err(Error::UnsupportedFeature {
            feature: "S3Backend operations (Phase 3)".to_string(),
        })
    }

    async fn get(&self, _path: &str, _options: &GetOptions) -> Result<Bytes> {
        Err(Error::UnsupportedFeature {
            feature: "S3Backend operations (Phase 3)".to_string(),
        })
    }

    async fn put(&self, _path: &str, _data: Bytes, _options: &PutOptions) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "S3Backend operations (Phase 3)".to_string(),
        })
    }

    async fn list(&self, _options: &ListOptions) -> Result<ListResult> {
        Err(Error::UnsupportedFeature {
            feature: "S3Backend operations (Phase 3)".to_string(),
        })
    }

    async fn delete(&self, _path: &str) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "S3Backend operations (Phase 3)".to_string(),
        })
    }

    async fn copy(&self, _from: &str, _to: &str) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "S3Backend operations (Phase 3)".to_string(),
        })
    }

    fn supports_multipart(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_s3_backend_creation() {
        let backend = S3Backend::new("test-bucket".to_string(), "us-west-2".to_string()).await;
        assert!(backend.is_ok());

        let backend = backend.unwrap();
        assert_eq!(backend.storage_type(), "s3");
        assert!(backend.supports_multipart());
    }

    #[tokio::test]
    async fn test_s3_operations_return_error() {
        let backend = S3Backend::new("test-bucket".to_string(), "us-west-2".to_string())
            .await
            .unwrap();

        // All operations should return UnsupportedFeature error
        assert!(backend.exists("test").await.is_err());
        assert!(backend.head("test").await.is_err());
        assert!(backend.get("test", &GetOptions::default()).await.is_err());
    }
}
