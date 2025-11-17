//! Cloud path parsers for different storage providers
//!
//! This module provides a trait and implementations for parsing cloud storage paths
//! in various formats (S3, GCS, Azure) to extract bucket/container names and object keys.

use crate::error::{Error, Result};

/// Trait for parsing cloud storage paths into components
///
/// Different cloud providers use different URL schemes and terminology:
/// - S3: s3://bucket/key
/// - GCS: gs://bucket/key
/// - Azure: az://container/blob or azure://container/blob
pub trait CloudPathParser: Send + Sync {
    /// The scheme prefix for this storage type (e.g., "s3://", "gs://")
    fn scheme(&self) -> &str;

    /// Parse a cloud path into (bucket/container, key/blob) components
    fn parse(&self, path: &str) -> Result<(String, String)>;

    /// Get the storage type name for error messages
    fn storage_type(&self) -> &str;

    /// Reconstruct a full path from bucket and key
    fn build_path(&self, bucket: &str, key: &str) -> String {
        format!("{}{}/{}", self.scheme(), bucket, key)
    }
}

/// S3 path parser for s3://bucket/key format
#[derive(Debug, Clone, Copy)]
pub struct S3PathParser;

impl CloudPathParser for S3PathParser {
    fn scheme(&self) -> &str {
        "s3://"
    }

    fn storage_type(&self) -> &str {
        "S3"
    }

    fn parse(&self, path: &str) -> Result<(String, String)> {
        let without_scheme =
            path.strip_prefix(self.scheme())
                .ok_or_else(|| Error::Configuration {
                    message: format!(
                        "Invalid {} path (must start with {}): {}",
                        self.storage_type(),
                        self.scheme(),
                        path
                    ),
                })?;

        let parts: Vec<&str> = without_scheme.splitn(2, '/').collect();
        if parts.len() != 2 {
            return Err(Error::Configuration {
                message: format!(
                    "Invalid {} path (must be {}bucket/key): {}",
                    self.storage_type(),
                    self.scheme(),
                    path
                ),
            });
        }

        Ok((parts[0].to_string(), parts[1].to_string()))
    }
}

/// GCS path parser for gs://bucket/key format
#[derive(Debug, Clone, Copy)]
pub struct GcsPathParser;

impl CloudPathParser for GcsPathParser {
    fn scheme(&self) -> &str {
        "gs://"
    }

    fn storage_type(&self) -> &str {
        "GCS"
    }

    fn parse(&self, path: &str) -> Result<(String, String)> {
        let without_scheme =
            path.strip_prefix(self.scheme())
                .ok_or_else(|| Error::Configuration {
                    message: format!(
                        "Invalid {} path (must start with {}): {}",
                        self.storage_type(),
                        self.scheme(),
                        path
                    ),
                })?;

        let parts: Vec<&str> = without_scheme.splitn(2, '/').collect();
        if parts.len() != 2 {
            return Err(Error::Configuration {
                message: format!(
                    "Invalid {} path (must be {}bucket/key): {}",
                    self.storage_type(),
                    self.scheme(),
                    path
                ),
            });
        }

        Ok((parts[0].to_string(), parts[1].to_string()))
    }
}

/// Azure path parser for az://container/blob or azure://container/blob format
#[derive(Debug, Clone, Copy)]
pub struct AzurePathParser;

impl CloudPathParser for AzurePathParser {
    fn scheme(&self) -> &str {
        "az://"
    }

    fn storage_type(&self) -> &str {
        "Azure"
    }

    fn parse(&self, path: &str) -> Result<(String, String)> {
        // Azure accepts both az:// and azure:// schemes
        let without_scheme = path
            .strip_prefix("az://")
            .or_else(|| path.strip_prefix("azure://"))
            .ok_or_else(|| Error::Configuration {
                message: format!(
                    "Invalid {} path (must start with az:// or azure://): {}",
                    self.storage_type(),
                    path
                ),
            })?;

        let parts: Vec<&str> = without_scheme.splitn(2, '/').collect();
        if parts.len() != 2 {
            return Err(Error::Configuration {
                message: format!(
                    "Invalid {} path (must be az://container/blob): {}",
                    self.storage_type(),
                    path
                ),
            });
        }

        Ok((parts[0].to_string(), parts[1].to_string()))
    }

    fn build_path(&self, container: &str, blob: &str) -> String {
        format!("az://{}/{}", container, blob)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s3_parser_valid() {
        let parser = S3PathParser;
        let (bucket, key) = parser.parse("s3://my-bucket/path/to/file.parquet").unwrap();
        assert_eq!(bucket, "my-bucket");
        assert_eq!(key, "path/to/file.parquet");
    }

    #[test]
    fn test_s3_parser_invalid_no_key() {
        let parser = S3PathParser;
        assert!(parser.parse("s3://bucket-only").is_err());
    }

    #[test]
    fn test_s3_parser_invalid_scheme() {
        let parser = S3PathParser;
        assert!(parser.parse("/local/path").is_err());
    }

    #[test]
    fn test_s3_build_path() {
        let parser = S3PathParser;
        let path = parser.build_path("my-bucket", "path/to/file.parquet");
        assert_eq!(path, "s3://my-bucket/path/to/file.parquet");
    }

    #[test]
    fn test_gcs_parser_valid() {
        let parser = GcsPathParser;
        let (bucket, key) = parser.parse("gs://my-bucket/path/to/file.parquet").unwrap();
        assert_eq!(bucket, "my-bucket");
        assert_eq!(key, "path/to/file.parquet");
    }

    #[test]
    fn test_gcs_parser_invalid_no_key() {
        let parser = GcsPathParser;
        assert!(parser.parse("gs://bucket-only").is_err());
    }

    #[test]
    fn test_gcs_parser_invalid_scheme() {
        let parser = GcsPathParser;
        assert!(parser.parse("/local/path").is_err());
    }

    #[test]
    fn test_azure_parser_valid_az() {
        let parser = AzurePathParser;
        let (container, blob) = parser
            .parse("az://my-container/path/to/file.parquet")
            .unwrap();
        assert_eq!(container, "my-container");
        assert_eq!(blob, "path/to/file.parquet");
    }

    #[test]
    fn test_azure_parser_valid_azure() {
        let parser = AzurePathParser;
        let (container, blob) = parser
            .parse("azure://my-container/path/to/file.parquet")
            .unwrap();
        assert_eq!(container, "my-container");
        assert_eq!(blob, "path/to/file.parquet");
    }

    #[test]
    fn test_azure_parser_invalid_no_blob() {
        let parser = AzurePathParser;
        assert!(parser.parse("az://container-only").is_err());
    }

    #[test]
    fn test_azure_parser_invalid_scheme() {
        let parser = AzurePathParser;
        assert!(parser.parse("/local/path").is_err());
    }

    #[test]
    fn test_azure_build_path() {
        let parser = AzurePathParser;
        let path = parser.build_path("my-container", "path/to/file.parquet");
        assert_eq!(path, "az://my-container/path/to/file.parquet");
    }
}
