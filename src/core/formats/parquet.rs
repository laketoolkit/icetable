//! Parquet format handler implementation
//!
//! This module provides support for reading and writing Apache Parquet files.
//! It leverages the parquet crate from the Arrow project for efficient I/O.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::datatypes::{DataType, Schema};
use arrow::record_batch::RecordBatch;
use async_trait::async_trait;
use bytes::Bytes;
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::file::properties::WriterProperties;
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::basic::Type as PhysicalType;

use crate::core::formats::traits::*;
use crate::core::storage::StorageBackend;
use crate::error::{Error, Result};

/// Handler for Apache Parquet files
pub struct ParquetHandler {
    path: PathBuf,
    storage: Arc<dyn StorageBackend>,
}

impl ParquetHandler {
    /// Create a new Parquet handler
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

    /// Helper function to convert Parquet statistics bytes to readable strings
    fn format_stat_value(bytes: &[u8], physical_type: PhysicalType, _data_type: &DataType) -> String {
        match physical_type {
            PhysicalType::INT32 => {
                if bytes.len() >= 4 {
                    let value = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    value.to_string()
                } else {
                    format!("{:?}", bytes)
                }
            }
            PhysicalType::INT64 => {
                if bytes.len() >= 8 {
                    let value = i64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                        bytes[4], bytes[5], bytes[6], bytes[7],
                    ]);
                    value.to_string()
                } else {
                    format!("{:?}", bytes)
                }
            }
            PhysicalType::FLOAT => {
                if bytes.len() >= 4 {
                    let value = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    value.to_string()
                } else {
                    format!("{:?}", bytes)
                }
            }
            PhysicalType::DOUBLE => {
                if bytes.len() >= 8 {
                    let value = f64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                        bytes[4], bytes[5], bytes[6], bytes[7],
                    ]);
                    value.to_string()
                } else {
                    format!("{:?}", bytes)
                }
            }
            PhysicalType::BYTE_ARRAY | PhysicalType::FIXED_LEN_BYTE_ARRAY => {
                // Try to interpret as UTF-8 string
                match std::str::from_utf8(bytes) {
                    Ok(s) => s.to_string(),
                    Err(_) => format!("{:?}", bytes),
                }
            }
            PhysicalType::BOOLEAN => {
                if !bytes.is_empty() {
                    (bytes[0] != 0).to_string()
                } else {
                    format!("{:?}", bytes)
                }
            }
            _ => format!("{:?}", bytes),
        }
    }
}

