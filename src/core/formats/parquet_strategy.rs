//! Parquet format strategy implementation
//!
//! This module implements the ReaderStrategy and WriterStrategy traits for Apache Parquet files.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use datafusion::arrow::datatypes::{DataType, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::parquet::arrow::ArrowWriter;
use datafusion::parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use datafusion::parquet::basic::Type as PhysicalType;
use datafusion::parquet::file::properties::WriterProperties;
use datafusion::parquet::file::reader::{FileReader, SerializedFileReader};

use crate::core::formats::strategies::{BatchReader, BatchWriter, ReaderStrategy, WriterStrategy};
use crate::core::formats::traits::{ColumnStats, WriteOptions};
use crate::error::{Error, Result};

/// Parquet reader strategy
///
/// Implements reading of Apache Parquet files, a columnar storage format optimized
/// for analytics. Parquet files include built-in column statistics, compression,
/// and encoding schemes.
///
/// # Format Details
///
/// - Magic bytes: `PAR1`
/// - File extensions: `.parquet`
/// - Native statistics: Available (min, max, null_count per column)
/// - Compression: Supports Snappy, GZIP, LZ4, ZSTD
///
/// # Performance
///
/// The batch size can be configured to optimize memory usage and throughput.
/// Default batch size is 1024 rows.
///
/// # Example
///
/// ```no_run
/// use tabletools::core::formats::parquet_strategy::ParquetReaderStrategy;
/// use tabletools::core::formats::strategies::ReaderStrategy;
///
/// let strategy = ParquetReaderStrategy::new()
///     .with_batch_size(2048);
/// # Ok::<(), tabletools::error::Error>(())
/// ```
pub struct ParquetReaderStrategy {
    batch_size: usize,
}

impl ParquetReaderStrategy {
    /// Create a new Parquet reader strategy with default batch size (1024)
    pub fn new() -> Self {
        Self { batch_size: 1024 }
    }

