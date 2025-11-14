//! Convert operation - convert between formats

use std::sync::Arc;

use serde::Serialize;

use crate::core::formats::{FormatHandler, ReadOptions, WriteOptions};
use crate::error::Result;

/// Operation for converting between formats
pub struct ConvertOperation {
    source_handler: Arc<dyn FormatHandler>,
    target_handler: Arc<dyn FormatHandler>,
}

impl ConvertOperation {
    /// Create a new convert operation
    pub fn new(
        source_handler: Arc<dyn FormatHandler>,
        target_handler: Arc<dyn FormatHandler>,
    ) -> Self {
        Self {
            source_handler,
            target_handler,
        }
    }

    /// Execute conversion
    pub async fn execute(&self, options: &WriteOptions) -> Result<ConvertResult> {
        let source_format = self.source_handler.format_name().to_string();
        let target_format = self.target_handler.format_name().to_string();

        // Get source metadata
        let source_metadata = self.source_handler.read_metadata().await?;
        let source_size = source_metadata.compressed_size.unwrap_or(0);

        // Read all batches from source
        let read_options = ReadOptions::default();
        let batches = self.source_handler.read_batches(&read_options).await?;

        // Count total rows
        let rows_converted = batches.iter().map(|b| b.num_rows() as i64).sum();

        // Write to target format
        self.target_handler.write(batches, options).await?;

        // Get target metadata
        let target_metadata = self.target_handler.read_metadata().await?;
        let target_size = target_metadata.compressed_size.unwrap_or(0);

        Ok(ConvertResult {
            source_format,
            target_format,
            rows_converted,
            source_size,
            target_size,
            compression_ratio: if source_size > 0 {
                Some((target_size as f64 / source_size as f64) * 100.0)
            } else {
                None
            },
        })
    }
}

/// Result of a convert operation
#[derive(Debug, Clone, Serialize)]
pub struct ConvertResult {
    /// Source format name
    pub source_format: String,

    /// Target format name
    pub target_format: String,

    /// Number of rows converted
    pub rows_converted: i64,

    /// Source file size in bytes
    pub source_size: u64,

    /// Target file size in bytes
    pub target_size: u64,

    /// Compression ratio (target/source * 100)
    pub compression_ratio: Option<f64>,
}
