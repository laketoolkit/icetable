//! JSON/NDJSON format strategy implementation
//!
//! This module implements the ReaderStrategy and WriterStrategy traits for JSON/NDJSON files.

use std::io::Cursor;
use std::sync::Arc;

use arrow_json::{ReaderBuilder, WriterBuilder, writer::LineDelimited};
use bytes::Bytes;
use datafusion::arrow::datatypes::Schema;
use datafusion::arrow::record_batch::RecordBatch;

use crate::core::formats::strategies::{BatchReader, BatchWriter, ReaderStrategy, WriterStrategy};
use crate::core::formats::traits::WriteOptions;
use crate::error::{Error, Result};

/// JSON reader strategy
///
/// Implements reading of JSON and NDJSON (newline-delimited JSON) files.
/// Supports both standard JSON arrays and line-delimited JSON format.
///
/// # Format Details
///
/// - File extensions: `.json`, `.jsonl`, `.ndjson`
/// - Native statistics: Not available (requires scanning)
/// - Schema inference: Infers types from first 100 records
///
/// # Supported Formats
///
/// - **Standard JSON**: Array of objects `[{...}, {...}]`
/// - **NDJSON**: One JSON object per line (newline-delimited)
///
/// # Example
///
/// ```no_run
/// use tabletools::core::formats::json_strategy::JsonReaderStrategy;
/// use tabletools::core::formats::strategies::ReaderStrategy;
///
/// let strategy = JsonReaderStrategy::new()
///     .with_batch_size(4096);
/// # Ok::<(), tabletools::error::Error>(())
/// ```
pub struct JsonReaderStrategy {
    batch_size: usize,
}

impl JsonReaderStrategy {
    /// Create a new JSON reader strategy with default batch size (8192)
    pub fn new() -> Self {
        Self { batch_size: 8192 }
    }

    /// Set custom batch size for reading
    ///
    /// # Arguments
    ///
    /// * `batch_size` - Number of rows per batch
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }
}

impl Default for JsonReaderStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl ReaderStrategy for JsonReaderStrategy {
    fn create_batch_reader(&self, data: Bytes) -> Result<Box<dyn BatchReader>> {
        let schema = self.extract_schema(data.clone())?;
        let schema_arc = Arc::new(schema);
        let cursor = Cursor::new(data);

        let reader = ReaderBuilder::new(schema_arc.clone())
            .with_batch_size(self.batch_size)
            .build(cursor)
            .map_err(|e| Error::General(format!("Failed to create JSON reader: {}", e)))?;

        Ok(Box::new(JsonBatchReader {
            reader,
            schema: schema_arc,
        }))
    }

    fn extract_schema(&self, data: Bytes) -> Result<Schema> {
        let cursor = Cursor::new(data);

        let (schema, _) = arrow_json::reader::infer_json_schema(cursor, Some(100))
            .map_err(|e| Error::General(format!("Failed to infer JSON schema: {}", e)))?;

        Ok(schema)
    }

    fn file_extensions(&self) -> &[&str] {
        &["json", "jsonl", "ndjson"]
    }

    fn format_name(&self) -> &str {
        "JSON/NDJSON"
    }
}

/// JSON batch reader wrapper
///
/// Wraps Arrow JSON reader to provide the `BatchReader` interface.
/// Parses JSON records into Arrow RecordBatches according to inferred schema.
struct JsonBatchReader<R: std::io::BufRead> {
    reader: arrow_json::Reader<R>,
    schema: Arc<Schema>,
}

impl<R: std::io::BufRead + Send> BatchReader for JsonBatchReader<R> {
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

/// JSON writer strategy
///
/// Implements writing RecordBatches to NDJSON (newline-delimited JSON) format.
/// Each row is written as a separate JSON object on its own line.
///
/// # Output Format
///
/// NDJSON format is preferred over standard JSON arrays because:
/// - Streaming friendly (can process line by line)
/// - Easier to append new records
/// - Better for large datasets
/// - Standard for log files and data processing pipelines
///
/// # Example
///
/// ```no_run
/// use tabletools::core::formats::json_strategy::JsonWriterStrategy;
/// use tabletools::core::formats::strategies::WriterStrategy;
///
/// let strategy = JsonWriterStrategy::new();
/// # Ok::<(), tabletools::error::Error>(())
/// ```
pub struct JsonWriterStrategy {
    line_delimited: bool,
}

impl JsonWriterStrategy {
    /// Create a new JSON writer strategy (defaults to NDJSON format)
    pub fn new() -> Self {
        Self {
            line_delimited: true,
        }
    }

    /// Configure line-delimited output format
    ///
    /// # Arguments
    ///
    /// * `line_delimited` - true for NDJSON format (one object per line)
    pub fn with_line_delimited(mut self, line_delimited: bool) -> Self {
        self.line_delimited = line_delimited;
        self
    }
}

impl Default for JsonWriterStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl WriterStrategy for JsonWriterStrategy {
    fn create_batch_writer<'a>(
        &self,
        buffer: &'a mut Vec<u8>,
        _schema: &Schema,
        _options: &WriteOptions,
    ) -> Result<Box<dyn BatchWriter + 'a>> {
        // Always use line-delimited (NDJSON) format
        let writer = WriterBuilder::new().build::<_, LineDelimited>(buffer);

        Ok(Box::new(JsonBatchWriter { writer }))
    }

    fn format_name(&self) -> &str {
        "JSON/NDJSON"
    }
}

/// JSON batch writer wrapper
///
/// Wraps Arrow JSON writer to provide the `BatchWriter` interface.
/// Converts RecordBatches to NDJSON format (one JSON object per line).
struct JsonBatchWriter<W: std::io::Write> {
    writer: arrow_json::Writer<W, LineDelimited>,
}

impl<W: std::io::Write + Send> BatchWriter for JsonBatchWriter<W> {
    fn write_batch(&mut self, batch: &RecordBatch) -> Result<()> {
        self.writer
            .write(batch)
            .map_err(|e| Error::General(format!("Failed to write JSON batch: {}", e)))
    }

    fn finish(&mut self) -> Result<()> {
        self.writer
            .finish()
            .map_err(|e| Error::General(format!("Failed to finish JSON writer: {}", e)))
    }
}