#[async_trait]
impl FormatHandler for ParquetHandler {
    async fn can_handle(&self, path: &Path) -> Result<bool> {
        // Check for .parquet extension first (fast check)
        if path.extension().map_or(false, |ext| ext == "parquet") {
            return Ok(true);
        }

        // For files without extension, try to read magic bytes
        let path_str = path.to_string_lossy().to_string();
        if self.storage.exists(&path_str).await? {
            // Try to read first 4 bytes to check for "PAR1" magic
            if let Ok(bytes) = self.storage.get_range(&path_str, 0, 4).await {
                if bytes.len() >= 4 && &bytes[..4] == b"PAR1" {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    fn format_name(&self) -> &str {
        "Apache Parquet"
    }

    async fn read_schema(&self) -> Result<Arc<Schema>> {
        let file_content = self.read_file_content().await?;
        let reader = SerializedFileReader::new(file_content).map_err(|e| {
            Error::corrupted_file(
                &self.path,
                format!("Failed to read Parquet metadata: {}", e),
            )
        })?;

        let parquet_schema = reader.metadata().file_metadata().schema_descr();
        let arrow_schema =
            parquet::arrow::parquet_to_arrow_schema(parquet_schema, None).map_err(|e| {
                Error::corrupted_file(
                    &self.path,
                    format!("Failed to convert Parquet schema: {}", e),
                )
            })?;

        Ok(Arc::new(arrow_schema))
    }

    async fn read_metadata(&self) -> Result<FileMetadata> {
        let file_content = self.read_file_content().await?;
        let reader = SerializedFileReader::new(file_content).map_err(|e| {
            Error::corrupted_file(
                &self.path,
                format!("Failed to read Parquet metadata: {}", e),
            )
        })?;

        let parquet_metadata = reader.metadata();
        let file_metadata_ref = parquet_metadata.file_metadata();

        let num_rows = file_metadata_ref.num_rows();
        let num_row_groups = parquet_metadata.num_row_groups();

        let mut total_compressed_size = 0u64;
        let mut total_uncompressed_size = 0u64;

        for rg in parquet_metadata.row_groups() {
            total_compressed_size += rg.compressed_size() as u64;
            total_uncompressed_size += rg.total_byte_size() as u64;
        }

        // Get compression codec from first column chunk (if available)
        let compression = if num_row_groups > 0 {
            let first_rg = parquet_metadata.row_group(0);
            if first_rg.num_columns() > 0 {
                Some(format!("{:?}", first_rg.column(0).compression()))
            } else {
                None
            }
        } else {
            None
        };

        let mut metadata_map = std::collections::HashMap::new();
        metadata_map.insert("row_groups".to_string(), num_row_groups.to_string());
        metadata_map.insert(
            "created_by".to_string(),
            file_metadata_ref
                .created_by()
                .unwrap_or("Unknown")
                .to_string(),
        );

        Ok(FileMetadata {
            num_rows: Some(num_rows),
            compressed_size: Some(total_compressed_size),
            uncompressed_size: Some(total_uncompressed_size),
            compression,
            format_version: Some(format!("{}", file_metadata_ref.version())),
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
            arrow::compute::concat_batches(&schema, &batches).map_err(|e| Error::Arrow(e))
        }
    }

    async fn read_batches(&self, options: &ReadOptions) -> Result<Vec<RecordBatch>> {
        let file_content = self.read_file_content().await?;

        let builder = ParquetRecordBatchReaderBuilder::try_new(file_content).map_err(|e| {
            Error::corrupted_file(&self.path, format!("Failed to read Parquet: {}", e))
        })?;

        // Extract column indices if specified
        let projection_indices: Option<Vec<usize>> = if let Some(cols) = &options.columns {
            let schema = builder.schema();
            let mut indices = Vec::new();
            for col_name in cols {
                if let Ok(idx) = schema.index_of(col_name) {
                    indices.push(idx);
                }
            }
            if indices.is_empty() {
                None
            } else {
                Some(indices)
            }
        } else {
            None
        };

        // Apply projections and batch size
        let builder = match projection_indices {
            Some(indices) => {
                let projection_mask = {
                    let parquet_schema = builder.parquet_schema();
                    parquet::arrow::ProjectionMask::roots(parquet_schema, indices)
                };
                builder.with_projection(projection_mask)
            }
            None => builder,
        };

        // Set batch size
        let batch_size = options.batch_size.unwrap_or(1024);
        let builder = builder.with_batch_size(batch_size);

        let reader = builder.build().map_err(|e| Error::Parquet(e))?;

        let mut batches = Vec::new();
        let mut total_rows = 0usize;
        let offset = options.offset.unwrap_or(0);
        let limit = options.limit.unwrap_or(usize::MAX);

        for batch_result in reader {
            let batch = batch_result.map_err(|e| Error::Arrow(e))?;
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
        let file_content = self.read_file_content().await?;
        let reader = SerializedFileReader::new(file_content.clone()).map_err(|e| {
            Error::corrupted_file(&self.path, format!("Failed to read Parquet: {}", e))
        })?;

        let parquet_metadata = reader.metadata();

        // Get schema
        let parquet_schema = parquet_metadata.file_metadata().schema_descr();
        let schema =
            parquet::arrow::parquet_to_arrow_schema(parquet_schema, None).map_err(|e| {
                Error::corrupted_file(&self.path, format!("Failed to convert schema: {}", e))
            })?;

        let mut stats_map: std::collections::HashMap<String, ColumnStats> =
            std::collections::HashMap::new();

        // Initialize stats for all columns
        for field in schema.fields() {
            stats_map.insert(
                field.name().clone(),
                ColumnStats {
                    name: field.name().clone(),
                    null_count: Some(0),
                    distinct_count: None,
                    min_value: None,
                    max_value: None,
                    mean: None,
                    std_dev: None,
                },
            );
        }

        // Aggregate statistics from all row groups
        for rg in parquet_metadata.row_groups() {
            for (col_idx, col_chunk) in rg.columns().iter().enumerate() {
                if col_idx < schema.fields().len() {
                    let field = schema.field(col_idx);
                    let col_name = field.name();
                    let physical_type = col_chunk.column_descr().physical_type();

                    if let Some(stats) = col_chunk.statistics() {
                        if let Some(existing) = stats_map.get_mut(col_name) {
                            // Aggregate null count - check if null_count is available
                            if let Some(null_count) = existing.null_count {
                                existing.null_count = Some(null_count + stats.null_count_opt().unwrap_or(0) as i64);
                            }

                            // Update min/max values if available
                            if let (Some(min_bytes), Some(max_bytes)) = (stats.min_bytes_opt(), stats.max_bytes_opt()) {
                                let min_str = Self::format_stat_value(min_bytes, physical_type, field.data_type());
                                let max_str = Self::format_stat_value(max_bytes, physical_type, field.data_type());

                                if existing.min_value.is_none() {
                                    existing.min_value = Some(min_str.clone());
                                }
                                if existing.max_value.is_none() {
                                    existing.max_value = Some(max_str.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(stats_map.into_values().collect())
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
                    "Failed to read Parquet metadata: {}",
                    e
                )]));
            }
        }

        // If not quick mode, try to read all data
        if !quick {
            match self.read_batches(&ReadOptions::default()).await {
                Ok(batches) => {
                    let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();
                    report.add_recommendation(format!("Successfully read {} rows", total_rows));
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

        // Configure writer properties
        let mut props_builder = WriterProperties::builder();

        if let Some(compression) = &options.compression {
            let codec = match compression.to_lowercase().as_str() {
                "snappy" => parquet::basic::Compression::SNAPPY,
                "gzip" => parquet::basic::Compression::GZIP(Default::default()),
                "lz4" => parquet::basic::Compression::LZ4,
                "zstd" => parquet::basic::Compression::ZSTD(Default::default()),
                "none" | "uncompressed" => parquet::basic::Compression::UNCOMPRESSED,
                _ => parquet::basic::Compression::SNAPPY, // Default
            };
            props_builder = props_builder.set_compression(codec);
        }

        if options.enable_dictionary {
            props_builder = props_builder.set_dictionary_enabled(true);
        }

        if options.enable_statistics {
            props_builder = props_builder
                .set_statistics_enabled(parquet::file::properties::EnabledStatistics::Page);
        }

        let props = props_builder.build();

        let mut writer = ArrowWriter::try_new(&mut buffer, schema, Some(props))
            .map_err(|e| Error::Parquet(e))?;

        for batch in data {
            writer.write(&batch).map_err(|e| Error::Parquet(e))?;
        }

        writer.close().map_err(|e| Error::Parquet(e))?;

        // Write to storage
        let path_str = self.path.to_string_lossy().to_string();
        let put_options = crate::core::storage::PutOptions::default();
        self.storage
            .put(&path_str, Bytes::from(buffer), &put_options)
            .await?;

        Ok(())
    }

    fn has_native_statistics(&self) -> bool {
        true // Parquet has built-in column statistics
    }
}
