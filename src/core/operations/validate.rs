//! Validate operation - check file integrity and quality

use std::sync::Arc;

use serde::Serialize;

use crate::core::formats::{FormatHandler, ValidationReport};
use crate::error::Result;

/// Operation for validating tables
pub struct ValidateOperation {
    handler: Arc<dyn FormatHandler>,
}

impl ValidateOperation {
    /// Create a new validate operation
    pub fn new(handler: Arc<dyn FormatHandler>) -> Self {
        Self { handler }
    }

    /// Execute validation
    pub async fn execute(&self, quick: bool) -> Result<ValidateResult> {
        let format_name = self.handler.format_name().to_string();

        // Run validation using the handler
        let report = self.handler.validate(quick).await?;

        // Get basic metadata if validation passed
        let metadata = if report.is_valid {
            self.handler.read_metadata().await.ok()
        } else {
            None
        };

        Ok(ValidateResult {
            format_name,
            is_valid: report.is_valid,
            errors: report.errors,
            warnings: report.warnings,
            recommendations: report.recommendations,
            num_rows: metadata.as_ref().and_then(|m| m.num_rows),
            file_size: metadata.as_ref().and_then(|m| m.compressed_size),
            quick_mode: quick,
        })
    }
}

/// Result of a validate operation
#[derive(Debug, Clone, Serialize)]
pub struct ValidateResult {
    /// Format name
    pub format_name: String,

    /// Whether the file is valid
    pub is_valid: bool,

    /// List of errors found
    pub errors: Vec<String>,

    /// List of warnings
    pub warnings: Vec<String>,

    /// List of recommendations
    pub recommendations: Vec<String>,

    /// Number of rows (if available)
    pub num_rows: Option<i64>,

    /// File size in bytes (if available)
    pub file_size: Option<u64>,

    /// Whether quick validation was performed
    pub quick_mode: bool,
}

impl ValidateResult {
    /// Create a successful validation result
    pub fn success(format_name: String) -> Self {
        Self {
            format_name,
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            recommendations: Vec::new(),
            num_rows: None,
            file_size: None,
            quick_mode: false,
        }
    }

    /// Create a failed validation result
    pub fn failed(format_name: String, errors: Vec<String>) -> Self {
        Self {
            format_name,
            is_valid: false,
            errors,
            warnings: Vec::new(),
            recommendations: Vec::new(),
            num_rows: None,
            file_size: None,
            quick_mode: false,
        }
    }
}
