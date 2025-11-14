//! Diff operation - compare two tables

use std::sync::Arc;

use crate::core::formats::FormatHandler;
use crate::error::Result;

/// Operation for comparing two tables
pub struct DiffOperation {
    left_handler: Arc<dyn FormatHandler>,
    right_handler: Arc<dyn FormatHandler>,
}

impl DiffOperation {
    /// Create a new diff operation
    pub fn new(
        left_handler: Arc<dyn FormatHandler>,
        right_handler: Arc<dyn FormatHandler>,
    ) -> Self {
        Self {
            left_handler,
            right_handler,
        }
    }

    /// Execute diff
    pub async fn execute(&self, options: &DiffOptions) -> Result<DiffResult> {
        // TODO: Implement
        todo!("DiffOperation::execute - to be implemented by Rust-Developer")
    }
}

/// Options for diff operation
#[derive(Debug, Clone, Default)]
pub struct DiffOptions {
    pub schema_only: bool,
    pub data_only: bool,
    pub ignore_order: bool,
    pub sample_size: Option<usize>,
}

/// Result of a diff operation
#[derive(Debug)]
pub struct DiffResult {
    // TODO: Define result structure
}
