//! Core traits for format handlers
//!
//! Defines the FormatHandler trait that all format implementations must satisfy.
//! This provides a unified interface for working with Apache Iceberg tables.

use crate::core::storage::{Storage, detect_storage_type};
use arrow::datatypes::Schema;
use arrow::record_batch::RecordBatch;
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

use crate::error::Result;

/// Options for time-travel queries
#[derive(Debug, Clone, Default)]
pub struct TimeTravelOptions {
    /// Specific snapshot ID
    pub version: Option<i64>,

    /// Timestamp for as-of queries (format: "2024-01-15" or "2024-01-15T10:30:00")
    pub as_of: Option<String>,
}

impl TimeTravelOptions {
    /// Check if any time-travel option is set
    pub fn is_set(&self) -> bool {
        self.version.is_some() || self.as_of.is_some()
    }
}

/// Metadata about a table
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
    columns: Option<Vec<String>>,
    offset: Option<usize>,
    limit: Option<usize>,
    sample: bool,
    batch_size: Option<usize>,
}

impl ReadOptions {
    /// Create a new builder for ReadOptions
    pub fn builder() -> ReadOptionsBuilder {
        ReadOptionsBuilder::default()
    }

    /// Get columns to read
    pub fn columns(&self) -> Option<&Vec<String>> {
        self.columns.as_ref()
    }

    /// Get row offset
    pub fn offset(&self) -> Option<usize> {
        self.offset
    }

    /// Get row limit
    pub fn limit(&self) -> Option<usize> {
        self.limit
    }

    /// Check if sampling is enabled
    pub fn sample(&self) -> bool {
        self.sample
    }

    /// Get batch size
    pub fn batch_size(&self) -> Option<usize> {
        self.batch_size
    }
}

/// Builder for ReadOptions
#[derive(Debug, Default)]
pub struct ReadOptionsBuilder {
    columns: Option<Vec<String>>,
    offset: Option<usize>,
    limit: Option<usize>,
    sample: bool,
    batch_size: Option<usize>,
}

impl ReadOptionsBuilder {
    /// Set columns to read
    pub fn columns(mut self, columns: Vec<String>) -> Self {
        self.columns = Some(columns);
        self
    }

    /// Set row offset
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Set row limit
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Enable sampling
    pub fn sample(mut self, sample: bool) -> Self {
        self.sample = sample;
        self
    }

    /// Set batch size
    pub fn batch_size(mut self, size: usize) -> Self {
        self.batch_size = Some(size);
        self
    }

    /// Build the ReadOptions
    pub fn build(self) -> ReadOptions {
        ReadOptions {
            columns: self.columns,
            offset: self.offset,
            limit: self.limit,
            sample: self.sample,
            batch_size: self.batch_size,
        }
    }
}

/// Options for writing data
#[derive(Debug, Clone, Default)]
pub struct WriteOptions {
    compression: Option<String>,
    row_group_size: Option<usize>,
    enable_dictionary: bool,
    enable_statistics: bool,
    overwrite: bool,
}

impl WriteOptions {
    /// Create a new builder for WriteOptions
    pub fn builder() -> WriteOptionsBuilder {
        WriteOptionsBuilder::default()
    }

    /// Get compression codec
    pub fn compression(&self) -> Option<&String> {
        self.compression.as_ref()
    }

    /// Get row group size
    pub fn row_group_size(&self) -> Option<usize> {
        self.row_group_size
    }

    /// Check if dictionary encoding is enabled
    pub fn enable_dictionary(&self) -> bool {
        self.enable_dictionary
    }

    /// Check if statistics generation is enabled
    pub fn enable_statistics(&self) -> bool {
        self.enable_statistics
    }

    /// Check if overwrite is enabled
    pub fn overwrite(&self) -> bool {
        self.overwrite
    }
}

/// Builder for WriteOptions
#[derive(Debug, Default)]
pub struct WriteOptionsBuilder {
    compression: Option<String>,
    row_group_size: Option<usize>,
    enable_dictionary: bool,
    enable_statistics: bool,
    overwrite: bool,
}

impl WriteOptionsBuilder {
    /// Set compression codec
    pub fn compression(mut self, compression: String) -> Self {
        self.compression = Some(compression);
        self
    }

    /// Set row group size
    pub fn row_group_size(mut self, size: usize) -> Self {
        self.row_group_size = Some(size);
        self
    }

    /// Enable dictionary encoding
    pub fn enable_dictionary(mut self, enable: bool) -> Self {
        self.enable_dictionary = enable;
        self
    }

