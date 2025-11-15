//! Data transformation utilities for convert operations

use arrow::record_batch::RecordBatch;

use crate::error::Result;

// Module declarations
pub mod cast;
pub mod config;
pub mod filter;
pub mod pipeline;
pub mod project;
pub mod rename;

// Re-export key types
pub use config::TransformConfig;
pub use pipeline::{
    CastStep, CustomTransformStep, FilterStep, ProjectStep, RenameStep, TransformPipeline,
    TransformStep,
};

/// Apply transformations to a record batch
///
/// This is a convenience function that applies transformations based on a
/// `TransformConfig`. For more complex transformation pipelines, use
/// `TransformPipeline` directly.
pub fn apply_transforms(batch: RecordBatch, config: &TransformConfig) -> Result<RecordBatch> {
    let mut current_batch = batch;

    // 1. Apply filtering first (reduces data size early)
    if let Some(filter_expr) = &config.filter_expression {
        current_batch = filter::apply_filter(current_batch, filter_expr)?;
    }

    // 2. Apply type casts
    if !config.cast_columns.is_empty() {
        current_batch = cast::apply_casts(current_batch, &config.cast_columns)?;
    }

    // 3. Apply column selection (projection)
    if let Some(columns) = &config.select_columns {
        current_batch = project::apply_projection(current_batch, columns)?;
    }

    // 4. Apply column renames (must be last as it modifies schema)
    if !config.rename_columns.is_empty() {
        current_batch = rename::apply_renames(current_batch, &config.rename_columns)?;
    }

    Ok(current_batch)
}
