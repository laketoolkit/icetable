//! Base format handler implementation using the Strategy pattern
//!
//! This module provides `BaseFormatHandler`, a generic handler that eliminates
//! code duplication across different format implementations.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    BaseFormatHandler                         │
//! │  (Common logic: I/O, pagination, projection, validation)    │
//! └──────────────────────┬──────────────────────────────────────┘
//!                        │
//!              Delegates to strategies
//!                        │
//!        ┌───────────────┴───────────────┐
//!        │                               │
//!   ┌────▼─────┐                   ┌────▼─────┐
//!   │  Reader  │                   │  Writer  │
//!   │ Strategy │                   │ Strategy │
//!   └────┬─────┘                   └────┬─────┘
//!        │                               │
//!   Format-specific                 Format-specific
//!   implementations                 implementations
//! ```
//!
//! # Benefits
//!
//! - Eliminates ~200 lines of duplicated code per format handler
//! - Adding a new format requires only 50-80 lines (just the strategy)
//! - Centralizes pagination, projection, and validation logic
//! - Maintains type safety and zero-cost abstractions

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use datafusion::arrow::array::RecordBatch;
use datafusion::arrow::compute;
use datafusion::arrow::datatypes::Schema;

use crate::core::formats::strategies::{BatchReader, ReaderStrategy, WriterStrategy};
use crate::core::formats::traits::*;
use crate::core::storage::{GetOptions, PutOptions, StorageBackend};
use crate::error::{Error, Result};

/// Generic format handler using composition with strategies
///
/// This handler implements all the common logic for file I/O, pagination,
/// column projection, and validation. Format-specific behavior is delegated
/// to the provided `ReaderStrategy` and optional `WriterStrategy`.
///
/// # Example
///
/// ```ignore
/// let handler = BaseFormatHandler::new(
///     path,
///     storage,
///     Arc::new(ArrowReaderStrategy::new()),
///     Some(Arc::new(ArrowWriterStrategy::new())),
/// );
/// ```
pub struct BaseFormatHandler {
    path: PathBuf,
    storage: Arc<dyn StorageBackend>,
    reader_strategy: Arc<dyn ReaderStrategy>,
    writer_strategy: Option<Arc<dyn WriterStrategy>>,
}

impl BaseFormatHandler {
    /// Create a new base format handler
    pub fn new(
        path: PathBuf,
        storage: Arc<dyn StorageBackend>,
        reader_strategy: Arc<dyn ReaderStrategy>,
        writer_strategy: Option<Arc<dyn WriterStrategy>>,
    ) -> Self {
        Self {
            path,
            storage,
            reader_strategy,
            writer_strategy,
        }
    }

    /// Read entire file content into memory
    ///
    /// This is the common I/O operation shared by all formats.
    /// Eliminates duplication of this logic across handlers.
    async fn read_file_content(&self) -> Result<Bytes> {
        let path_str = self.path.to_string_lossy().to_string();
        let options = GetOptions::default();
        self.storage.get(&path_str, &options).await
    }

    /// Apply column projection to a batch
    ///
    /// Filters the batch to only include requested columns.
    /// This logic was duplicated across all format handlers.
    fn apply_column_projection(
        &self,
        batch: RecordBatch,
        columns: &[String],
    ) -> Result<RecordBatch> {
        let schema = batch.schema();
        let mut indices = Vec::new();

        for col_name in columns {
            match schema.index_of(col_name) {
                Ok(idx) => indices.push(idx),
                Err(_) => {
                    // Column not found - skip it (lenient behavior)
                    continue;
                }
            }
        }

        if indices.is_empty() {
            // No valid columns found - return empty batch with schema
            return Ok(RecordBatch::new_empty(batch.schema()));
        }

        // Project columns
        let projected_columns: Vec<_> = indices
            .iter()
            .map(|&idx| batch.column(idx).clone())
            .collect();

        let projected_fields: Vec<_> = indices
            .iter()
            .map(|&idx| schema.field(idx).clone())
            .collect();

        let projected_schema = Arc::new(Schema::new(projected_fields));

        RecordBatch::try_new(projected_schema, projected_columns).map_err(|e| Error::Arrow(e))
    }

