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
///
/// Supports various URL schemes:
/// - S3: `s3://`, `s3a://`
/// - GCS: `gs://`, `gcs://`
/// - Azure: `az://`, `azure://`, `abfs://`, `abfss://`
/// - Local: everything else (including `file://`)
pub fn detect_storage_type(path: &str) -> &'static str {
    if path.starts_with("s3://") || path.starts_with("s3a://") {
        "s3"
    } else if path.starts_with("gs://") || path.starts_with("gcs://") {
        "gcs"
    } else if path.starts_with("az://")
        || path.starts_with("azure://")
        || path.starts_with("abfs://")
        || path.starts_with("abfss://")
    {
        "azure"
    } else {
        "local"
    }
}

fn create_local_store(_path: &str) -> Result<Storage> {
    Ok(Arc::new(LocalFileSystem::new()))
}

/// Validate S3 bucket name according to AWS naming rules
fn validate_s3_bucket_name(bucket: &str) -> Result<()> {
    // AWS S3 bucket naming rules:
    // - 3-63 characters
    // - lowercase letters, numbers, hyphens, periods
    // - cannot start/end with hyphen or period
    // - cannot be IP address format
    if bucket.len() < 3 || bucket.len() > 63 {
        return Err(Error::Configuration {
            message: format!(
                "Invalid S3 bucket name '{}': must be 3-63 characters",
                bucket
            ),
        });
    }

    if !bucket
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
    {
        return Err(Error::Configuration {
            message: format!(
                "Invalid S3 bucket name '{}': only lowercase letters, numbers, hyphens, periods allowed",
                bucket
            ),
        });
    }

    if bucket.starts_with('-')
        || bucket.starts_with('.')
        || bucket.ends_with('-')
        || bucket.ends_with('.')
    {
        return Err(Error::Configuration {
            message: format!(
                "Invalid S3 bucket name '{}': cannot start/end with hyphen or period",
                bucket
            ),
        });
    }

    // Check for IP address format (simple heuristic)
    let parts: Vec<&str> = bucket.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
        return Err(Error::Configuration {
            message: format!(
                "Invalid S3 bucket name '{}': cannot be formatted as IP address",
                bucket
            ),
        });
    }

    Ok(())
}

/// Validate S3 key for security issues
fn validate_s3_key(key: &str) -> Result<()> {
    // Reject path traversal attempts
    if key.contains("..") {
        return Err(Error::Configuration {
            message: format!("Invalid S3 key: path traversal detected in '{}'", key),
        });
    }

    // Reject null bytes
    if key.contains('\0') {
        return Err(Error::Configuration {
            message: "Invalid S3 key: null byte detected".to_string(),
        });
    }

    Ok(())
}

fn create_s3_store(path: &str) -> Result<Storage> {
    let url = Url::parse(path).map_err(|e| Error::Configuration {
        message: format!("Invalid S3 URL: {}", e),
    })?;

    let bucket = url.host_str().ok_or_else(|| Error::Configuration {
        message: "Missing bucket in S3 URL".to_string(),
    })?;

    // Validate bucket name
    validate_s3_bucket_name(bucket)?;

    // Validate key (path component)
    let key = url.path();
    if !key.is_empty() && key != "/" {
        validate_s3_key(key)?;
    }

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

    Ok(Arc::new(store))
}

fn create_gcs_store(path: &str) -> Result<Storage> {
    let url = Url::parse(path).map_err(|e| Error::Configuration {
        message: format!("Invalid GCS URL: {}", e),
    })?;

    let bucket = url.host_str().ok_or_else(|| Error::Configuration {
        message: "Missing bucket in GCS URL".to_string(),
    })?;

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

    Ok(Arc::new(store))
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

    Ok(Arc::new(store))
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

    // S3 bucket name validation tests
    #[test]
    fn test_validate_s3_bucket_name_valid() {
        assert!(validate_s3_bucket_name("my-bucket").is_ok());
        assert!(validate_s3_bucket_name("my.bucket.name").is_ok());
        assert!(validate_s3_bucket_name("bucket123").is_ok());
        assert!(validate_s3_bucket_name("abc").is_ok()); // minimum 3 chars
    }

    #[test]
    fn test_validate_s3_bucket_name_too_short() {
        let result = validate_s3_bucket_name("ab");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("3-63 characters"));
    }

    #[test]
    fn test_validate_s3_bucket_name_too_long() {
        let long_name = "a".repeat(64);
        let result = validate_s3_bucket_name(&long_name);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("3-63 characters"));
    }

    #[test]
    fn test_validate_s3_bucket_name_invalid_chars() {
        let result = validate_s3_bucket_name("My-Bucket"); // uppercase
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("only lowercase letters")
        );
    }

    #[test]
    fn test_validate_s3_bucket_name_invalid_start_end() {
        assert!(validate_s3_bucket_name("-bucket").is_err());
        assert!(validate_s3_bucket_name("bucket-").is_err());
        assert!(validate_s3_bucket_name(".bucket").is_err());
        assert!(validate_s3_bucket_name("bucket.").is_err());
    }

    #[test]
    fn test_validate_s3_bucket_name_ip_address() {
        let result = validate_s3_bucket_name("192.168.1.1");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("IP address"));
    }

    // S3 key validation tests
    #[test]
    fn test_validate_s3_key_valid() {
        assert!(validate_s3_key("/prefix/table/data/file.parquet").is_ok());
        assert!(validate_s3_key("/some/path").is_ok());
    }

    #[test]
    fn test_validate_s3_key_path_traversal() {
        let result = validate_s3_key("/prefix/../etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path traversal"));
    }

    #[test]
    fn test_validate_s3_key_null_byte() {
        let result = validate_s3_key("/prefix/file\0.parquet");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("null byte"));
    }
}
