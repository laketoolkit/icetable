//! JSON/NDJSON format handler implementation
//!
//! This module provides support for reading and writing JSON and NDJSON (newline-delimited JSON)
//! files using the arrow-json crate with proper schema inference.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::Array;
use arrow::datatypes::Schema;
use arrow::record_batch::RecordBatch;
use arrow_json::{ReaderBuilder, WriterBuilder, writer::LineDelimited};
use async_trait::async_trait;
use bytes::Bytes;

use crate::core::formats::traits::*;
use crate::core::storage::StorageBackend;
use crate::error::{Error, Result};

/// Handler for JSON/NDJSON files
pub struct JsonHandler {
    path: PathBuf,
    storage: Arc<dyn StorageBackend>,
}

impl JsonHandler {
    /// Create a new JSON handler
    pub fn new(path: &Path, storage: Arc<dyn StorageBackend>) -> Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            storage,
        })
    }

    /// Read the entire file content into memory
    async fn read_file_content(&self) -> Result<Bytes> {
        let path_str = self.path.to_string_lossy().to_string();
        let options = crate::core::storage::GetOptions::default();
        self.storage.get(&path_str, &options).await
    }

    /// Infer schema from JSON file
    async fn infer_schema(&self, max_records: Option<usize>) -> Result<Arc<Schema>> {
        let file_content = self.read_file_content().await?;
        let cursor = Cursor::new(file_content);

        let (schema, _inferred_len) = arrow_json::reader::infer_json_schema(cursor, max_records)
            .map_err(|e| {
                Error::corrupted_file(&self.path, format!("Failed to infer JSON schema: {}", e))
            })?;

        Ok(Arc::new(schema))
    }

    /// Calculate statistics from record batches
    fn calculate_statistics(&self, batches: &[RecordBatch]) -> Result<Vec<ColumnStats>> {
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
impl FormatHandler for JsonHandler {
    async fn can_handle(&self, path: &Path) -> Result<bool> {
        // Check for JSON extensions first (fast check)
        if let Some(ext) = path.extension() {
            let ext_str = ext.to_string_lossy();
            if ext_str.eq_ignore_ascii_case("json")
                || ext_str.eq_ignore_ascii_case("jsonl")
                || ext_str.eq_ignore_ascii_case("ndjson")
            {
                return Ok(true);
            }
        }

        // For files without extension, try to peek at content
        let path_str = path.to_string_lossy().to_string();
        if self.storage.exists(&path_str).await? {
            // Read first few bytes to check if it looks like JSON
            if let Ok(bytes) = self.storage.get_range(&path_str, 0, 100).await {
                if let Ok(content) = std::str::from_utf8(&bytes) {
                    let trimmed = content.trim_start();
                    // Check for JSON object or array start
                    return Ok(trimmed.starts_with('{') || trimmed.starts_with('['));
                }
            }
        }

        Ok(false)
    }

    fn format_name(&self) -> &str {
        "JSON/NDJSON"
    }

    async fn read_schema(&self) -> Result<Arc<Schema>> {
        self.infer_schema(Some(100)).await
    }

    async fn read_metadata(&self) -> Result<FileMetadata> {
        let path_str = self.path.to_string_lossy().to_string();
        let metadata = self.storage.head(&path_str).await?;

        let mut metadata_map = std::collections::HashMap::new();
        metadata_map.insert("path".to_string(), path_str);

        Ok(FileMetadata {
            num_rows: None, // Would need to scan entire file
            compressed_size: Some(metadata.size),
            uncompressed_size: Some(metadata.size),
            compression: Some("none".to_string()),
            format_version: None,
            created_at: Some(metadata.last_modified),
            metadata: metadata_map,
        })
    }

    async fn read_batch(&self, options: &ReadOptions) -> Result<RecordBatch> {
        let batches = self.read_batches(options).await?;

        if batches.is_empty() {
            // Return empty batch with schema
            let schema = self.read_schema().await?;
            Ok(RecordBatch::new_empty(schema))
        } else if batches.len() == 1 {
            Ok(batches.into_iter().next().unwrap())
        } else {
            // Concatenate multiple batches
            let schema = batches[0].schema();
            arrow::compute::concat_batches(&schema, &batches).map_err(Error::Arrow)
        }
    }

    async fn read_batches(&self, options: &ReadOptions) -> Result<Vec<RecordBatch>> {
        let file_content = self.read_file_content().await?;
        let cursor = Cursor::new(file_content);

        // Infer schema first
        let schema = self.infer_schema(Some(100)).await?;

        // Build JSON reader
        let batch_size = options.batch_size().unwrap_or(8192);
        let builder = ReaderBuilder::new(schema.clone()).with_batch_size(batch_size);

        let mut reader = builder.build(cursor).map_err(|e| {
            Error::corrupted_file(&self.path, format!("Failed to create JSON reader: {}", e))
        })?;

        let mut batches = Vec::new();
        let mut total_rows = 0usize;
        let offset = options.offset().unwrap_or(0);
        let limit = options.limit().unwrap_or(usize::MAX);

        // Read batches
        for batch_result in reader.by_ref() {
            let mut batch = batch_result.map_err(Error::Arrow)?;

            // Apply column projection if specified
            if let Some(cols) = options.columns() {
                let batch_schema = batch.schema();
                let mut projection_indices = Vec::new();

                for col_name in cols {
                    if let Ok(idx) = batch_schema.index_of(col_name) {
                        projection_indices.push(idx);
                    } else {
                        return Err(Error::General(format!(
                            "Column '{}' not found in JSON schema",
                            col_name
                        )));
                    }
                }

                // Project columns
                let projected_columns: Vec<_> = projection_indices
                    .iter()
                    .map(|&idx| batch.column(idx).clone())
                    .collect();

                let projected_fields: Vec<_> = projection_indices
                    .iter()
                    .map(|&idx| batch_schema.field(idx).clone())
                    .collect();

                let projected_schema = Arc::new(Schema::new(projected_fields));
                batch = RecordBatch::try_new(projected_schema, projected_columns)
                    .map_err(Error::Arrow)?;
            }

            let batch_rows = batch.num_rows();

            // Handle offset
            if total_rows + batch_rows <= offset {
                total_rows += batch_rows;
                continue;
            }

            let start_in_batch = offset.saturating_sub(total_rows);

            let rows_to_take = (batch_rows - start_in_batch).min(
                limit
                    - (batches
                        .iter()
                        .map(|b: &RecordBatch| b.num_rows())
                        .sum::<usize>()),
            );

            if rows_to_take > 0 {
                let sliced_batch = batch.slice(start_in_batch, rows_to_take);
                batches.push(sliced_batch);
            }

            total_rows += batch_rows;

            // Check if we've collected enough rows
            let collected_rows: usize = batches.iter().map(|b| b.num_rows()).sum();
            if collected_rows >= limit {
                break;
            }
        }

        Ok(batches)
    }

    async fn read_statistics(&self) -> Result<Vec<ColumnStats>> {
        // JSON doesn't have native statistics, calculate from data
        let batches = self.read_batches(&ReadOptions::default()).await?;
        self.calculate_statistics(&batches)
    }

    async fn validate(&self, quick: bool) -> Result<ValidationReport> {
        let mut report = ValidationReport::success();

        // Quick validation: Just try to read schema
        match self.read_schema().await {
            Ok(_) => {
                // File structure is valid
            }
            Err(e) => {
                return Ok(ValidationReport::failed(vec![format!(
                    "Failed to read JSON schema: {}",
                    e
                )]));
            }
        }

        // If not quick mode, try to read all data
        if !quick {
            match self.read_batches(&ReadOptions::default()).await {
                Ok(batches) => {
                    if batches.is_empty() {
                        report.add_warning("JSON file is empty".to_string());
                    }
                }
                Err(e) => {
                    report.errors.push(format!("Failed to read data: {}", e));
                    report.is_valid = false;
                }
            }
        }

        Ok(report)
    }

    async fn write(&self, data: Vec<RecordBatch>, _options: &WriteOptions) -> Result<()> {
        if data.is_empty() {
            return Err(Error::General("No data to write".to_string()));
        }

        // Create a buffer to write to
        let mut buffer = Vec::new();

        // Create JSON writer - writes NDJSON format (one JSON object per line)
        let mut writer = WriterBuilder::new().build::<_, LineDelimited>(&mut buffer);

        for batch in data {
            writer
                .write(&batch)
                .map_err(|e| Error::General(format!("Failed to write JSON batch: {}", e)))?;
        }

        writer
            .finish()
            .map_err(|e| Error::General(format!("Failed to finish JSON writer: {}", e)))?;

        drop(writer);

        // Write to storage
        let path_str = self.path.to_string_lossy().to_string();
        let put_options = crate::core::storage::PutOptions::default();
        self.storage
            .put(&path_str, Bytes::from(buffer), &put_options)
            .await?;

        Ok(())
    }

    fn has_native_statistics(&self) -> bool {
        false // JSON doesn't have built-in column statistics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_handle_json_extension() {
        // Test would require mock storage backend
        // Placeholder for when we implement tests
    }
}