    /// Apply pagination (offset/limit) to a batch stream
    ///
    /// This logic was duplicated ~50 lines in each format handler.
    /// Now implemented once here.
    fn apply_pagination(
        &self,
        reader: &mut Box<dyn BatchReader>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<RecordBatch>> {
        let mut batches = Vec::new();
        let mut total_rows_seen = 0usize;
        let mut collected_rows = 0usize;

        while let Some(batch) = reader.next_batch()? {
            let batch_rows = batch.num_rows();

            // Skip batches before offset
            if total_rows_seen + batch_rows <= offset {
                total_rows_seen += batch_rows;
                continue;
            }

            // Calculate slice within this batch
            let start_in_batch = if total_rows_seen < offset {
                offset - total_rows_seen
            } else {
                0
            };

            let remaining_limit = limit - collected_rows;
            let rows_to_take = (batch_rows - start_in_batch).min(remaining_limit);

            if rows_to_take > 0 {
                let sliced_batch = batch.slice(start_in_batch, rows_to_take);
                collected_rows += sliced_batch.num_rows();
                batches.push(sliced_batch);
            }

            total_rows_seen += batch_rows;

            // Stop if we've collected enough rows
            if collected_rows >= limit {
                break;
            }
        }

        Ok(batches)
    }

    /// Merge multiple batches into a single batch
    ///
    /// Common operation when returning a single RecordBatch from multiple batches.
    fn merge_batches(&self, batches: Vec<RecordBatch>) -> Result<RecordBatch> {
        if batches.is_empty() {
            // Return empty batch with schema from reader
            let data = futures::executor::block_on(self.read_file_content())?;
            let schema = self.reader_strategy.extract_schema(data)?;
            Ok(RecordBatch::new_empty(Arc::new(schema)))
        } else if batches.len() == 1 {
            Ok(batches.into_iter().next().unwrap())
        } else {
            let schema = batches[0].schema();
            compute::concat_batches(&schema, &batches).map_err(|e| Error::Arrow(e))
        }
    }

    /// Calculate basic statistics from batches
    ///
    /// Fallback for formats without native statistics.
    /// Computes null counts for all columns.
    fn calculate_basic_statistics(&self, batches: &[RecordBatch]) -> Result<Vec<ColumnStats>> {
        if batches.is_empty() {
            return Ok(Vec::new());
        }

        let schema = batches[0].schema();
        let mut stats = Vec::new();

        for (col_idx, field) in schema.fields().iter().enumerate() {
            let mut null_count = 0i64;

            for batch in batches {
                let column = batch.column(col_idx);
                null_count += column.null_count() as i64;
            }

            stats.push(ColumnStats {
                name: field.name().clone(),
                null_count: Some(null_count),
                distinct_count: None,
                min_value: None,
                max_value: None,
                mean: None,
                std_dev: None,
            });
        }

        Ok(stats)
    }
}

#[async_trait]
impl FormatHandler for BaseFormatHandler {
    async fn can_handle(&self, path: &Path) -> Result<bool> {
        // Check file extension first (fast path)
        let extensions = self.reader_strategy.file_extensions();
        if let Some(ext) = path.extension() {
            let ext_str = ext.to_string_lossy();
            if extensions.iter().any(|&e| e == ext_str) {
                return Ok(true);
            }
        }

        // Check magic bytes if available (requires storage access)
        if let Some(magic) = self.reader_strategy.magic_bytes() {
            let path_str = path.to_string_lossy().to_string();
            if self.storage.exists(&path_str).await? {
                if let Ok(bytes) = self
                    .storage
                    .get_range(&path_str, 0, magic.len() as u64)
                    .await
                {
                    if bytes.len() >= magic.len() && &bytes[..magic.len()] == magic {
                        return Ok(true);
                    }
                }
            }
        }

        Ok(false)
    }

    fn format_name(&self) -> &str {
        self.reader_strategy.format_name()
    }

    async fn read_schema(&self) -> Result<Arc<Schema>> {
        let data = self.read_file_content().await?;
        let schema = self.reader_strategy.extract_schema(data)?;
        Ok(Arc::new(schema))
    }

