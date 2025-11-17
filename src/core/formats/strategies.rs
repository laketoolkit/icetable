//! Format-specific strategies for reading and writing data
//!
//! This module defines the strategy pattern interfaces that allow different
//! file formats to plug into the BaseFormatHandler infrastructure.
//!
//! # Architecture
//!
//! Instead of duplicating pagination, projection, and I/O logic across multiple
//! format handlers, we extract format-specific behavior into strategy traits:
//!
//! - `ReaderStrategy`: How to create a reader for a specific format
//! - `WriterStrategy`: How to create a writer for a specific format
//!
//! The `BaseFormatHandler` handles all common operations (reading from storage,
//! applying filters, pagination, etc.) and delegates format-specific operations
//! to the appropriate strategy.

use async_trait::async_trait;
use bytes::Bytes;
use datafusion::arrow::datatypes::Schema;
use datafusion::arrow::record_batch::RecordBatch;

use crate::core::formats::traits::{ColumnStats, WriteOptions};
use crate::error::Result;

/// Strategy for reading a specific file format
///
/// Each format (Arrow, Parquet, CSV, etc.) implements this trait to define
/// how to create readers and extract format-native metadata.
#[async_trait]
pub trait ReaderStrategy: Send + Sync {
    /// Create a batch reader from raw file data
    ///
    /// This is called by BaseFormatHandler after loading the file into memory.
    /// The strategy is responsible for creating a format-specific reader.
    fn create_batch_reader(&self, data: Bytes) -> Result<Box<dyn BatchReader>>;

    /// Extract the schema without reading all data
    ///
    /// This should be efficient and not require loading the entire file.
    fn extract_schema(&self, data: Bytes) -> Result<Schema>;

    /// Check if this format has native column statistics
    ///
    /// Formats like Parquet store statistics in metadata, while others
    /// (like CSV) require scanning the data to compute stats.
    fn has_native_statistics(&self) -> bool {
        false
    }

    /// Extract native statistics from file metadata
    ///
    /// Only called if `has_native_statistics()` returns true.
    /// Default implementation returns empty vec.
    async fn extract_native_statistics(&self, _data: Bytes) -> Result<Vec<ColumnStats>> {
        Ok(Vec::new())
    }

    /// Get magic bytes for format detection (optional)
    ///
    /// Returns the expected magic bytes at the start of files of this format.
    /// Used for fast format detection without file extension.
    fn magic_bytes(&self) -> Option<&[u8]> {
        None
    }

    /// Get common file extensions for this format
    fn file_extensions(&self) -> &[&str];

    /// Format display name
    fn format_name(&self) -> &str;
}

/// Strategy for writing a specific file format
///
/// Each format implements this trait to define how to serialize RecordBatches
/// to the format's binary representation.
pub trait WriterStrategy: Send + Sync {
    /// Create a batch writer that writes to a Vec<u8> buffer
    ///
    /// The writer will be used to serialize one or more RecordBatches.
    /// Returns the writer which will write to the provided buffer.
    fn create_batch_writer<'a>(
        &self,
        buffer: &'a mut Vec<u8>,
        schema: &Schema,
        options: &WriteOptions,
    ) -> Result<Box<dyn BatchWriter + 'a>>;

    /// Format display name
    fn format_name(&self) -> &str;
}

/// Abstraction over format-specific batch readers
///
/// This trait allows BaseFormatHandler to read batches from any format
/// without knowing the specific implementation details.
pub trait BatchReader: Send {
    /// Read the next batch
    ///
    /// Returns `None` when no more batches are available.
    fn next_batch(&mut self) -> Result<Option<RecordBatch>>;

    /// Get the schema of batches returned by this reader
    fn schema(&self) -> &Schema;

    /// Get total number of batches (if known)
    fn num_batches(&self) -> Option<usize> {
        None
    }
}

/// Abstraction over format-specific batch writers
///
/// This trait allows BaseFormatHandler to write batches to any format
/// without knowing the specific implementation details.
pub trait BatchWriter: Send {
    /// Write a batch
    fn write_batch(&mut self, batch: &RecordBatch) -> Result<()>;

    /// Finalize writing and flush any buffered data
    fn finish(&mut self) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verify trait object safety
    #[test]
    fn test_reader_strategy_is_object_safe() {
        let _: Option<Box<dyn ReaderStrategy>> = None;
    }

    #[test]
    fn test_writer_strategy_is_object_safe() {
        let _: Option<Box<dyn WriterStrategy>> = None;
    }
}
