//! CSV format strategy implementation
//!
//! This module implements the ReaderStrategy and WriterStrategy traits for CSV files.

use std::io::Cursor;
use std::sync::Arc;

use arrow_csv::{ReaderBuilder, WriterBuilder};
use bytes::Bytes;
use datafusion::arrow::datatypes::Schema;
use datafusion::arrow::record_batch::RecordBatch;

use crate::core::formats::strategies::{BatchReader, BatchWriter, ReaderStrategy, WriterStrategy};
use crate::core::formats::traits::WriteOptions;
use crate::error::{Error, Result};

/// CSV reader strategy
///
/// Implements reading of CSV (Comma-Separated Values) files with configurable
/// options for headers and batch sizes.
///
/// # Format Details
///
/// - File extensions: `.csv`
/// - Native statistics: Not available (requires scanning)
/// - Schema inference: Infers types from first 100 rows
///
/// # Configuration
///
/// - Header row: Configurable (default: true)
/// - Batch size: Configurable (default: 8192 rows)
///
/// # Example
///
/// ```no_run
/// use tabletools::core::formats::csv_strategy::CsvReaderStrategy;
/// use tabletools::core::formats::strategies::ReaderStrategy;
///
/// let strategy = CsvReaderStrategy::new()
///     .with_batch_size(4096)
///     .with_header(true);
/// # Ok::<(), tabletools::error::Error>(())
/// ```
pub struct CsvReaderStrategy {
    batch_size: usize,
    has_header: bool,
}

impl CsvReaderStrategy {
    /// Create a new CSV reader strategy with default settings
    ///
    /// Defaults: batch_size=8192, has_header=true
    pub fn new() -> Self {
        Self {
            batch_size: 8192,
            has_header: true,
        }
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

    /// Configure whether the CSV file has a header row
    ///
    /// # Arguments
    ///
    /// * `has_header` - true if first row contains column names
    pub fn with_header(mut self, has_header: bool) -> Self {
        self.has_header = has_header;
        self
    }
}

impl Default for CsvReaderStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl ReaderStrategy for CsvReaderStrategy {
    fn create_batch_reader(&self, data: Bytes) -> Result<Box<dyn BatchReader>> {
        let schema = self.extract_schema(data.clone())?;
        let schema_arc = Arc::new(schema);
        let cursor = Cursor::new(data);

        let reader = ReaderBuilder::new(schema_arc.clone())
            .with_header(self.has_header)
            .with_batch_size(self.batch_size)
            .build(cursor)
            .map_err(|e| Error::General(format!("Failed to create CSV reader: {}", e)))?;

        Ok(Box::new(CsvBatchReader {
            reader,
            schema: schema_arc,
        }))
    }

    fn extract_schema(&self, data: Bytes) -> Result<Schema> {
        let cursor = Cursor::new(data);

        let (schema, _) = arrow_csv::reader::Format::default()
            .with_header(self.has_header)
            .infer_schema(cursor, Some(100))
            .map_err(|e| Error::General(format!("Failed to infer CSV schema: {}", e)))?;

        Ok(schema)
    }

    fn file_extensions(&self) -> &[&str] {
        &["csv"]
    }

    fn format_name(&self) -> &str {
        "CSV"
    }
}

/// CSV batch reader wrapper
///
/// Wraps Arrow CSV reader to provide the `BatchReader` interface.
/// Parses CSV rows into Arrow RecordBatches according to inferred schema.
struct CsvBatchReader<R: std::io::Read> {
    reader: arrow_csv::Reader<R>,
    schema: Arc<Schema>,
}

impl<R: std::io::Read + Send> BatchReader for CsvBatchReader<R> {
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

/// CSV writer strategy
///
/// Implements writing RecordBatches to CSV format.
///
/// # Configuration
///
/// - Header row: Configurable (default: true - writes column names as first row)
///
/// # Example
///
/// ```no_run
/// use tabletools::core::formats::csv_strategy::CsvWriterStrategy;
/// use tabletools::core::formats::strategies::WriterStrategy;
///
/// let strategy = CsvWriterStrategy::new()
///     .with_header(true);
/// # Ok::<(), tabletools::error::Error>(())
/// ```
pub struct CsvWriterStrategy {
    has_header: bool,
}

impl CsvWriterStrategy {
    /// Create a new CSV writer strategy with header enabled
    pub fn new() -> Self {
        Self { has_header: true }
    }

    /// Configure whether to write a header row
    ///
    /// # Arguments
    ///
    /// * `has_header` - true to write column names as first row
    pub fn with_header(mut self, has_header: bool) -> Self {
        self.has_header = has_header;
        self
    }
}

impl Default for CsvWriterStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl WriterStrategy for CsvWriterStrategy {
    fn create_batch_writer<'a>(
        &self,
        buffer: &'a mut Vec<u8>,
        _schema: &Schema,
        _options: &WriteOptions,
    ) -> Result<Box<dyn BatchWriter + 'a>> {
        let writer = WriterBuilder::new()
            .with_header(self.has_header)
            .build(buffer);

        Ok(Box::new(CsvBatchWriter { writer }))
    }

    fn format_name(&self) -> &str {
        "CSV"
    }
}

/// CSV batch writer wrapper
///
/// Wraps Arrow CSV writer to provide the `BatchWriter` interface.
/// Converts RecordBatches to CSV text format.
struct CsvBatchWriter<W: std::io::Write> {
    writer: arrow_csv::Writer<W>,
}

impl<W: std::io::Write + Send> BatchWriter for CsvBatchWriter<W> {
    fn write_batch(&mut self, batch: &RecordBatch) -> Result<()> {
        self.writer
            .write(batch)
            .map_err(|e| Error::General(format!("Failed to write CSV batch: {}", e)))
    }

    fn finish(&mut self) -> Result<()> {
        // CSV writer doesn't require explicit finalization
        Ok(())
    }
}
