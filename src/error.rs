//! Error types for TableTools
//!
//! Defines a comprehensive error hierarchy following the specification in section 7.4.
//! All errors implement std::error::Error and are designed to provide actionable
//! error messages to users.

use std::path::PathBuf;
use thiserror::Error;

/// The main error type for icetable operations
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// IO error from std
    #[error("{0}")]
    Io(#[from] std::io::Error),

    /// File not found
    #[error("file not found: {}", path.display())]
    FileNotFound {
        /// Path to the missing file
        path: PathBuf,
    },

    /// Permission denied
    #[error("permission denied: {}", path.display())]
    PermissionDenied {
        /// Path that couldn't be accessed
        path: PathBuf,
    },

    /// Parse/deserialization error
    #[error("{message}")]
    Parse {
        /// Error description
        message: String,
        /// Underlying error
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Corrupted file
    #[error("corrupted: {} ({message})", path.display())]
    CorruptedFile {
        /// Path to corrupted file
        path: PathBuf,
        /// Why it's corrupted
        message: String,
    },

    /// Invalid format
    #[error("invalid format: {message}")]
    InvalidFormat {
        /// Error description
        message: String,
    },

    /// Schema validation failed
    #[error("invalid schema: {message}")]
    SchemaValidation {
        /// Error description
        message: String,
    },

    /// Data validation failed
    #[error("invalid data: {message}")]
    DataValidation {
        /// Error description
        message: String,
    },

    /// Type conversion error
    #[error("type error: {message}")]
    TypeConversion {
        /// Error description
        message: String,
    },

    /// Feature not supported
    #[error("not supported: {feature}")]
    UnsupportedFeature {
        /// Unsupported feature name
        feature: String,
    },

    /// Authentication failed
    #[error("{provider} auth failed: {message}")]
    AuthenticationFailed {
        /// Provider name (Polaris, AWS, etc.)
        provider: String,
        /// Error description
        message: String,
    },

    /// Network error
    #[error("network error: {message}")]
    Network {
        /// Error description
        message: String,
        /// Underlying error
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Operation timed out
    #[error("timeout after {seconds}s: {operation}")]
    Timeout {
        /// Operation that timed out
        operation: String,
        /// Timeout duration
        seconds: u64,
    },

    /// Access denied
    #[error("access denied: {message}")]
    AccessDenied {
        /// Resource path
        path: String,
        /// Error description
        message: String,
    },

    /// Configuration error
    #[error("{message}")]
    Configuration {
        /// Error description
        message: String,
    },

    /// Arrow error
    #[error("{0}")]
    Arrow(#[from] arrow::error::ArrowError),

    /// Parquet error
    #[error("{0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    /// Object store error
    #[error("{0}")]
    ObjectStore(#[from] object_store::Error),

    /// Cloud storage error
    #[error("{provider}: {message}")]
    CloudStorage {
        /// Provider name
        provider: String,
        /// Error description
        message: String,
        /// Provider error code
        error_code: Option<String>,
        /// HTTP status code
        http_status: Option<u16>,
    },

    /// Metadata error (generic - prefer specific variants below)
    #[error("{message}")]
    Metadata {
        /// Error description
        message: String,
    },

    /// Error loading table metadata from storage
    #[error("failed to load metadata from {path}: {message}")]
    MetadataLoad {
        /// Path where metadata was being loaded from
        path: String,
        /// Error description
        message: String,
    },

    /// Error building table metadata (builder pattern failures)
    #[error("failed to build metadata: {message}")]
    MetadataBuild {
        /// Error description
        message: String,
    },

    /// Error executing table scan
    #[error("scan failed: {message}")]
    TableScan {
        /// Error description
        message: String,
    },

    /// Manifest error
    #[error("{message}")]
    Manifest {
        /// Error description
        message: String,
    },

    /// Serialization error
    #[error("{message}")]
    Serialization {
        /// Error description
        message: String,
    },

    /// Storage error
    #[error("{message}")]
    Storage {
        /// Error description
        message: String,
    },

    /// Column not found
    #[error("column '{column}' not found")]
    ColumnNotFound {
        /// Column name
        column: String,
    },

    /// Snapshot not found
    #[error("snapshot {snapshot_id} not found")]
    SnapshotNotFound {
        /// Snapshot ID
        snapshot_id: i64,
    },

    /// Table not found
    #[error("table not found: {path}")]
    TableNotFound {
        /// Table path
        path: String,
    },

    /// Table already exists
    #[error("table exists: {path}")]
    TableAlreadyExists {
        /// Table path
        path: String,
    },

    /// Namespace already exists
    #[error("namespace exists: {name}")]
    NamespaceAlreadyExists {
        /// Namespace name
        name: String,
    },

    /// Branch not found
    #[error("branch '{name}' not found")]
    BranchNotFound {
        /// Branch name
        name: String,
    },

    /// Tag not found
    #[error("tag '{name}' not found")]
    TagNotFound {
        /// Tag name
        name: String,
    },

    /// Concurrent modification conflict
    #[error("{0}")]
    Conflict(String),

    /// Operation cancelled
    #[error("cancelled")]
    Cancelled,

    /// Memory limit exceeded
    #[error("memory limit: {current} used, {limit} allowed")]
    MemoryLimitExceeded {
        /// Current memory usage
        current: String,
        /// Configured limit
        limit: String,
    },

    /// Multiple errors
    #[error("{}", format_errors(.0))]
    Multiple(Vec<Error>),

    /// Catalog required for operation
    #[error("catalog required for '{table_ref}'")]
    CatalogRequired {
        /// Table reference
        table_ref: String,
    },

    /// Write requires catalog
    #[error("write requires catalog\n  → icetable config add <name> --uri <URL>")]
    CatalogRequiredForWrite {
        /// Write operation
        operation: String,
    },

    /// Failed to load Iceberg table
    #[error("failed to load '{path}': {source}")]
    IcebergLoad {
        /// Table path
        path: String,
        /// Underlying error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Failed to load from catalog
    #[error("failed to load '{table_ref}': {source}")]
    CatalogLoad {
        /// Table reference
        table_ref: String,
        /// Underlying error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Invalid catalog reference
    #[error("invalid reference '{ref_str}': {message}")]
    InvalidCatalogRef {
        /// Reference string
        ref_str: String,
        /// Why it's invalid
        message: String,
    },

    /// Catalog configuration error
    #[error("{source}")]
    CatalogConfig {
        /// Underlying error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Failed to build catalog
    #[error("catalog build failed: {source}")]
    CatalogBuild {
        /// Underlying error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Unsupported catalog type
    #[error("unsupported catalog: {catalog_type}")]
    UnsupportedCatalog {
        /// Catalog type
        catalog_type: String,
    },

    /// Failed to create Iceberg table
    #[error("failed to create '{path}': {source}")]
    IcebergCreate {
        /// Table path
        path: String,
        /// Underlying error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Scan failed
    #[error("scan failed: {source}")]
    IcebergScan {
        /// Underlying error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Missing required argument
    #[error("missing {argument}: {description}")]
    MissingArgument {
        /// Argument name
        argument: String,
        /// Why it's required
        description: String,
    },

    /// No catalog specified
    #[error("No catalog configured. Run: icetable config use <catalog>@<warehouse>")]
    NoCatalog,

    /// Catalog not found
    #[error("catalog '{name}' not found")]
    CatalogNotFound {
        /// Catalog name
        name: String,
    },

    /// No namespace specified
    #[error("No namespace specified. Use -n <namespace> or include in context")]
    NoNamespace,

    /// Namespace not found
    #[error("namespace '{name}' not found")]
    NamespaceNotFound {
        /// Namespace name
        name: String,
    },

    /// No table specified
    #[error("No table specified. Use -t <table> or include in context")]
    NoTable,

    /// Invalid namespace
    #[error("invalid namespace '{value}': {message}")]
    InvalidNamespace {
        /// Namespace value
        value: String,
        /// Why it's invalid
        message: String,
    },

    /// Catalog operation error
    #[error("{message}")]
    CatalogOperation {
        /// Error description
        message: String,
    },

    /// Invalid snapshot reference
    #[error("invalid snapshot '{reference}': {message}")]
    InvalidSnapshotRef {
        /// Reference string
        reference: String,
        /// Why it's invalid
        message: String,
    },

    /// Invalid filter expression
    #[error("invalid filter '{expression}': {message}")]
    InvalidFilterExpression {
        /// Expression string
        expression: String,
        /// Why it's invalid
        message: String,
    },

    /// Async operation failed with context
    #[error("{operation} failed: {message}")]
    AsyncOperation {
        /// Operation that failed (e.g., "loading table", "committing snapshot")
        operation: String,
        /// Error description
        message: String,
        /// Underlying error
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Error with additional context
    #[error("{context}: {source}")]
    WithContext {
        /// Additional context about the operation
        context: String,
        /// Underlying error
        #[source]
        source: Box<Error>,
    },
}

/// Result type alias for TableTools operations
pub type Result<T> = std::result::Result<T, Error>;

/// Extension trait for adding context to Results
pub trait ResultExt<T> {
    /// Add context to an error result
    ///
    /// # Example
    /// ```ignore
    /// use icetable::error::ResultExt;
    ///
    /// fn load_table(path: &str) -> Result<Table> {
    ///     load_metadata(path).context("loading table metadata")?;
    ///     // ...
    /// }
    /// ```
    fn context<S: Into<String>>(self, context: S) -> Result<T>;

    /// Add lazy context to an error result (context computed only on error)
    fn with_context<S: Into<String>, F: FnOnce() -> S>(self, f: F) -> Result<T>;
}

impl<T> ResultExt<T> for Result<T> {
    fn context<S: Into<String>>(self, context: S) -> Result<T> {
        self.map_err(|e| e.with_context(context))
    }

    fn with_context<S: Into<String>, F: FnOnce() -> S>(self, f: F) -> Result<T> {
        self.map_err(|e| e.with_context(f()))
    }
}

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

    /// Create a new async operation error
    pub fn async_op<S: Into<String>, M: Into<String>>(operation: S, message: M) -> Self {
        Error::AsyncOperation {
            operation: operation.into(),
            message: message.into(),
            source: None,
        }
    }

    /// Create a new async operation error with a source
    pub fn async_op_with_source<S, M, E>(operation: S, message: M, source: E) -> Self
    where
        S: Into<String>,
        M: Into<String>,
        E: std::error::Error + Send + Sync + 'static,
    {
        Error::AsyncOperation {
            operation: operation.into(),
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Wrap this error with additional context
    ///
    /// # Example
    /// ```ignore
    /// let err = Error::Metadata { message: "bad data".to_string() };
    /// let contextual = err.with_context("loading snapshot 123");
    /// assert!(contextual.to_string().contains("loading snapshot 123"));
    /// ```
    pub fn with_context<S: Into<String>>(self, context: S) -> Self {
        Error::WithContext {
            context: context.into(),
            source: Box::new(self),
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
        use iceberg::ErrorKind;

        let message = error.to_string();
        match error.kind() {
            ErrorKind::DataInvalid => Error::DataValidation { message },
            ErrorKind::FeatureUnsupported => Error::UnsupportedFeature { feature: message },
            ErrorKind::TableNotFound => Error::TableNotFound { path: message },
            ErrorKind::NamespaceNotFound => Error::NamespaceNotFound { name: message },
            ErrorKind::TableAlreadyExists => Error::TableAlreadyExists { path: message },
            ErrorKind::NamespaceAlreadyExists => Error::NamespaceAlreadyExists { name: message },
            ErrorKind::CatalogCommitConflicts => Error::Conflict(message),
            ErrorKind::PreconditionFailed => {
                Error::Conflict(format!("Precondition failed: {}", message))
            }
            ErrorKind::Unexpected => Error::Metadata { message },
            // Handle future ErrorKind variants
            _ => Error::Metadata { message },
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
        assert!(err.to_string().contains("file not found"));
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
            operation: "read".to_string(),
            seconds: 30,
        };
        assert!(err.is_recoverable());

        let err = Error::FileNotFound {
            path: PathBuf::from("/tmp/test.parquet"),
        };
        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_with_context() {
        let err = Error::Metadata {
            message: "bad data".to_string(),
        };
        let contextual = err.with_context("loading snapshot 123");
        let msg = contextual.to_string();
        assert!(msg.contains("loading snapshot 123"));
        assert!(msg.contains("bad data"));
    }

    #[test]
    fn test_async_op_error() {
        let err = Error::async_op("loading table", "connection refused");
        let msg = err.to_string();
        assert!(msg.contains("loading table failed"));
        assert!(msg.contains("connection refused"));
    }

    #[test]
    fn test_async_op_with_source() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = Error::async_op_with_source("reading metadata", "failed to read", io_err);
        let msg = err.to_string();
        assert!(msg.contains("reading metadata failed"));
        assert!(msg.contains("failed to read"));
    }

    #[test]
    fn test_result_context_extension() {
        fn failing_operation() -> Result<()> {
            Err(Error::Metadata {
                message: "invalid format".to_string(),
            })
        }

        let result = failing_operation().context("processing table foo");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("processing table foo"));
        assert!(err_msg.contains("invalid format"));
    }

    #[test]
    fn test_result_with_context_lazy() {
        fn failing_operation() -> Result<()> {
            Err(Error::Metadata {
                message: "invalid".to_string(),
            })
        }

        let table_name = "my_table";
        let result =
            failing_operation().with_context(|| format!("processing table {}", table_name));

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("processing table my_table"));
    }
}
