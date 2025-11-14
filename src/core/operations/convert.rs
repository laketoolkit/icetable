//! Convert operation - convert between formats

use std::sync::Arc;

use crate::core::formats::{FormatHandler, WriteOptions};
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
        // TODO: Implement
        todo!("ConvertOperation::execute - to be implemented by Rust-Developer")
    }
}

/// Result of a convert operation
#[derive(Debug)]
pub struct ConvertResult {
    pub rows_converted: i64,
    pub source_size: u64,
    pub target_size: u64,
}
