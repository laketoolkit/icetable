//! Arrow IPC format handler implementation
//!
//! This module provides support for reading and writing Arrow IPC (Feather) files.
//!
//! Implementation delegates to BaseFormatHandler using ArrowReaderStrategy and
//! ArrowWriterStrategy for format-specific operations.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use datafusion::arrow::datatypes::Schema;
use datafusion::arrow::record_batch::RecordBatch;

use crate::core::formats::arrow_strategy::{ArrowReaderStrategy, ArrowWriterStrategy};
use crate::core::formats::base::BaseFormatHandler;
use crate::core::formats::traits::*;
use crate::core::storage::StorageBackend;
use crate::error::Result;

/// Handler for Arrow IPC files
///
/// This is a lightweight wrapper around BaseFormatHandler that uses
/// ArrowReaderStrategy and ArrowWriterStrategy for format-specific operations.
pub struct ArrowHandler {
    handler: BaseFormatHandler,
}

impl ArrowHandler {
    /// Create a new Arrow IPC handler
    pub fn new(path: &Path, storage: Arc<dyn StorageBackend>) -> Result<Self> {
        let reader_strategy = Arc::new(ArrowReaderStrategy::new());
        let writer_strategy = Some(Arc::new(ArrowWriterStrategy::new())
            as Arc<dyn crate::core::formats::strategies::WriterStrategy>);

        let handler = BaseFormatHandler::new(
            path.to_path_buf(),
            storage,
            reader_strategy,
            writer_strategy,
        );

        Ok(Self { handler })
    }
}

#[async_trait]
impl FormatHandler for ArrowHandler {
    async fn can_handle(&self, path: &Path) -> Result<bool> {
        self.handler.can_handle(path).await
    }

    fn format_name(&self) -> &str {
        self.handler.format_name()
    }

    async fn read_schema(&self) -> Result<Arc<Schema>> {
        self.handler.read_schema().await
    }

    async fn read_metadata(&self) -> Result<FileMetadata> {
        self.handler.read_metadata().await
    }

    async fn read_batch(&self, options: &ReadOptions) -> Result<RecordBatch> {
        self.handler.read_batch(options).await
    }

    async fn read_batches(&self, options: &ReadOptions) -> Result<Vec<RecordBatch>> {
        self.handler.read_batches(options).await
    }

    async fn read_statistics(&self) -> Result<Vec<ColumnStats>> {
        self.handler.read_statistics().await
    }

    async fn validate(&self, quick: bool) -> Result<ValidationReport> {
        self.handler.validate(quick).await
    }

    async fn write(&self, data: Vec<RecordBatch>, options: &WriteOptions) -> Result<()> {
        self.handler.write(data, options).await
    }

    fn has_native_statistics(&self) -> bool {
        self.handler.has_native_statistics()
    }

    fn supports_write(&self) -> bool {
        self.handler.supports_write()
    }
}
