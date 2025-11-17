//! JSON/NDJSON format handler implementation
//!
//! This module provides support for reading and writing JSON and NDJSON (newline-delimited JSON)
//! files using the arrow-json crate with proper schema inference.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use crate::core::formats::base::BaseFormatHandler;
use crate::core::formats::json_strategy::{JsonReaderStrategy, JsonWriterStrategy};
use crate::core::formats::strategies::{ReaderStrategy, WriterStrategy};
use crate::core::formats::traits::*;
use crate::core::storage::StorageBackend;
use crate::error::Result;

/// Handler for JSON/NDJSON files
pub struct JsonHandler {
    base: BaseFormatHandler,
}

impl JsonHandler {
    /// Create a new JSON handler
    pub fn new(path: &Path, storage: Arc<dyn StorageBackend>) -> Result<Self> {
        let reader_strategy: Arc<dyn ReaderStrategy> = Arc::new(JsonReaderStrategy::new());
        let writer_strategy: Option<Arc<dyn WriterStrategy>> =
            Some(Arc::new(JsonWriterStrategy::new()));

        Ok(Self {
            base: BaseFormatHandler::new(
                PathBuf::from(path),
                storage,
                reader_strategy,
                writer_strategy,
            ),
        })
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

        // Use base handler's logic for extension/magic byte checking
        self.base.can_handle(path).await
    }

    fn format_name(&self) -> &str {
        self.base.format_name()
    }

    async fn read_schema(&self) -> Result<Arc<datafusion::arrow::datatypes::Schema>> {
        self.base.read_schema().await
    }

    async fn read_metadata(&self) -> Result<FileMetadata> {
        self.base.read_metadata().await
    }

    async fn read_batch(
        &self,
        options: &ReadOptions,
    ) -> Result<datafusion::arrow::record_batch::RecordBatch> {
        self.base.read_batch(options).await
    }

    async fn read_batches(
        &self,
        options: &ReadOptions,
    ) -> Result<Vec<datafusion::arrow::record_batch::RecordBatch>> {
        self.base.read_batches(options).await
    }

    async fn read_statistics(&self) -> Result<Vec<ColumnStats>> {
        self.base.read_statistics().await
    }

    async fn validate(&self, quick: bool) -> Result<ValidationReport> {
        self.base.validate(quick).await
    }

    async fn write(
        &self,
        data: Vec<datafusion::arrow::record_batch::RecordBatch>,
        options: &WriteOptions,
    ) -> Result<()> {
        self.base.write(data, options).await
    }

    fn has_native_statistics(&self) -> bool {
        self.base.has_native_statistics()
    }
}

#[cfg(test)]
mod tests {
    

    #[test]
    fn test_can_handle_json_extension() {
        // Test would require mock storage backend
        // Placeholder for when we implement tests
    }
}