    /// Enable statistics generation
    pub fn enable_statistics(mut self, enable: bool) -> Self {
        self.enable_statistics = enable;
        self
    }

    /// Enable overwrite
    pub fn overwrite(mut self, overwrite: bool) -> Self {
        self.overwrite = overwrite;
        self
    }

    /// Build the WriteOptions
    pub fn build(self) -> WriteOptions {
        WriteOptions {
            compression: self.compression,
            row_group_size: self.row_group_size,
            enable_dictionary: self.enable_dictionary,
            enable_statistics: self.enable_statistics,
            overwrite: self.overwrite,
        }
    }
}

/// Validation report for a table
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
    async fn can_handle(&self, path: &Path) -> Result<bool>;

    /// Get the format name (e.g., "Apache Iceberg")
    fn format_name(&self) -> &str;

    /// Read the schema without loading data
    async fn read_schema(&self) -> Result<Arc<Schema>>;

    /// Read metadata about the table
    async fn read_metadata(&self) -> Result<FileMetadata>;

    /// Read a batch of data
    async fn read_batch(&self, options: &ReadOptions) -> Result<RecordBatch>;

    /// Read multiple batches as a stream
    async fn read_batches(&self, options: &ReadOptions) -> Result<Vec<RecordBatch>>;

    /// Get statistics for all columns
    async fn read_statistics(&self) -> Result<Vec<ColumnStats>>;

    /// Validate the table structure and data
    async fn validate(&self, quick: bool) -> Result<ValidationReport>;

    /// Write data to the table
    async fn write(&self, data: Vec<RecordBatch>, options: &WriteOptions) -> Result<()>;

    /// Estimate the number of rows
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

/// Factory for creating Iceberg format handlers
pub struct FormatHandlerFactory;

impl FormatHandlerFactory {
    /// Create a handler for Iceberg format
    pub async fn create_handler_for_format(
        format: &str,
        path: &Path,
        storage: Storage,
    ) -> Result<Box<dyn FormatHandler>> {
        match format.to_lowercase().as_str() {
            "iceberg" => {
                let handler = crate::core::formats::IcebergHandler::new(path, storage)?;
                Ok(Box::new(handler))
            }
            _ => Err(crate::error::Error::InvalidFormat {
                message: format!("Unsupported format: {}. Only 'iceberg' is supported.", format),
            }),
        }
    }

    /// Detect the table format and create an appropriate handler
    pub async fn create_handler(path: &Path, storage: Storage) -> Result<Box<dyn FormatHandler>> {
        let path_str = path.to_str().unwrap_or("");

        // For cloud storage, use listing-based detection
        if detect_storage_type(path_str) != "local" {
            if Self::check_iceberg_exists(path_str, &storage).await {
                let handler = crate::core::formats::IcebergHandler::new(path, storage)?;
                return Ok(Box::new(handler));
            }

            return Err(crate::error::Error::InvalidFormat {
                message: format!("Could not detect Iceberg table at path: {}", path.display()),
            });
        }

        // For local storage, use can_handle
        let iceberg_handler = crate::core::formats::IcebergHandler::new(path, storage)?;
        if iceberg_handler.can_handle(path).await? {
            return Ok(Box::new(iceberg_handler));
        }

        Err(crate::error::Error::InvalidFormat {
            message: format!("Could not detect Iceberg table at path: {}", path.display()),
        })
    }

    /// Check if Iceberg table exists using storage backend
    async fn check_iceberg_exists(path_str: &str, storage: &Storage) -> bool {
        use crate::core::storage::to_path;
        use futures::TryStreamExt;

        let clean_path = Self::extract_storage_path(path_str);

        let metadata_prefix = if clean_path.is_empty() {
            "metadata/".to_string()
        } else {
            format!("{}/metadata/", clean_path)
        };

        let prefix_path = to_path(&metadata_prefix);
        let stream = storage.list(Some(&prefix_path));

        match stream.try_collect::<Vec<_>>().await {
            Ok(items) => items
                .iter()
                .take(5)
                .any(|obj| obj.location.to_string().contains(".metadata.json")),
            Err(_) => false,
        }
    }

    /// Extract storage path (removes scheme and bucket)
    fn extract_storage_path(path_str: &str) -> String {
        if let Some(pos) = path_str.find("://") {
            let after_scheme = &path_str[pos + 3..];
            if let Some(slash_pos) = after_scheme.find('/') {
                after_scheme[slash_pos + 1..].to_string()
            } else {
                String::new()
            }
        } else {
            path_str.to_string()
        }
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
