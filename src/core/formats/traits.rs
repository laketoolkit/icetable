//! Core traits for format handlers
//!
//! Defines the FormatHandler trait that all format implementations must satisfy.
//! This provides a unified interface for working with different table formats
//! (Parquet, Arrow, Iceberg, Delta Lake, etc.)

use arrow::datatypes::Schema;
use arrow::record_batch::RecordBatch;
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

use crate::error::Result;

/// Metadata about a table file
#[derive(Debug, Clone)]
pub struct FileMetadata {
    /// Total number of rows (may be estimated)
    pub num_rows: Option<i64>,

    /// Compressed file size in bytes
    pub compressed_size: Option<u64>,

    /// Uncompressed file size in bytes
    pub uncompressed_size: Option<u64>,

    /// Compression codec used
    pub compression: Option<String>,

    /// Format version
    pub format_version: Option<String>,

    /// Created timestamp
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,

    /// Additional format-specific metadata
    pub metadata: std::collections::HashMap<String, String>,
}

/// Statistics for a single column
#[derive(Debug, Clone)]
pub struct ColumnStats {
    /// Column name
    pub name: String,

    /// Number of null values
    pub null_count: Option<i64>,

    /// Number of distinct values
    pub distinct_count: Option<i64>,

    /// Minimum value (as string for display)
    pub min_value: Option<String>,

    /// Maximum value (as string for display)
    pub max_value: Option<String>,

    /// Mean value (for numeric columns)
    pub mean: Option<f64>,

    /// Standard deviation (for numeric columns)
    pub std_dev: Option<f64>,
}

/// Options for reading data
#[derive(Debug, Clone, Default)]
pub struct ReadOptions {
    /// Columns to read (None = all columns)
    pub columns: Option<Vec<String>>,

    /// Row offset to start reading from
    pub offset: Option<usize>,

    /// Maximum number of rows to read
    pub limit: Option<usize>,

    /// Use sampling instead of sequential reading
    pub sample: bool,

    /// Batch size for reading
    pub batch_size: Option<usize>,
}

/// Options for writing data
#[derive(Debug, Clone, Default)]
pub struct WriteOptions {
    /// Compression codec to use
    pub compression: Option<String>,

    /// Row group size for Parquet
    pub row_group_size: Option<usize>,

    /// Dictionary encoding configuration
    pub enable_dictionary: bool,

    /// Statistics generation
    pub enable_statistics: bool,

    /// Overwrite existing file
    pub overwrite: bool,
}

/// Validation report for a file
#[derive(Debug, Clone)]
pub struct ValidationReport {
    /// Overall validation status
    pub is_valid: bool,

    /// List of errors found
    pub errors: Vec<String>,

    /// List of warnings
    pub warnings: Vec<String>,

    /// Recommendations for improvement
    pub recommendations: Vec<String>,
}

impl ValidationReport {
    /// Create a new successful validation report
    pub fn success() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            recommendations: Vec::new(),
        }
    }

    /// Create a failed validation report
    pub fn failed(errors: Vec<String>) -> Self {
        Self {
            is_valid: false,
            errors,
            warnings: Vec::new(),
            recommendations: Vec::new(),
        }
    }

    /// Add a warning
    pub fn add_warning(&mut self, warning: String) {
        self.warnings.push(warning);
    }

    /// Add a recommendation
    pub fn add_recommendation(&mut self, recommendation: String) {
        self.recommendations.push(recommendation);
    }
}

/// The core trait that all format handlers must implement
///
/// This trait provides a unified interface for reading, writing, and validating
/// different table formats. Implementations should be efficient and support
/// streaming operations where possible.
#[async_trait]
pub trait FormatHandler: Send + Sync {
    /// Detect if this handler can process the given path
    ///
    /// This should be a fast check based on file extension, magic bytes, or
    /// the presence of specific metadata files.
    async fn can_handle(&self, path: &Path) -> Result<bool>;