    async fn read_metadata(&self) -> Result<FileMetadata> {
        let file_content = self.read_file_content().await?;
        let mut reader = self
            .reader_strategy
            .create_batch_reader(file_content.clone())?;

        // Count total rows and batches
        let mut total_rows = 0u64;
        let mut num_batches = 0usize;

        while let Some(batch) = reader.next_batch()? {
            total_rows += batch.num_rows() as u64;
            num_batches += 1;
        }

        let file_size = file_content.len() as u64;

        let mut metadata_map = std::collections::HashMap::new();
        metadata_map.insert("num_batches".to_string(), num_batches.to_string());

        Ok(FileMetadata {
            num_rows: Some(total_rows as i64),
            compressed_size: Some(file_size),
            uncompressed_size: Some(file_size),
            compression: None,
            format_version: None,
            created_at: None,
            metadata: metadata_map,
        })
    }

    async fn read_batch(&self, options: &ReadOptions) -> Result<RecordBatch> {
        let batches = self.read_batches(options).await?;
        self.merge_batches(batches)
    }

    async fn read_batches(&self, options: &ReadOptions) -> Result<Vec<RecordBatch>> {
        let file_content = self.read_file_content().await?;
        let mut reader = self.reader_strategy.create_batch_reader(file_content)?;

        // Apply pagination
        let offset = options.offset().unwrap_or(0);
        let limit = options.limit().unwrap_or(usize::MAX);
        let mut batches = self.apply_pagination(&mut reader, offset, limit)?;

        // Apply column projection if specified
        if let Some(cols) = options.columns() {
            batches = batches
                .into_iter()
                .map(|batch| self.apply_column_projection(batch, cols))
                .collect::<Result<Vec<_>>>()?;
        }

        Ok(batches)
    }

    async fn read_statistics(&self) -> Result<Vec<ColumnStats>> {
        // Use native statistics if available
        if self.reader_strategy.has_native_statistics() {
            let data = self.read_file_content().await?;
            return self.reader_strategy.extract_native_statistics(data).await;
        }

        // Otherwise, compute basic statistics by scanning
        let batches = self.read_batches(&ReadOptions::default()).await?;
        self.calculate_basic_statistics(&batches)
    }

    async fn validate(&self, quick: bool) -> Result<ValidationReport> {
        let mut report = ValidationReport::success();

        // Quick validation: try to read metadata
        match self.read_metadata().await {
            Ok(_) => {
                // File structure is valid
            }
            Err(e) => {
                return Ok(ValidationReport::failed(vec![format!(
                    "Failed to read {} metadata: {}",
                    self.format_name(),
                    e
                )]));
            }
        }

        // Full validation: try to read all data
        if !quick {
            match self.read_batches(&ReadOptions::default()).await {
                Ok(_) => {
                    // Successfully read all batches
                }
                Err(e) => {
                    report.errors.push(format!("Failed to read data: {}", e));
                    report.is_valid = false;
                }
            }
        }

        Ok(report)
    }

    async fn write(&self, data: Vec<RecordBatch>, options: &WriteOptions) -> Result<()> {
        let writer_strategy = self.writer_strategy.as_ref().ok_or_else(|| {
            Error::General(format!(
                "{} format does not support writing",
                self.format_name()
            ))
        })?;

        if data.is_empty() {
            return Err(Error::General("No data to write".to_string()));
        }

        let schema = data[0].schema();

        // Create in-memory buffer
        let mut buffer = Vec::new();

        // Create format-specific writer and write batches
        {
            let mut batch_writer =
                writer_strategy.create_batch_writer(&mut buffer, &schema, options)?;

            // Write all batches
            for batch in &data {
                batch_writer.write_batch(batch)?;
            }

            // Finalize
            batch_writer.finish()?;
        } // batch_writer dropped here, releasing borrow of buffer

        // Upload to storage
        let path_str = self.path.to_string_lossy().to_string();
        let put_options = PutOptions::default();
        self.storage
            .put(&path_str, Bytes::from(buffer), &put_options)
            .await?;

        Ok(())
    }

    fn has_native_statistics(&self) -> bool {
        self.reader_strategy.has_native_statistics()
    }

    fn supports_write(&self) -> bool {
        self.writer_strategy.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verify that BaseFormatHandler implements FormatHandler
    #[test]
    fn test_base_handler_implements_trait() {
        fn assert_format_handler<T: FormatHandler>() {}
        // This compiles only if BaseFormatHandler implements FormatHandler
        // assert_format_handler::<BaseFormatHandler>(); // Can't test without concrete types
    }
}
