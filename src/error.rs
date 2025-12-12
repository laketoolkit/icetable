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

    /// Storage/filesystem operation errors
    #[error("Storage error: {message}")]
    Storage {
        /// The error message describing what failed
        message: String,
    },

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

    /// Catalog required but not provided
    #[error("Catalog required for table reference: {table_ref}")]
    CatalogRequired {
        /// The table reference that requires a catalog
        table_ref: String,
    },

    /// Write operations require a catalog
    #[error(
        "Write operations require a catalog.\n  → icetable config add-catalog <name> --uri <URL>"
    )]
    CatalogRequiredForWrite {
        /// The write operation that was attempted
        operation: String,
    },

    /// Failed to load Iceberg table
    #[error("Failed to load Iceberg table at {path}: {source}")]
    IcebergLoad {
        /// The path to the Iceberg table
        path: String,
        /// The underlying error source
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Failed to load table from catalog
    #[error("Failed to load table {table_ref} from catalog: {source}")]
    CatalogLoad {
        /// The table reference
        table_ref: String,
        /// The underlying error source
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Invalid catalog reference format
    #[error("Invalid catalog reference '{ref_str}': {reason}")]
    InvalidCatalogRef {
        /// The invalid catalog reference string
        ref_str: String,
        /// The reason it's invalid
        reason: String,
    },

    /// Catalog configuration error
    #[error("Catalog configuration error: {source}")]
    CatalogConfig {
        /// The underlying error source
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Failed to build catalog
    #[error("Failed to build catalog: {source}")]
    CatalogBuild {
        /// The underlying error source
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Unsupported catalog type
    #[error("Unsupported catalog type: {catalog_type}")]
    UnsupportedCatalog {
        /// The unsupported catalog type
        catalog_type: String,
    },

    /// Failed to create Iceberg table
    #[error("Failed to create Iceberg table at {path}: {source}")]
    IcebergCreate {
        /// The path where table creation failed
        path: String,
        /// The underlying error source
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Iceberg scan operation failed
    #[error("Iceberg scan operation failed: {source}")]
    IcebergScan {
        /// The underlying error source
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Missing required argument
    #[error("Missing required argument '{argument}': {description}")]
    MissingArgument {
        /// The name of the missing argument
        argument: String,
        /// Description of why the argument is required
        description: String,
    },

    // =========================================================================
    // CLI-specific errors
    // =========================================================================
    /// No catalog configured or specified
    #[error("No catalog specified. Use 'icetable config use <catalog>' or specify -c <catalog>")]
    NoCatalog,

    /// Catalog not found in configuration
    #[error("Catalog '{name}' not found in configuration")]
    CatalogNotFound {
        /// The catalog name that was not found
        name: String,
    },

    /// No namespace specified when required
    #[error(
        "No namespace specified. Use 'icetable config use <catalog> -n <namespace>' or specify -n <namespace>"
    )]
    NoNamespace,

    /// Namespace not found in catalog
    #[error("Namespace '{name}' not found in catalog")]
    NamespaceNotFound {
        /// The namespace name that was not found
        name: String,
    },

    /// No table specified when required
    #[error(
        "No table specified. Use 'icetable config use <catalog> -n <namespace> -t <table>' or specify -t <table>"
    )]
    NoTable,

    /// Invalid namespace format
    #[error("Invalid namespace '{value}': {reason}")]
    InvalidNamespace {
        /// The invalid namespace value
        value: String,
        /// Why it's invalid
        reason: String,
    },

    /// Catalog operation failed
    #[error("Catalog operation failed: {message}")]
    CatalogOperation {
        /// Description of what failed
        message: String,
    },
}

/// Result type alias for TableTools operations
pub type Result<T> = std::result::Result<T, Error>;

/// Format multiple errors into a single string
fn format_errors(errors: &[Error]) -> String {
    if errors.is_empty() {
        return "No errors".to_string();
    }

    let mut result = String::new();
    for (i, error) in errors.iter().enumerate() {
        result.push_str(&format!("{}. {}\n", i + 1, error));
    }
    result
}

impl Error {
    /// Create a new parse error
    pub fn parse<S: Into<String>>(message: S) -> Self {
        Error::Parse {
            message: message.into(),
            source: None,
        }
    }

    /// Get a user-friendly error message with suggestions
    pub fn user_message(&self) -> String {
        let base_message = self.to_string();

        let suggestions = match self {
            Error::FileNotFound { path } => {
                let path_str = path.display();
                Some(format!(
                    "\n\nPossible solutions:\n\
                     - Check if the path '{}' exists\n\
                     - Verify you have read permissions\n\
                     - Ensure the table path is correct",
                    path_str
                ))
            }
            Error::PermissionDenied { .. } => Some(
                "\n\nPossible solutions:\n\
                 - Check file/directory permissions\n\
                 - Run with appropriate privileges\n\
                 - Verify cloud credentials are configured"
                    .to_string(),
            ),
            Error::Configuration { .. } => Some(
                "\n\nPossible solutions:\n\
                 - Check environment variables (AWS_*, GOOGLE_*, AZURE_*)\n\
                 - Verify configuration file syntax\n\
                 - See documentation for required settings"
                    .to_string(),
            ),
            Error::Network { .. } => Some(
                "\n\nPossible solutions:\n\
                 - Check network connectivity\n\
                 - Verify endpoint URL is correct\n\
                 - Check firewall settings"
                    .to_string(),
            ),
            _ => None,
        };

        match suggestions {
            Some(s) => format!("{}{}", base_message, s),
            None => base_message,
        }
    }

    /// Check if the error is recoverable (can be retried)
    pub fn is_recoverable(&self) -> bool {
        matches!(self, Error::Timeout { .. } | Error::Network { .. })
    }
}

impl From<iceberg::Error> for Error {
    fn from(error: iceberg::Error) -> Self {
        Error::Parse {
            message: format!("Iceberg error: {}", error),
            source: Some(Box::new(error)),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Error::Parse {
            message: format!("JSON parsing error: {}", error),
            source: Some(Box::new(error)),
        }
    }
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
        let err = Error::Timeout {
            operation: "read file".to_string(),
            seconds: 30,
        };
        assert!(err.is_recoverable());

        let err = Error::FileNotFound {
            path: PathBuf::from("/tmp/test.parquet"),
        };
        assert!(!err.is_recoverable());
    }
}
