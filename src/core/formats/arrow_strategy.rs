//! Arrow IPC format strategies
//!
//! This module provides ReaderStrategy and WriterStrategy implementations
//! for Apache Arrow IPC (Feather) format.

use std::io::Cursor;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use datafusion::arrow::datatypes::Schema;
use datafusion::arrow::ipc::reader::FileReader as ArrowFileReader;
use datafusion::arrow::ipc::writer::FileWriter as ArrowFileWriter;
use datafusion::arrow::record_batch::RecordBatch;

use crate::core::formats::strategies::{BatchReader, BatchWriter, ReaderStrategy, WriterStrategy};
use crate::core::formats::traits::WriteOptions;
use crate::error::{Error, Result};

/// Reader strategy for Arrow IPC format
///
/// Implements reading of Apache Arrow IPC (Inter-Process Communication) files,
/// also known as Feather format. This format is designed for efficient in-memory
/// columnar data representation and serialization.
///
/// # Format Details
///
/// - Magic bytes: `ARROW1`
/// - File extensions: `.arrow`, `.feather`
/// - Native statistics: Not available
///
/// # Example
///
/// ```no_run
/// use tablectl::core::formats::arrow_strategy::ArrowReaderStrategy;
/// use tablectl::core::formats::strategies::ReaderStrategy;
/// use bytes::Bytes;
///
/// let strategy = ArrowReaderStrategy::new();
/// let data: Bytes = /* read Arrow IPC file */
/// # Bytes::new();
/// let schema = strategy.extract_schema(data)?;
/// # Ok::<(), tablectl::error::Error>(())
/// ```
pub struct ArrowReaderStrategy;

impl ArrowReaderStrategy {
    /// Create a new Arrow IPC reader strategy
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ReaderStrategy for ArrowReaderStrategy {
    fn create_batch_reader(&self, data: Bytes) -> Result<Box<dyn BatchReader>> {
        let cursor = Cursor::new(data);
        let reader = ArrowFileReader::try_new(cursor, None)
            .map_err(|e| Error::General(format!("Failed to create Arrow IPC reader: {}", e)))?;

        Ok(Box::new(ArrowBatchReader::new(reader)))
    }

    fn extract_schema(&self, data: Bytes) -> Result<Schema> {
        let cursor = Cursor::new(data);
        let reader = ArrowFileReader::try_new(cursor, None)
            .map_err(|e| Error::General(format!("Failed to read Arrow IPC schema: {}", e)))?;

        Ok((*reader.schema()).clone())
    }

    fn has_native_statistics(&self) -> bool {
        false // Arrow IPC doesn't have built-in column statistics
    }

    fn magic_bytes(&self) -> Option<&[u8]> {
        Some(b"ARROW1") // Arrow IPC magic bytes
    }

    fn file_extensions(&self) -> &[&str] {
        &["arrow", "feather"]
    }

    fn format_name(&self) -> &str {
        "Apache Arrow IPC"
    }
}

/// Batch reader implementation for Arrow IPC
///
/// Wraps Arrow's native `FileReader` to provide the `BatchReader` interface.
/// Reads batches sequentially from the Arrow IPC file.
struct ArrowBatchReader {
    reader: ArrowFileReader<Cursor<Bytes>>,
    schema: Arc<Schema>,
}

impl ArrowBatchReader {
    /// Create a new batch reader from an Arrow IPC file reader
    fn new(reader: ArrowFileReader<Cursor<Bytes>>) -> Self {
        let schema = reader.schema();
        Self { reader, schema }
    }
}

impl BatchReader for ArrowBatchReader {
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

/// Writer strategy for Arrow IPC format
///
/// Implements writing RecordBatches to Apache Arrow IPC format.
/// This format preserves exact Arrow types and metadata without conversion.
///
/// # Format Features
///
/// - Zero-copy deserialization when used with Arrow libraries
/// - Preserves all Arrow data types including nested types
/// - Supports metadata attached to schema and columns
/// - Efficient for local storage and IPC scenarios
///
/// # Example
///
/// ```no_run
/// use tablectl::core::formats::arrow_strategy::ArrowWriterStrategy;
/// use tablectl::core::formats::strategies::WriterStrategy;
/// use tablectl::core::formats::traits::WriteOptions;
/// use datafusion::arrow::datatypes::{Schema, Field, DataType};
/// use std::sync::Arc;
///
/// let strategy = ArrowWriterStrategy::new();
/// let mut buffer = Vec::new();
/// let schema = Schema::new(vec![Field::new("id", DataType::Int32, false)]);
/// let options = WriteOptions::default();
///
/// let writer = strategy.create_batch_writer(&mut buffer, &schema, &options)?;
/// # Ok::<(), tablectl::error::Error>(())
/// ```
pub struct ArrowWriterStrategy;

impl ArrowWriterStrategy {
    /// Create a new Arrow IPC writer strategy
    pub fn new() -> Self {
        Self
    }
}

impl WriterStrategy for ArrowWriterStrategy {
    fn create_batch_writer<'a>(
        &self,
        buffer: &'a mut Vec<u8>,
        schema: &Schema,
        _options: &WriteOptions,
    ) -> Result<Box<dyn BatchWriter + 'a>> {
        let writer = ArrowFileWriter::try_new(buffer, schema)
            .map_err(|e| Error::General(format!("Failed to create Arrow IPC writer: {}", e)))?;

        Ok(Box::new(ArrowBatchWriter { writer }))
    }

    fn format_name(&self) -> &str {
        "Apache Arrow IPC"
    }
}

/// Batch writer implementation for Arrow IPC
///
/// Wraps Arrow's native `FileWriter` to provide the `BatchWriter` interface.
/// Buffers batches and writes them in Arrow IPC format when `finish()` is called.
struct ArrowBatchWriter<'a> {
    writer: ArrowFileWriter<&'a mut Vec<u8>>,
}

impl<'a> BatchWriter for ArrowBatchWriter<'a> {
    fn write_batch(&mut self, batch: &RecordBatch) -> Result<()> {
        self.writer
            .write(batch)
            .map_err(|e| Error::General(format!("Failed to write Arrow IPC batch: {}", e)))
    }

    fn finish(&mut self) -> Result<()> {
        self.writer
            .finish()
            .map_err(|e| Error::General(format!("Failed to finish Arrow IPC writer: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    
    

    #[test]
    fn test_arrow_reader_strategy_extensions() {
        let strategy = ArrowReaderStrategy::new();
        assert_eq!(strategy.file_extensions(), &["arrow", "feather"]);
    }

    #[test]
    fn test_arrow_reader_strategy_magic_bytes() {
        let strategy = ArrowReaderStrategy::new();
        assert_eq!(strategy.magic_bytes(), Some(b"ARROW1" as &[u8]));
    }

    #[test]
    fn test_arrow_reader_strategy_format_name() {
        let strategy = ArrowReaderStrategy::new();
        assert_eq!(strategy.format_name(), "Apache Arrow IPC");
    }

    #[test]
    fn test_arrow_no_native_stats() {
        let strategy = ArrowReaderStrategy::new();
        assert!(!strategy.has_native_statistics());
    }
}
