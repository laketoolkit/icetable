//! Error types for TableTools
//!
//! Defines a comprehensive error hierarchy following the specification in section 7.4.
//! All errors implement std::error::Error and are designed to provide actionable
//! error messages to users.

use std::path::PathBuf;
use thiserror::Error;

/// The main error type for TableTools operations
#[derive(Error, Debug)]
pub enum Error {
    /// Errors related to I/O operations
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// File not found errors with context
    #[error("File not found: {path}")]
    FileNotFound { path: PathBuf },

    /// Permission denied errors
    #[error("Permission denied accessing: {path}")]
    PermissionDenied { path: PathBuf },

    /// Errors parsing or reading table formats
    #[error("Parse error: {message}")]
    Parse {
        message: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Corrupted file errors
    #[error("Corrupted file: {path} - {reason}")]
    CorruptedFile { path: PathBuf, reason: String },

    /// Invalid format errors
    #[error("Invalid format: {message}")]
    InvalidFormat { message: String },

    /// Schema validation errors
    #[error("Schema validation failed: {message}")]
    SchemaValidation { message: String },

    /// Data validation errors
    #[error("Data validation failed: {message}")]
    DataValidation { message: String },

    /// Type conversion errors
    #[error("Type conversion error: {message}")]
    TypeConversion { message: String },

    /// Unsupported feature errors
    #[error("Unsupported feature: {feature}")]
    UnsupportedFeature { feature: String },

    /// Cloud storage authentication errors
    #[error("Authentication failed for {provider}: {message}")]
    AuthenticationFailed { provider: String, message: String },

    /// Cloud storage network errors
    #[error("Network error: {message}")]
    Network {
        message: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Timeout errors
    #[error("Operation timed out after {seconds}s: {operation}")]
    Timeout { operation: String, seconds: u64 },

    /// Cloud storage access denied
    #[error("Access denied to {path}: {message}")]
    AccessDenied { path: String, message: String },

    /// Configuration errors
    #[error("Configuration error: {message}")]
    Configuration { message: String },

    /// Arrow-specific errors
    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    /// Parquet-specific errors
    #[error("Parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    /// DataFusion SQL errors
    #[error("SQL error: {0}")]
    DataFusion(#[from] datafusion::error::DataFusionError),

    /// Object store errors (S3, GCS, Azure)
    #[error("Storage error: {0}")]
    ObjectStore(#[from] object_store::Error),

    /// General errors with context
    #[error("{0}")]
    General(String),

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

    /// Get a user-friendly error message with suggestions
    pub fn user_message(&self) -> String {
        match self {
            Error::FileNotFound { path } => {
                format!(
                    "File not found: {}\n\nPossible solutions:\n  1. Check the file path is correct\n  2. Verify you have read permissions\n  3. If on cloud storage, ensure credentials are configured",
                    path.display()
                )
            }
            Error::PermissionDenied { path } => {
                format!(
                    "Permission denied: {}\n\nTry:\n  1. Check file permissions: ls -l {}\n  2. Verify you have the necessary access rights",
                    path.display(),
                    path.display()
                )
            }
            Error::CorruptedFile { path, reason } => {
                format!(
                    "Corrupted file: {}\n\nReason: {}\n\nThis usually means:\n  1. File transfer was interrupted\n  2. File is not actually in the expected format\n  3. Disk corruption\n\nTry:\n  1. Re-download or regenerate the file\n  2. Run: tabletools validate {}\n  3. Check file type: file {}",
                    path.display(),
                    reason,
                    path.display(),
                    path.display()
                )
            }
            Error::AuthenticationFailed { provider, message } => {
                let suggestion = match provider.as_str() {
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
            Error::Timeout { operation, seconds } => {
                format!(
                    "Operation timed out after {}s: {}\n\nThis may be due to:\n  1. Slow network connection\n  2. Large file size\n  3. Cloud storage throttling\n\nTry:\n  1. Check your internet connection\n  2. Increase timeout with --timeout flag\n  3. Use --quick for faster validation",
                    seconds, operation
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
