//! Parquet file utilities
//!
//! Common operations for reading parquet file metadata.

use bytes::Bytes;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

/// Read record count from a parquet file's metadata
///
/// Returns 0 if the file cannot be read or parsed.
pub fn read_parquet_record_count(path: &str) -> u64 {
    match std::fs::read(path) {
        Ok(data) => match ParquetRecordBatchReaderBuilder::try_new(Bytes::from(data)) {
            Ok(builder) => builder.metadata().file_metadata().num_rows() as u64,
            Err(_) => 0,
        },
        Err(_) => 0,
    }
}

/// Read record count from a parquet file using a file handle
///
/// More efficient for large files as it only reads metadata.
pub fn read_parquet_record_count_from_file(path: &std::path::Path) -> Option<u64> {
    let file = std::fs::File::open(path).ok()?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file).ok()?;
    Some(reader.metadata().file_metadata().num_rows() as u64)
}
