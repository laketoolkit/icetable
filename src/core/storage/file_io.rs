//! Centralized FileIO creation for Iceberg operations
//!
//! This module provides a single source of truth for creating `iceberg::io::FileIO`
//! instances, avoiding duplication across the codebase.

use iceberg::io::{FileIO, FileIOBuilder};

use crate::error::{Error, Result};

/// Create a FileIO instance for Iceberg operations based on the path scheme.
///
/// This function centralizes all FileIO creation logic to ensure consistent
/// configuration across the codebase. It automatically detects the storage
/// backend from the path prefix and configures it appropriately.
///
/// # Supported schemes
///
/// - `s3://` or `s3a://` - Amazon S3 (uses AWS_* environment variables)
/// - `gs://` or `gcs://` - Google Cloud Storage (uses ADC)
/// - `az://`, `abfs://`, `abfss://` - Azure Blob Storage
/// - Local paths - Local filesystem
///
/// # Environment Variables (S3)
///
/// - `AWS_ACCESS_KEY_ID` - Access key
/// - `AWS_SECRET_ACCESS_KEY` - Secret key
/// - `AWS_SESSION_TOKEN` - Session token (optional)
/// - `AWS_ENDPOINT_URL` - Custom endpoint (for MinIO, LocalStack, etc.)
/// - `AWS_REGION` or `AWS_DEFAULT_REGION` - Region (defaults to us-east-1)
///
/// # Example
///
/// ```ignore
/// use icetable::core::storage::create_file_io;
///
/// let file_io = create_file_io("s3://my-bucket/tables")?;
/// ```
pub fn create_file_io(path: &str) -> Result<FileIO> {
    // Use unified detect_storage_type from factory
    match super::factory::detect_storage_type(path) {
        "s3" => create_s3_file_io(),
        "gcs" => create_gcs_file_io(),
        "azure" => create_azure_file_io(),
        _ => create_local_file_io(),
    }
}

/// Create S3 FileIO with configuration from environment variables
fn create_s3_file_io() -> Result<FileIO> {
    let mut builder = FileIOBuilder::new("s3");

    // Credentials
    if let Ok(key) = std::env::var("AWS_ACCESS_KEY_ID") {
        builder = builder.with_prop("s3.access-key-id", key);
    }
    if let Ok(secret) = std::env::var("AWS_SECRET_ACCESS_KEY") {
        builder = builder.with_prop("s3.secret-access-key", secret);
    }
    if let Ok(token) = std::env::var("AWS_SESSION_TOKEN") {
        builder = builder.with_prop("s3.session-token", token);
    }

    // Endpoint (for MinIO, LocalStack, etc.)
    if let Ok(endpoint) = std::env::var("AWS_ENDPOINT_URL") {
        builder = builder.with_prop("s3.endpoint", endpoint);
    }

    // Region
    if let Ok(region) = std::env::var("AWS_REGION") {
        builder = builder.with_prop("s3.region", region);
    } else if let Ok(region) = std::env::var("AWS_DEFAULT_REGION") {
        builder = builder.with_prop("s3.region", region);
    } else {
        builder = builder.with_prop("s3.region", "us-east-1");
    }

    // Path-style access (required for MinIO and some S3-compatible services)
    builder = builder.with_prop("s3.path-style-access", "true");

    builder.build().map_err(|e| Error::CloudStorage {
        provider: "S3".to_string(),
        message: format!("Failed to create FileIO: {}", e),
        error_code: None,
        http_status: None,
    })
}

/// Create GCS FileIO (uses Application Default Credentials)
fn create_gcs_file_io() -> Result<FileIO> {
    FileIOBuilder::new("gcs")
        .build()
        .map_err(|e| Error::CloudStorage {
            provider: "GCS".to_string(),
            message: format!("Failed to create FileIO: {}", e),
            error_code: None,
            http_status: None,
        })
}

/// Create Azure Blob Storage FileIO
fn create_azure_file_io() -> Result<FileIO> {
    FileIOBuilder::new("azblob")
        .build()
        .map_err(|e| Error::CloudStorage {
            provider: "Azure".to_string(),
            message: format!("Failed to create FileIO: {}", e),
            error_code: None,
            http_status: None,
        })
}

/// Create local filesystem FileIO
fn create_local_file_io() -> Result<FileIO> {
    FileIOBuilder::new_fs_io()
        .build()
        .map_err(|e| Error::Configuration {
            message: format!("Failed to create local FileIO: {}", e),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_local_file_io() {
        let result = create_file_io("/tmp/test");
        assert!(result.is_ok());
    }
}