    /// Set custom batch size for reading
    ///
    /// # Arguments
    ///
    /// * `batch_size` - Number of rows per batch (typical values: 1024-8192)
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Helper function to convert Parquet statistics bytes to readable strings
    ///
    /// Parquet stores statistics as raw bytes. This function decodes them based
    /// on the physical type to produce human-readable values.
    ///
    /// # Arguments
    ///
    /// * `bytes` - Raw bytes from Parquet statistics
    /// * `physical_type` - Parquet physical type (INT32, DOUBLE, BYTE_ARRAY, etc.)
    /// * `_data_type` - Arrow logical type (currently unused, reserved for future use)
    ///
    /// # Returns
    ///
    /// String representation of the statistical value
    fn format_stat_value(
        bytes: &[u8],
        physical_type: PhysicalType,
        _data_type: &DataType,
    ) -> String {
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
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                        bytes[7],
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
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                        bytes[7],
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

impl Default for ParquetReaderStrategy {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ReaderStrategy for ParquetReaderStrategy {
    fn create_batch_reader(&self, data: Bytes) -> Result<Box<dyn BatchReader>> {
        let builder =
            ParquetRecordBatchReaderBuilder::try_new(data).map_err(|e| Error::Parquet(e))?;

        let schema = builder.schema().clone();

        let reader = builder
            .with_batch_size(self.batch_size)
            .build()
            .map_err(|e| Error::Parquet(e))?;

        Ok(Box::new(ParquetBatchReader { reader, schema }))
    }

    fn extract_schema(&self, data: Bytes) -> Result<Schema> {
        let reader = SerializedFileReader::new(data).map_err(|e| Error::Parquet(e))?;

        let parquet_schema = reader.metadata().file_metadata().schema_descr();
        let arrow_schema =
            datafusion::parquet::arrow::parquet_to_arrow_schema(parquet_schema, None)
                .map_err(|e| Error::Parquet(e))?;

        Ok(arrow_schema)
    }

    fn has_native_statistics(&self) -> bool {
        true // Parquet has built-in column statistics
    }

    async fn extract_native_statistics(&self, data: Bytes) -> Result<Vec<ColumnStats>> {
        let reader = SerializedFileReader::new(data.clone()).map_err(|e| Error::Parquet(e))?;

        let parquet_metadata = reader.metadata();

        // Get schema
        let parquet_schema = parquet_metadata.file_metadata().schema_descr();
        let schema = datafusion::parquet::arrow::parquet_to_arrow_schema(parquet_schema, None)
            .map_err(|e| Error::Parquet(e))?;

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
                            // Aggregate null count
                            if let Some(null_count) = existing.null_count {
                                existing.null_count =
                                    Some(null_count + stats.null_count_opt().unwrap_or(0) as i64);
                            }

                            // Update min/max values if available
                            if let (Some(min_bytes), Some(max_bytes)) =
                                (stats.min_bytes_opt(), stats.max_bytes_opt())
                            {
                                let min_str = Self::format_stat_value(
                                    min_bytes,
                                    physical_type,
                                    field.data_type(),
                                );
                                let max_str = Self::format_stat_value(
                                    max_bytes,
                                    physical_type,
                                    field.data_type(),
                                );

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

    fn magic_bytes(&self) -> Option<&[u8]> {
        Some(b"PAR1")
    }

    fn file_extensions(&self) -> &[&str] {
        &["parquet"]
    }

    fn format_name(&self) -> &str {
        "Apache Parquet"
    }
}

/// Parquet batch reader wrapper
///
/// Wraps DataFusion's ParquetRecordBatchReader to provide the `BatchReader` interface.
/// Reads batches from row groups in the Parquet file.
struct ParquetBatchReader {
    reader: datafusion::parquet::arrow::arrow_reader::ParquetRecordBatchReader,
    schema: Arc<Schema>,
}

impl BatchReader for ParquetBatchReader {
    fn next_batch(&mut self) -> Result<Option<RecordBatch>> {
        match self.reader.next() {
            Some(Ok(batch)) => Ok(Some(batch)),
            Some(Err(e)) => Err(Error::Arrow(e)),
            None => Ok(None),
        }
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }
}

/// Parquet writer strategy
///
/// Implements writing RecordBatches to Parquet format with configurable compression
/// and encoding options.
///
/// # Compression Options
///
/// - `snappy` (default) - Fast compression, moderate ratio
/// - `gzip` - Slower compression, better ratio
/// - `lz4` - Very fast compression
/// - `zstd` - Balanced speed and compression ratio
/// - `none` / `uncompressed` - No compression
///
/// # Example
///
/// ```no_run
/// use tabletools::core::formats::parquet_strategy::ParquetWriterStrategy;
/// use tabletools::core::formats::strategies::WriterStrategy;
///
/// let strategy = ParquetWriterStrategy::new()
///     .with_compression(Some("zstd".to_string()));
/// # Ok::<(), tabletools::error::Error>(())
/// ```
pub struct ParquetWriterStrategy {
    compression: Option<String>,
}

impl ParquetWriterStrategy {
    /// Create a new Parquet writer strategy with Snappy compression
    pub fn new() -> Self {
        Self {
            compression: Some("snappy".to_string()),
        }
    }

    /// Set compression codec
    ///
    /// # Arguments
    ///
    /// * `compression` - Codec name (`snappy`, `gzip`, `lz4`, `zstd`, `none`)
    pub fn with_compression(mut self, compression: Option<String>) -> Self {
        self.compression = compression;
        self
    }
}

impl Default for ParquetWriterStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl WriterStrategy for ParquetWriterStrategy {
    fn create_batch_writer<'a>(
        &self,
        buffer: &'a mut Vec<u8>,
        schema: &Schema,
        options: &WriteOptions,
    ) -> Result<Box<dyn BatchWriter + 'a>> {
        // Configure writer properties
        let mut props_builder = WriterProperties::builder();

        // Use compression from options if available, otherwise use strategy default
        let compression = if let Some(c) = options.compression() {
            Some(c.as_str())
        } else {
            self.compression.as_deref()
        };

        if let Some(compression_str) = compression {
            let codec = match compression_str.to_lowercase().as_str() {
                "snappy" => datafusion::parquet::basic::Compression::SNAPPY,
                "gzip" => datafusion::parquet::basic::Compression::GZIP(Default::default()),
                "lz4" => datafusion::parquet::basic::Compression::LZ4,
                "zstd" => datafusion::parquet::basic::Compression::ZSTD(Default::default()),
                "none" | "uncompressed" => datafusion::parquet::basic::Compression::UNCOMPRESSED,
                _ => datafusion::parquet::basic::Compression::SNAPPY, // Default
            };
            props_builder = props_builder.set_compression(codec);
        }

        if options.enable_dictionary() {
            props_builder = props_builder.set_dictionary_enabled(true);
        }

        if options.enable_statistics() {
            props_builder = props_builder.set_statistics_enabled(
                datafusion::parquet::file::properties::EnabledStatistics::Page,
            );
        }

        let props = props_builder.build();

        let writer = ArrowWriter::try_new(buffer, Arc::new(schema.clone()), Some(props))
            .map_err(|e| Error::Parquet(e))?;

        Ok(Box::new(ParquetBatchWriter {
            writer: Some(writer),
        }))
    }

    fn format_name(&self) -> &str {
        "Apache Parquet"
    }
}

/// Parquet batch writer wrapper
///
/// Wraps DataFusion's ArrowWriter to provide the `BatchWriter` interface.
/// The writer is wrapped in Option to allow taking ownership during `finish()`.
struct ParquetBatchWriter<W: std::io::Write> {
    writer: Option<ArrowWriter<W>>,
}

impl<W: std::io::Write + Send> BatchWriter for ParquetBatchWriter<W> {
    fn write_batch(&mut self, batch: &RecordBatch) -> Result<()> {
        if let Some(ref mut writer) = self.writer {
            writer.write(batch).map_err(|e| Error::Parquet(e))
        } else {
            Err(Error::General("Writer already closed".to_string()))
        }
    }

    fn finish(&mut self) -> Result<()> {
        if let Some(writer) = self.writer.take() {
            writer.close().map_err(|e| Error::Parquet(e))?;
        }
        Ok(())
    }
}
