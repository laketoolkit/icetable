//! Arrow IPC format handler implementation
//!
//! This module provides support for reading and writing Arrow IPC (Feather) files.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use datafusion::arrow::array::Array;
use datafusion::arrow::datatypes::Schema;
use datafusion::arrow::ipc::reader::FileReader as ArrowFileReader;
use datafusion::arrow::ipc::writer::FileWriter as ArrowFileWriter;
use datafusion::arrow::record_batch::RecordBatch;
use async_trait::async_trait;
use bytes::Bytes;

use crate::core::formats::traits::*;
use crate::core::storage::StorageBackend;
use crate::error::{Error, Result};

/// Handler for Arrow IPC files
pub struct ArrowHandler {
    path: PathBuf,
    storage: Arc<dyn StorageBackend>,
}

impl ArrowHandler {
    /// Create a new Arrow IPC handler
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

    /// Create a reader from file content
    fn create_reader(&self, data: Bytes) -> Result<ArrowFileReader<Cursor<Bytes>>> {
        let cursor = Cursor::new(data);
        ArrowFileReader::try_new(cursor, None).map_err(|e| {
            Error::corrupted_file(&self.path, format!("Failed to read Arrow IPC file: {}", e))
        })
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
impl FormatHandler for ArrowHandler {
    async fn can_handle(&self, path: &Path) -> Result<bool> {
        // Check for .arrow or .feather extension first (fast check)
        if path
            .extension()
            .map_or(false, |ext| ext == "arrow" || ext == "feather")
        {
            return Ok(true);
        }

        // For files without extension, try to read magic bytes
        // Arrow IPC files start with "ARROW1" magic bytes
        let path_str = path.to_string_lossy().to_string();
        if self.storage.exists(&path_str).await? {
            if let Ok(bytes) = self.storage.get_range(&path_str, 0, 6).await {
                if bytes.len() >= 6 && &bytes[..6] == b"ARROW1" {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    fn format_name(&self) -> &str {
        "Apache Arrow IPC"
    }

    async fn read_schema(&self) -> Result<Arc<Schema>> {
        let file_content = self.read_file_content().await?;
        let reader = self.create_reader(file_content)?;
        Ok(reader.schema())
    }

    async fn read_metadata(&self) -> Result<FileMetadata> {
        let file_content = self.read_file_content().await?;
        let mut reader = self.create_reader(file_content.clone())?;

        // Count total rows and batches
        let mut total_rows = 0u64;
        let mut num_batches = 0usize;

        while let Some(batch_result) = reader.next() {
            let batch = batch_result.map_err(|e| Error::Arrow(e))?;
            total_rows += batch.num_rows() as u64;
            num_batches += 1;
        }

        let file_size = file_content.len() as u64;

        let mut metadata_map = std::collections::HashMap::new();
        metadata_map.insert("num_batches".to_string(), num_batches.to_string());

        Ok(FileMetadata {
            num_rows: Some(total_rows as i64),
            compressed_size: Some(file_size),
            uncompressed_size: Some(file_size), // Arrow IPC is not compressed by default
            compression: None,
            format_version: Some("1.0".to_string()),
            created_at: None,
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
            datafusion::arrow::compute::concat_batches(&schema, &batches).map_err(|e| Error::Arrow(e))
        }
    }

    async fn read_batches(&self, options: &ReadOptions) -> Result<Vec<RecordBatch>> {
        let file_content = self.read_file_content().await?;
        let mut reader = self.create_reader(file_content)?;

        let mut batches = Vec::new();
        let mut total_rows = 0usize;
        let offset = options.offset().unwrap_or(0);
        let limit = options.limit().unwrap_or(usize::MAX);

        // Read all batches
        while let Some(batch_result) = reader.next() {
            let mut batch = batch_result.map_err(|e| Error::Arrow(e))?;

            // Apply column filtering if specified
            if let Some(cols) = options.columns() {
                let schema = batch.schema();
                let mut indices = Vec::new();
                for col_name in cols {
                    if let Ok(idx) = schema.index_of(col_name) {
                        indices.push(idx);
                    }
                }
                if !indices.is_empty() {
                    let columns: Result<Vec<_>> = indices
                        .iter()
                        .map(|&idx| batch.column(idx).clone().slice(0, batch.num_rows()).into())
                        .collect::<Vec<_>>()
                        .into_iter()
                        .map(Ok)
                        .collect();
                    let columns = columns?;
                    let filtered_fields: Vec<_> = indices
                        .iter()
                        .map(|&idx| schema.field(idx).clone())
                        .collect();
                    let filtered_schema = Arc::new(Schema::new(filtered_fields));
                    batch = RecordBatch::try_new(filtered_schema, columns)
                        .map_err(|e| Error::Arrow(e))?;
                }
            }

            let batch_rows = batch.num_rows();

            // Handle offset
            if total_rows + batch_rows <= offset {
                total_rows += batch_rows;
                continue;
            }

            let start_in_batch = if total_rows < offset {
                offset - total_rows
            } else {
                0
            };

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
        // Arrow IPC doesn't store statistics, so we need to calculate them
        let batches = self.read_batches(&ReadOptions::default()).await?;
        self.calculate_statistics(&batches)
    }

    async fn validate(&self, quick: bool) -> Result<ValidationReport> {
        let mut report = ValidationReport::success();

        // Quick validation: Just try to read metadata
        match self.read_metadata().await {
            Ok(_) => {
                // File structure is valid
            }
            Err(e) => {
                return Ok(ValidationReport::failed(vec![format!(
                    "Failed to read Arrow IPC metadata: {}",
                    e
                )]));
            }
        }

        // If not quick mode, try to read all data
        if !quick {
            match self.read_batches(&ReadOptions::default()).await {
                Ok(_batches) => {
                    // Successfully read all batches - no action needed
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
        if data.is_empty() {
            return Err(Error::General("No data to write".to_string()));
        }

        let schema = data[0].schema();

        // Create a buffer to write to
        let mut buffer = Vec::new();

        // Create Arrow IPC writer
        let mut writer = ArrowFileWriter::try_new(&mut buffer, &schema)
            .map_err(|e| Error::General(format!("Failed to create Arrow IPC writer: {}", e)))?;

        for batch in data {
            writer
                .write(&batch)
                .map_err(|e| Error::General(format!("Failed to write batch: {}", e)))?;
        }

        writer
            .finish()
            .map_err(|e| Error::General(format!("Failed to finish writing: {}", e)))?;

        // Write to storage
        let path_str = self.path.to_string_lossy().to_string();
        let put_options = crate::core::storage::PutOptions::default();
        self.storage
            .put(&path_str, Bytes::from(buffer), &put_options)
            .await?;

        Ok(())
    }

    fn has_native_statistics(&self) -> bool {
        false // Arrow IPC doesn't have built-in column statistics
    }
}
