//! Error types for TableTools
//!
//! Defines a comprehensive error hierarchy following the specification in section 7.4.
//! All errors implement std::error::Error and are designed to provide actionable
//! error messages to users.

use regex;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// The main error type for TableTools operations
#[derive(Error, Debug)]
pub enum Error {
    /// Errors related to I/O operations
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// File not found errors with context
    #[error("File not found: {path}")]
    FileNotFound {
        /// The path to the file that was not found
        path: PathBuf,
    },

    /// Permission denied errors
    #[error("Permission denied accessing: {path}")]
    PermissionDenied {
        /// The path that could not be accessed
        path: PathBuf,
    },

    /// Errors parsing or reading table formats
    #[error("Parse error: {message}")]
    Parse {
        /// The error message describing what failed
        message: String,
        /// The underlying error source, if available
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Corrupted file errors
    #[error("Corrupted file: {path} - {reason}")]
    CorruptedFile {
        /// The path to the corrupted file
        path: PathBuf,
        /// The reason the file is considered corrupted
        reason: String,
    },

    /// Invalid format errors
    #[error("Invalid format: {message}")]
    InvalidFormat {
        /// The error message describing the format issue
        message: String,
    },

    /// Schema validation errors
    #[error("Schema validation failed: {message}")]
    SchemaValidation {
        /// The error message describing the validation failure
        message: String,
    },

    /// Data validation errors
    #[error("Data validation failed: {message}")]
    DataValidation {
        /// The error message describing the data validation failure
        message: String,
    },

    /// Type conversion errors
    #[error("Type conversion error: {message}")]
    TypeConversion {
        /// The error message describing the conversion failure
        message: String,
    },

    /// Unsupported feature errors
    #[error("Unsupported feature: {feature}")]
    UnsupportedFeature {
        /// The name of the unsupported feature
        feature: String,
    },

    /// Cloud storage authentication errors
    #[error("Authentication failed for {provider}: {message}")]
    AuthenticationFailed {
        /// The cloud storage provider (e.g., S3, GCS, Azure)
        provider: String,
        /// The error message describing the authentication failure
        message: String,
    },

    /// Cloud storage network errors
    #[error("Network error: {message}")]
    Network {
        /// The error message describing the network failure
        message: String,
        /// The underlying error source, if available
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Timeout errors
    #[error("Operation timed out after {seconds}s: {operation}")]
    Timeout {
        /// The operation that timed out
        operation: String,
        /// The timeout duration in seconds
        seconds: u64,
    },

    /// Cloud storage access denied
    #[error("Access denied to {path}: {message}")]
    AccessDenied {
        /// The path or resource that was denied
        path: String,
        /// The error message describing the access denial
        message: String,
    },

    /// Configuration errors
    #[error("Configuration error: {message}")]
    Configuration {
        /// The error message describing the configuration issue
        message: String,
    },

    /// Arrow-specific errors
    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    /// Parquet-specific errors
    #[error("Parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    /// Object store errors (S3, GCS, Azure)
    #[error("Storage error: {0}")]
    ObjectStore(#[from] object_store::Error),

    /// Cloud storage provider-specific errors
    #[error("{provider} error: {message}")]
    CloudStorage {
        /// The cloud storage provider (e.g., "S3", "GCS", "Azure")
        provider: String,
        /// The error message describing the failure
        message: String,
        /// The underlying error code if available
        error_code: Option<String>,
        /// The HTTP status code if available
        http_status: Option<u16>,
    },

    /// Metadata-related errors (loading, parsing, writing)
    #[error("Metadata error: {message}")]
    Metadata {
        /// The error message describing what failed
        message: String,
    },

    /// Manifest-related errors (loading, parsing manifests)
    #[error("Manifest error: {message}")]
    Manifest {
        /// The error message describing what failed
        message: String,
    },

    /// Serialization/deserialization errors
    #[error("Serialization error: {message}")]
    Serialization {
        /// The error message describing what failed
        message: String,
    },

    /// Column not found in schema
    #[error("Column not found: {column}")]
    ColumnNotFound {
        /// The column name that was not found
        column: String,
    },

    /// Snapshot not found
    #[error("Snapshot not found: {snapshot_id}")]
    SnapshotNotFound {
        /// The snapshot ID that was not found
        snapshot_id: i64,
    },

    /// Table not found or not valid
    #[error("Table not found or invalid: {path}")]
    TableNotFound {
        /// The path to the table
        path: String,
    },

    /// General errors with context (use sparingly - prefer specific variants)
    #[error("{0}")]
    General(String),

    /// Concurrent modification conflict (optimistic concurrency)
    #[error("Conflict: {0}")]
    Conflict(String),

    /// Operation was cancelled by user (Ctrl+C)
    #[error("Operation cancelled by user")]
    Cancelled,

    /// Memory limit exceeded
    #[error("Memory limit exceeded: {current} used, {limit} allowed")]
    MemoryLimitExceeded {
        /// Current memory usage
        current: String,
        /// Configured memory limit
        limit: String,
    },

    /// Multiple errors accumulated during batch operations
    #[error("Multiple errors occurred: {}", format_errors(.0))]
    Multiple(Vec<Error>),
}

/// Result type alias for TableTools operations
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Create a new parse error
    pub fn parse<S: Into<String>>(message: S) -> Self {
        Error::Parse {
            message: message.into(),
            source: None,
        }
    }

    /// Parse an object_store::Error into a more specific Error
    pub fn from_object_store(error: object_store::Error, path: &str) -> Self {
        let error_str = error.to_string();
        let error_str_lower = error_str.to_lowercase();

        // Extract provider from path
        let provider = if path.starts_with("s3://") {
            "S3"
        } else if path.starts_with("gs://") || path.starts_with("gcs://") {
            "GCS"
        } else if path.starts_with("az://") || path.starts_with("azure://") {
            "Azure"
        } else {
            "CloudStorage"
        };

        // Parse HTTP status codes and error codes from the error string
        let mut error_code = None;
        let mut http_status = None;

        // Try to extract HTTP status code
        if let Some(caps) = regex::Regex::new(r"HTTP (\d{3})")
            .ok()
            .and_then(|re| re.captures(&error_str))
            && let Some(status) = caps.get(1)
        {
            http_status = status.as_str().parse::<u16>().ok();
        }

        // Try to extract AWS error codes
        let aws_error_patterns = [
            ("AccessDenied", "AccessDenied"),
            ("NoSuchBucket", "NoSuchBucket"),
            ("NoSuchKey", "NoSuchKey"),
            ("SlowDown", "SlowDown"),
            ("RequestTimeTooSkewed", "RequestTimeTooSkewed"),
            ("SignatureDoesNotMatch", "SignatureDoesNotMatch"),
            ("InvalidAccessKeyId", "InvalidAccessKeyId"),
            ("InvalidToken", "InvalidToken"),
            ("ExpiredToken", "ExpiredToken"),
            ("TokenRefreshRequired", "TokenRefreshRequired"),
            ("BucketAlreadyExists", "BucketAlreadyExists"),
            ("BucketAlreadyOwnedByYou", "BucketAlreadyOwnedByYou"),
            ("InvalidBucketName", "InvalidBucketName"),
            ("InvalidRange", "InvalidRange"),
            ("KeyTooLong", "KeyTooLong"),
            ("MissingContentLength", "MissingContentLength"),
            ("MissingSecurityHeader", "MissingSecurityHeader"),
            ("RequestTimeout", "RequestTimeout"),
            ("ServiceUnavailable", "ServiceUnavailable"),
            ("Throttling", "Throttling"),
            ("RequestThrottled", "RequestThrottled"),
        ];

        for (pattern, code) in aws_error_patterns.iter() {
            if error_str.contains(pattern) {
                error_code = Some(code.to_string());
                break;
            }
        }

        // If no AWS error code found, try to extract generic error patterns
        if error_code.is_none() {
            if error_str_lower.contains("connection") || error_str_lower.contains("network") {
                error_code = Some("NetworkError".to_string());
            } else if error_str_lower.contains("timeout") || error_str_lower.contains("timed out") {
                error_code = Some("Timeout".to_string());
            } else if error_str_lower.contains("permission")
                || error_str_lower.contains("access denied")
            {
                error_code = Some("PermissionDenied".to_string());
            } else if error_str_lower.contains("not found") {
                error_code = Some("NotFound".to_string());
            } else if error_str_lower.contains("throttl") || error_str_lower.contains("slow down") {
                error_code = Some("Throttling".to_string());
            }
        }

        // Create appropriate error type based on the parsed information
        match (error_code.as_deref(), http_status) {
            (Some("AccessDenied"), _) | (_, Some(403)) => Error::AccessDenied {
                path: path.to_string(),
                message: format!("{}: {}", provider, error_str),
            },
            (Some("NoSuchBucket") | Some("NoSuchKey") | Some("NotFound"), Some(404)) => {
                Error::FileNotFound {
                    path: std::path::PathBuf::from(path),
                }
            }
            (
                Some("InvalidAccessKeyId")
                | Some("SignatureDoesNotMatch")
                | Some("ExpiredToken")
                | Some("InvalidToken"),
                _,
            ) => Error::AuthenticationFailed {
                provider: provider.to_string(),
                message: error_str,
            },
            (Some("Throttling") | Some("SlowDown") | Some("RequestThrottled"), Some(429)) => {
                Error::CloudStorage {
                    provider: provider.to_string(),
                    message: format!("Rate limited or throttled: {}", error_str),
                    error_code,
                    http_status,
                }
            }
            (_, Some(503) | Some(502) | Some(504)) => Error::Network {
                message: format!("{} service unavailable: {}", provider, error_str),
                source: Some(Box::new(error)),
            },
            (Some("NetworkError") | Some("Timeout"), _) => Error::Network {
                message: format!("{} network error: {}", provider, error_str),
                source: Some(Box::new(error)),
            },
            _ => Error::CloudStorage {
                provider: provider.to_string(),
                message: error_str,
                error_code,
                http_status,
            },
        }
    }

    /// Create a new parse error with a source
    pub fn parse_with_source<S: Into<String>, E>(message: S, source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Error::Parse {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Create a new network error
    pub fn network<S: Into<String>>(message: S) -> Self {
        Error::Network {
            message: message.into(),
            source: None,
        }
    }

    /// Create a new network error with a source
    pub fn network_with_source<S: Into<String>, E>(message: S, source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Error::Network {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Create a timeout error
    pub fn timeout<S: Into<String>>(operation: S, seconds: u64) -> Self {
        Error::Timeout {
            operation: operation.into(),
            seconds,
        }
    }

    /// Create a corrupted file error
    pub fn corrupted_file<P: Into<PathBuf>, S: Into<String>>(path: P, reason: S) -> Self {
        Error::CorruptedFile {
            path: path.into(),
            reason: reason.into(),
        }
    }

    /// Create a metadata error
    pub fn metadata<S: Into<String>>(message: S) -> Self {
        Error::Metadata {
            message: message.into(),
        }
    }

    /// Create a manifest error
    pub fn manifest<S: Into<String>>(message: S) -> Self {
        Error::Manifest {
            message: message.into(),
        }
    }

    /// Create a serialization error
    pub fn serialization<S: Into<String>>(message: S) -> Self {
        Error::Serialization {
            message: message.into(),
        }
    }

    /// Create a column not found error
    pub fn column_not_found<S: Into<String>>(column: S) -> Self {
        Error::ColumnNotFound {
            column: column.into(),
        }
    }

    /// Create a snapshot not found error
    pub fn snapshot_not_found(snapshot_id: i64) -> Self {
        Error::SnapshotNotFound { snapshot_id }
    }

    /// Create a table not found error
    pub fn table_not_found<S: Into<String>>(path: S) -> Self {
        Error::TableNotFound { path: path.into() }
    }

    fn file_not_found_suggestion(path: &Path) -> String {
        format!(
            "File not found: {}\n\nPossible solutions:\n  1. Check the file path is correct\n  2. Verify you have read permissions\n  3. If on cloud storage, ensure credentials are configured",
            path.display()
        )
    }

    fn permission_denied_suggestion(path: &Path) -> String {
        format!(
            "Permission denied: {}\n\nTry:\n  1. Check file permissions: ls -l {}\n  2. Verify you have the necessary access rights",
            path.display(),
            path.display()
        )
    }

    fn corrupted_file_suggestion(path: &Path, reason: &str) -> String {
        format!(
            "Corrupted file: {}\n\nReason: {}\n\nThis usually means:\n  1. File transfer was interrupted\n  2. File is not actually in the expected format\n  3. Disk corruption\n\nTry:\n  1. Re-download or regenerate the file\n  2. Run: icetable validate {}\n  3. Check file type: file {}",
            path.display(),
            reason,
            path.display(),
            path.display()
        )
    }

    fn authentication_failed_suggestion(provider: &str, message: &str) -> String {
        let suggestion = match provider.to_lowercase().as_str() {
            "aws" | "s3" => {
                "Try:\n  1. aws configure\n  2. Check AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY environment variables\n  3. Verify AWS_REGION is set correctly"
            }
            "gcp" | "gcs" => {
                "Try:\n  1. gcloud auth application-default login\n  2. Check GOOGLE_APPLICATION_CREDENTIALS environment variable\n  3. Verify the service account has storage.objectAdmin role"
            }
            "azure" => {
                "Try:\n  1. az login\n  2. Check AZURE_STORAGE_ACCOUNT and AZURE_STORAGE_KEY environment variables\n  3. For managed identity: export AZURE_STORAGE_USE_AZURE_AD=true"
            }
            _ => "Check your cloud credentials configuration",
        };
        format!(
            "Authentication failed for {}: {}\n\n{}",
            provider, message, suggestion
        )
    }

    fn cloud_storage_suggestion(
        provider: &str,
        message: &str,
        error_code: Option<&str>,
        http_status: Option<u16>,
    ) -> String {
        let mut suggestions = Vec::new();

        // Add provider-specific suggestions
        match provider.to_lowercase().as_str() {
            "s3" => {
                suggestions.push("• Check S3 bucket permissions and policies");
                suggestions.push("• Verify bucket exists and is in the correct region");
                suggestions.push("• For MinIO: check AWS_ENDPOINT_URL is set correctly");
            }
            "gcs" => {
                suggestions.push("• Check GCS bucket permissions and IAM roles");
                suggestions.push("• Verify bucket exists in the correct project");
                suggestions.push("• Check if requester pays is enabled on the bucket");
            }
            "azure" => {
                suggestions.push("• Check Azure Storage account permissions");
                suggestions.push("• Verify container exists in the storage account");
                suggestions.push("• Check if firewall rules allow your IP address");
            }
            _ => {
                suggestions.push("• Check cloud provider credentials and permissions");
                suggestions.push("• Verify the resource exists and is accessible");
            }
        }

        // Add error code specific suggestions
        if let Some(code) = error_code {
            match code {
                "Throttling" | "SlowDown" | "RequestThrottled" => {
                    suggestions.push("• This is a rate limiting error - try again later");
                    suggestions
                        .push("• Consider implementing exponential backoff in your application");
                    suggestions.push("• Check if you're exceeding request quotas");
                }
                "NetworkError" | "Timeout" => {
                    suggestions.push("• Check your network connection");
                    suggestions.push("• Verify firewall allows outbound connections");
                    suggestions.push("• Try increasing timeout settings");
                }
                "NoSuchBucket" | "NoSuchKey" => {
                    suggestions.push("• Verify the bucket/container name is correct");
                    suggestions.push("• Check if the object/key exists");
                    suggestions.push("• Ensure you have list permissions on the bucket");
                }
                _ => {}
            }
        }

        // Add HTTP status specific suggestions
        if let Some(status) = http_status {
            match status {
                403 => suggestions.push("• Check IAM permissions or bucket policies"),
                404 => suggestions.push("• Resource not found - verify the path is correct"),
                429 => suggestions.push("• Too many requests - implement rate limiting"),
                500..=599 => suggestions.push("• Cloud provider service issue - try again later"),
                _ => {}
            }
        }

        let suggestions_str = suggestions.join("\n");
        format!(
            "{} error: {}{}{}\n\nTroubleshooting:\n{}",
            provider,
            message,
            error_code
                .map(|c| format!(" (Error code: {})", c))
                .unwrap_or_default(),
            http_status
                .map(|s| format!(" (HTTP: {})", s))
                .unwrap_or_default(),
            suggestions_str
        )
    }

    fn timeout_suggestion(operation: &str, seconds: u64) -> String {
        format!(
            "Operation timed out after {}s: {}\n\nThis may be due to:\n  1. Slow network connection\n  2. Large file size\n  3. Cloud storage throttling\n\nTry:\n  1. Check your internet connection\n  2. Increase timeout with --timeout flag\n  3. Use --quick for faster validation",
            seconds, operation
        )
    }

    fn conflict_suggestion(message: &str) -> String {
        format!(
            "Conflict detected: {}\n\nAnother process modified the table while this operation was in progress.\n\nTo resolve:\n  1. Retry the operation - it will use the latest table state\n  2. If using automation, implement retry logic with backoff\n  3. Consider using table locks in high-concurrency scenarios",
            message
        )
    }

    /// Get a user-friendly error message with suggestions
    pub fn user_message(&self) -> String {
        match self {
            Error::FileNotFound { path } => Self::file_not_found_suggestion(path),
            Error::PermissionDenied { path } => Self::permission_denied_suggestion(path),
            Error::CorruptedFile { path, reason } => Self::corrupted_file_suggestion(path, reason),
            Error::AuthenticationFailed { provider, message } => {
                Self::authentication_failed_suggestion(provider, message)
            }
            Error::Timeout { operation, seconds } => Self::timeout_suggestion(operation, *seconds),
            Error::Conflict(message) => Self::conflict_suggestion(message),
            Error::CloudStorage {
                provider,
                message,
                error_code,
                http_status,
            } => Self::cloud_storage_suggestion(
                provider,
                message,
                error_code.as_deref(),
                *http_status,
            ),
            Error::AccessDenied { path, message } => {
                format!(
                    "Access denied to {}: {}\n\nCheck:\n  1. IAM permissions or bucket policies\n  2. Network firewall rules\n  3. Resource exists and is accessible",
                    path, message
                )
            }
            Error::Network { message, .. } => {
                format!(
                    "Network error: {}\n\nPossible causes:\n  1. Internet connection issues\n  2. Firewall blocking connections\n  3. Cloud provider service disruption\n  4. DNS resolution problems",
                    message
                )
            }
            _ => self.to_string(),
        }
    }

    /// Check if this is a recoverable error
    pub fn is_recoverable(&self) -> bool {
        matches!(self, Error::Network { .. } | Error::Timeout { .. })
    }
}

/// Format multiple errors for display
fn format_errors(errors: &[Error]) -> String {
    errors
        .iter()
        .enumerate()
        .map(|(i, e)| format!("\n  {}. {}", i + 1, e))
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = Error::FileNotFound {
            path: PathBuf::from("/tmp/test.parquet"),
        };
        assert!(err.to_string().contains("File not found"));
    }

    #[test]
    fn test_user_message() {
        let err = Error::FileNotFound {
            path: PathBuf::from("/tmp/test.parquet"),
        };
        let msg = err.user_message();
        assert!(msg.contains("Possible solutions"));
    }

    #[test]
    fn test_recoverable() {
        let err = Error::timeout("read file", 30);
        assert!(err.is_recoverable());

        let err = Error::FileNotFound {
            path: PathBuf::from("/tmp/test.parquet"),
        };
        assert!(!err.is_recoverable());
    }
}
