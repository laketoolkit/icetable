//! Error types for TableTools
//!
//! Defines a comprehensive error hierarchy following the specification in section 7.4.
//! All errors implement std::error::Error and are designed to provide actionable
//! error messages to users.

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

    /// General errors with context
    #[error("{0}")]
    General(String),

    /// Concurrent modification conflict (optimistic concurrency)
    #[error("Conflict: {0}")]
    Conflict(String),

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
        let suggestion = match provider {
            "aws" | "s3" => "Try: aws configure",
            "gcp" | "gcs" => "Try: gcloud auth application-default login",
            "azure" => "Try: az login",
            _ => "Check your cloud credentials",
        };
        format!(
            "Authentication failed for {}: {}\n\n{}",
            provider, message, suggestion
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
