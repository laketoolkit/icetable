//! Storage factory for creating object_store instances
//!
//! This module provides a unified way to create [`ObjectStore`] instances
//! from URLs. It auto-detects the storage type (local, S3, GCS, Azure) and
//! creates the appropriate backend.
//!
//! # Example
//!
//! ```ignore
//! use icetable::core::storage::create_object_store;
//!
//! // Local filesystem
//! let store = create_object_store("/path/to/table").await?;
//!
//! // S3
//! let store = create_object_store("s3://bucket/prefix").await?;
//!
//! // GCS
//! let store = create_object_store("gs://bucket/prefix").await?;
//! ```

use std::sync::Arc;

use object_store::ObjectStore;
use object_store::aws::AmazonS3Builder;
use object_store::azure::MicrosoftAzureBuilder;
use object_store::gcp::GoogleCloudStorageBuilder;
use object_store::local::LocalFileSystem;
use object_store::prefix::PrefixStore;
use url::Url;

use crate::error::{Error, Result};

/// Type alias for the standard object store reference
pub type Storage = Arc<dyn ObjectStore>;

/// Create an object store from a path/URL
///
/// Supports:
/// - Local paths: `/path/to/table` or `file:///path/to/table`
/// - S3: `s3://bucket/prefix`
/// - GCS: `gs://bucket/prefix`
/// - Azure: `az://container/prefix` or `azure://container/prefix`
///
/// For cloud storage, credentials are read from environment variables:
/// - S3: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_ENDPOINT_URL` (optional)
/// - GCS: `GOOGLE_APPLICATION_CREDENTIALS` or `GOOGLE_SERVICE_ACCOUNT_KEY`
/// - Azure: `AZURE_STORAGE_ACCOUNT_NAME`, `AZURE_STORAGE_ACCOUNT_KEY`
pub async fn create_object_store(path: &str) -> Result<Storage> {
    let (storage_type, normalized) = parse_storage_url(path)?;

    match storage_type.as_str() {
        "local" => create_local_store(&normalized),
        "s3" => create_s3_store(path),
        "gcs" => create_gcs_store(path),
        "azure" => create_azure_store(path),
        _ => Err(Error::Configuration {
            message: format!("Unknown storage type: {}", storage_type),
        }),
    }
}

/// Parse a path/URL to determine the storage type
pub fn parse_storage_url(path: &str) -> Result<(String, String)> {
    if let Some(url) = path.strip_prefix("s3://") {
        let parts: Vec<&str> = url.splitn(2, '/').collect();
        if parts.len() == 2 {
            Ok(("s3".to_string(), path.to_string()))
        } else {
            Err(Error::Configuration {
                message: format!("Invalid S3 URL: {}", path),
            })
        }
    } else if path.strip_prefix("gs://").is_some() {
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

/// Detect storage type from a path
pub fn detect_storage_type(path: &str) -> &'static str {
    if path.starts_with("s3://") {
        "s3"
    } else if path.starts_with("gs://") {
        "gcs"
    } else if path.starts_with("az://") || path.starts_with("azure://") {
        "azure"
    } else {
        "local"
    }
}

fn create_local_store(path: &str) -> Result<Storage> {
    // LocalFileSystem operates on absolute paths
    // We use PrefixStore to make it work relative to the table path
    let local = LocalFileSystem::new();

    // Normalize the path (remove trailing slashes, etc.)
    let normalized = std::path::Path::new(path)
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(path));

    let prefix = normalized.to_string_lossy();

    // If the path is root or empty, return the store directly
    if prefix.is_empty() || prefix == "/" {
        Ok(Arc::new(local))
    } else {
        // Strip leading slash for PrefixStore (it expects relative paths)
        let prefix_str = prefix.trim_start_matches('/');
        Ok(Arc::new(PrefixStore::new(local, prefix_str)))
    }
}