    /// Get the format name (e.g., "Parquet", "Arrow IPC", "Iceberg")
    fn format_name(&self) -> &str;

    /// Read the schema without loading data
    ///
    /// This should be fast and not require reading the entire file.
    async fn read_schema(&self) -> Result<Arc<Schema>>;

    /// Read metadata about the file
    ///
    /// This should extract metadata without reading the actual data rows.
    async fn read_metadata(&self) -> Result<FileMetadata>;

    /// Read a batch of data
    ///
    /// This supports streaming and pagination through the offset/limit options.
    async fn read_batch(&self, options: &ReadOptions) -> Result<RecordBatch>;

    /// Read multiple batches as a stream
    ///
    /// This is more efficient for large files as it doesn't load everything
    /// into memory at once.
    async fn read_batches(
        &self,
        options: &ReadOptions,
    ) -> Result<Vec<RecordBatch>>;

    /// Get statistics for all columns
    ///
    /// This should use format-native statistics when available (e.g., Parquet
    /// column statistics) to avoid reading the actual data.
    async fn read_statistics(&self) -> Result<Vec<ColumnStats>>;

    /// Validate the file structure and data
    ///
    /// This performs comprehensive validation including:
    /// - File structure integrity
    /// - Schema validity
    /// - Data quality checks (if quick=false)
    async fn validate(&self, quick: bool) -> Result<ValidationReport>;

    /// Write data to a new file
    ///
    /// This creates a new file in the format handled by this implementation.
    async fn write(&self, data: Vec<RecordBatch>, options: &WriteOptions) -> Result<()>;

    /// Estimate the number of rows
    ///
    /// This should be fast and can return an estimate based on metadata.
    async fn estimate_row_count(&self) -> Result<Option<i64>> {
        let metadata = self.read_metadata().await?;
        Ok(metadata.num_rows)
    }

    /// Check if the format supports write operations
    fn supports_write(&self) -> bool {
        true
    }

    /// Check if the format has native statistics
    fn has_native_statistics(&self) -> bool {
        false
    }
}

/// Factory for creating format handlers
pub struct FormatHandlerFactory;

impl FormatHandlerFactory {
    /// Detect the format and create an appropriate handler
    ///
    /// This tries each registered handler's `can_handle` method until one
    /// returns true. The order of checking is optimized for common formats.
    pub async fn create_handler(
        path: &Path,
        storage: Arc<dyn crate::core::storage::StorageBackend>,
    ) -> Result<Box<dyn FormatHandler>> {
        // Try Parquet first (most common)
        let parquet_handler = crate::core::formats::ParquetHandler::new(path, storage.clone())?;
        if parquet_handler.can_handle(path).await? {
            return Ok(Box::new(parquet_handler));
        }

        // Try Arrow IPC
        let arrow_handler = crate::core::formats::ArrowHandler::new(path, storage.clone())?;
        if arrow_handler.can_handle(path).await? {
            return Ok(Box::new(arrow_handler));
        }

        // Try table formats (Delta, Iceberg) - these check for metadata directories
        #[cfg(feature = "delta")]
        {
            let delta_handler = crate::core::formats::DeltaHandler::new(path, storage.clone())?;
            if delta_handler.can_handle(path).await? {
                return Ok(Box::new(delta_handler));
            }
        }

        #[cfg(feature = "iceberg")]
        {
            let iceberg_handler = crate::core::formats::IcebergHandler::new(path, storage)?;
            if iceberg_handler.can_handle(path).await? {
                return Ok(Box::new(iceberg_handler));
            }
        }

        Err(crate::error::Error::InvalidFormat {
            message: format!("Could not detect format for path: {}", path.display()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_report() {
        let mut report = ValidationReport::success();
        assert!(report.is_valid);
        assert!(report.errors.is_empty());

        report.add_warning("Test warning".to_string());
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn test_read_options_default() {
        let options = ReadOptions::default();
        assert!(options.columns.is_none());
        assert!(options.limit.is_none());
        assert!(!options.sample);
    }
}
