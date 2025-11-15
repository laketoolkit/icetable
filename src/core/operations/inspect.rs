//! Inspect operation - view contents and metadata of tables

use std::sync::Arc;

use datafusion::arrow::datatypes::Schema;
use datafusion::arrow::record_batch::RecordBatch;
use serde::Serialize;

use crate::core::formats::{ColumnStats, FileMetadata, FormatHandler, ReadOptions};
use crate::error::Result;

/// Options for inspect operation
#[derive(Debug, Clone)]
pub struct InspectOptions {
    /// Show only schema (no data) - deprecated, use show_data instead
    pub schema_only: bool,

    /// Show schema
    pub show_schema: bool,

    /// Show metadata
    pub show_metadata: bool,

    /// Show statistics
    pub show_stats: bool,

    /// Show data preview
    pub show_data: bool,

    /// Number of rows to sample
    pub num_rows: usize,

    /// Columns to include
    pub columns: Option<Vec<String>>,

    /// Use random sampling
    pub sample: bool,
}

impl Default for InspectOptions {
    fn default() -> Self {
        Self {
            schema_only: false,
            show_schema: true,
            show_metadata: true,
            show_stats: false,
            show_data: true,
            num_rows: 10,
            columns: None,
            sample: false,
        }
    }
}

/// Operation for inspecting table contents
pub struct InspectOperation {
    handler: Box<dyn FormatHandler>,
}

impl InspectOperation {
    /// Create a new inspect operation
    pub fn new(handler: Box<dyn FormatHandler>) -> Self {
        Self { handler }
    }

    /// Execute the inspect operation
    pub async fn execute(&self, options: &InspectOptions) -> Result<InspectResult> {
        // Always read schema
        let schema = self.handler.read_schema().await?;

        // Read metadata if requested
        let metadata = if options.show_metadata {
            Some(self.handler.read_metadata().await?)
        } else {
            None
        };

        // Read statistics if requested
        let statistics = if options.show_stats {
            Some(self.handler.read_statistics().await?)
        } else {
            None
        };

        // Read sample data if requested
        let sample_data = if options.show_data && !options.schema_only {
            let mut read_opts_builder = ReadOptions::builder()
                .limit(options.num_rows)
                .sample(options.sample)
                .batch_size(1024);

            if let Some(cols) = options.columns.clone() {
                read_opts_builder = read_opts_builder.columns(cols);
            }

            let read_opts = read_opts_builder.build();

            Some(self.handler.read_batch(&read_opts).await?)
        } else {
            None
        };

        Ok(InspectResult {
            format_name: self.handler.format_name().to_string(),
            schema,
            metadata,
            statistics,
            sample_data,
        })
    }
}

/// Result of an inspect operation
#[derive(Debug, Serialize)]
pub struct InspectResult {
    /// Format name (e.g., "Apache Parquet")
    pub format_name: String,

    /// Table schema
    #[serde(skip)]
    pub schema: Arc<Schema>,

    /// File metadata (if requested)
    #[serde(skip)]
    pub metadata: Option<FileMetadata>,

    /// Column statistics (if requested)
    #[serde(skip)]
    pub statistics: Option<Vec<ColumnStats>>,

    /// Sample data (if requested)
    #[serde(skip)]
    pub sample_data: Option<RecordBatch>,
}