fn create_s3_store(path: &str) -> Result<Storage> {
    let url = Url::parse(path).map_err(|e| Error::Configuration {
        message: format!("Invalid S3 URL: {}", e),
    })?;

    let bucket = url.host_str().ok_or_else(|| Error::Configuration {
        message: "Missing bucket in S3 URL".to_string(),
    })?;

    let prefix = url.path().trim_start_matches('/');

    let mut builder = AmazonS3Builder::from_env().with_bucket_name(bucket);

    // Handle custom endpoint (MinIO, LocalStack, etc.)
    if let Ok(endpoint) = std::env::var("AWS_ENDPOINT_URL") {
        builder = builder
            .with_endpoint(&endpoint)
            .with_virtual_hosted_style_request(false)
            .with_allow_http(endpoint.starts_with("http://"));
    }

    let store = builder.build().map_err(|e| Error::Configuration {
        message: format!(
            "Failed to create S3 store: {}\n\
             Make sure AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY are set.\n\
             For MinIO/LocalStack, also set AWS_ENDPOINT_URL.",
            e
        ),
    })?;

    if prefix.is_empty() {
        Ok(Arc::new(store))
    } else {
        Ok(Arc::new(PrefixStore::new(store, prefix)))
    }
}

fn create_gcs_store(path: &str) -> Result<Storage> {
    let url = Url::parse(path).map_err(|e| Error::Configuration {
        message: format!("Invalid GCS URL: {}", e),
    })?;

    let bucket = url.host_str().ok_or_else(|| Error::Configuration {
        message: "Missing bucket in GCS URL".to_string(),
    })?;

    let prefix = url.path().trim_start_matches('/');

    let store = GoogleCloudStorageBuilder::from_env()
        .with_bucket_name(bucket)
        .build()
        .map_err(|e| Error::Configuration {
            message: format!(
                "Failed to create GCS store: {}\n\
                 Make sure GOOGLE_APPLICATION_CREDENTIALS or GOOGLE_SERVICE_ACCOUNT_KEY is set.",
                e
            ),
        })?;

    if prefix.is_empty() {
        Ok(Arc::new(store))
    } else {
        Ok(Arc::new(PrefixStore::new(store, prefix)))
    }
}

fn create_azure_store(path: &str) -> Result<Storage> {
    // Normalize az:// to azure://
    let normalized = if let Some(p) = path.strip_prefix("az://") {
        format!("azure://{}", p)
    } else {
        path.to_string()
    };

    let url = Url::parse(&normalized).map_err(|e| Error::Configuration {
        message: format!("Invalid Azure URL: {}", e),
    })?;

    let container = url.host_str().ok_or_else(|| Error::Configuration {
        message: "Missing container in Azure URL".to_string(),
    })?;

    let prefix = url.path().trim_start_matches('/');

    let store = MicrosoftAzureBuilder::from_env()
        .with_container_name(container)
        .build()
        .map_err(|e| Error::Configuration {
            message: format!(
                "Failed to create Azure store: {}\n\
                 Make sure AZURE_STORAGE_ACCOUNT_NAME and AZURE_STORAGE_ACCOUNT_KEY are set.",
                e
            ),
        })?;

    if prefix.is_empty() {
        Ok(Arc::new(store))
    } else {
        Ok(Arc::new(PrefixStore::new(store, prefix)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_local_path() {
        let (storage_type, path) = parse_storage_url("/tmp/table").unwrap();
        assert_eq!(storage_type, "local");
        assert_eq!(path, "/tmp/table");
    }

    #[test]
    fn test_parse_file_url() {
        let (storage_type, path) = parse_storage_url("file:///tmp/table").unwrap();
        assert_eq!(storage_type, "local");
        assert_eq!(path, "/tmp/table");
    }

    #[test]
    fn test_parse_s3_url() {
        let (storage_type, _) = parse_storage_url("s3://bucket/prefix/table").unwrap();
        assert_eq!(storage_type, "s3");
    }

    #[test]
    fn test_parse_gcs_url() {
        let (storage_type, _) = parse_storage_url("gs://bucket/prefix").unwrap();
        assert_eq!(storage_type, "gcs");
    }

    #[test]
    fn test_parse_azure_url() {
        let (storage_type, _) = parse_storage_url("az://container/prefix").unwrap();
        assert_eq!(storage_type, "azure");

        let (storage_type, _) = parse_storage_url("azure://container/prefix").unwrap();
        assert_eq!(storage_type, "azure");
    }

    #[test]
    fn test_detect_storage_type() {
        assert_eq!(detect_storage_type("/local/path"), "local");
        assert_eq!(detect_storage_type("s3://bucket/key"), "s3");
        assert_eq!(detect_storage_type("gs://bucket/key"), "gcs");
        assert_eq!(detect_storage_type("az://container/key"), "azure");
    }
}
