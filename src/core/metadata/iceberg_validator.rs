//! Iceberg metadata validation
//!
//! Validates metadata before writing to ensure consistency and correctness.

use iceberg::spec::TableMetadata;

use crate::error::{Error, Result};

/// Validation errors found in metadata
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub code: &'static str,
    pub message: String,
}

/// Result of validating metadata
#[derive(Debug)]
pub struct ValidationResult {
    /// List of validation errors (blocking issues)
    pub errors: Vec<ValidationError>,
    /// List of validation warnings (non-blocking issues)
    pub warnings: Vec<ValidationError>,
}

impl ValidationResult {
    /// Check if validation passed (no errors)
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Create a new empty validation result
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn add_error(&mut self, code: &'static str, message: String) {
        self.errors.push(ValidationError { code, message });
    }

    fn add_warning(&mut self, code: &'static str, message: String) {
        self.warnings.push(ValidationError { code, message });
    }
}

impl Default for ValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Validate table metadata before writing
///
/// Returns Ok if valid, Err with details if invalid.
pub fn validate_metadata(metadata: &TableMetadata) -> Result<ValidationResult> {
    let mut result = ValidationResult::new();

    // 1. Check that current schema exists
    let schema = metadata.current_schema();
    if schema.as_struct().fields().is_empty() {
        result.add_warning("EMPTY_SCHEMA", "Schema has no fields".to_string());
    }

    // 2. Check that partition spec exists and is valid
    let partition_spec = metadata.default_partition_spec();
    for field in partition_spec.fields() {
        // Verify partition field references a valid source column
        let source_id = field.source_id;
        if schema.field_by_id(source_id).is_none() {
            result.add_error(
                "INVALID_PARTITION_FIELD",
                format!(
                    "Partition field '{}' references non-existent source column id {}",
                    field.name, source_id
                ),
            );
        }
    }

    // 3. Check snapshots
    let snapshots: Vec<_> = metadata.snapshots().collect();
    let mut snapshot_ids: std::collections::HashSet<i64> = std::collections::HashSet::new();

    for snapshot in &snapshots {
        let id = snapshot.snapshot_id();

        // Check for duplicate snapshot IDs
        if !snapshot_ids.insert(id) {
            result.add_error(
                "DUPLICATE_SNAPSHOT_ID",
                format!("Duplicate snapshot ID: {}", id),
            );
        }

        // Check parent reference
        if let Some(parent_id) = snapshot.parent_snapshot_id() {
            // Parent should exist (unless it was expired)
            // This is a warning, not an error, because parent can be legitimately missing after expire
            let parent_exists = snapshots.iter().any(|s| s.snapshot_id() == parent_id);
            if !parent_exists && parent_id != 0 {
                result.add_warning(
                    "MISSING_PARENT_SNAPSHOT",
                    format!(
                        "Snapshot {} references parent {} which does not exist (may have been expired)",
                        id, parent_id
                    ),
                );
            }
        }

        // Check manifest list path is not empty
        if snapshot.manifest_list().is_empty() {
            result.add_error(
                "EMPTY_MANIFEST_LIST",
                format!("Snapshot {} has empty manifest list path", id),
            );
        }
    }

    // 4. Check current snapshot reference
    if let Some(current_id) = metadata.current_snapshot_id()
        && !snapshot_ids.contains(&current_id) {
            result.add_error(
                "INVALID_CURRENT_SNAPSHOT",
                format!(
                    "Current snapshot {} does not exist in snapshots list",
                    current_id
                ),
            );
        }

    // 5. Check refs point to valid snapshots
    // Note: metadata.refs() is pub(crate) in iceberg-rs, so we skip this for now

    // 6. Check sort orders reference valid columns
    for sort_order in metadata.sort_orders_iter() {
        for field in sort_order.fields.iter() {
            let source_id = field.source_id;
            if schema.field_by_id(source_id).is_none() {
                result.add_error(
                    "INVALID_SORT_FIELD",
                    format!(
                        "Sort order field references non-existent source column id {}",
                        source_id
                    ),
                );
            }
        }
    }

    Ok(result)
}

/// Validate metadata and return error if invalid
pub fn validate_or_error(metadata: &TableMetadata) -> Result<()> {
    let result = validate_metadata(metadata)?;

    if !result.is_valid() {
        let error_messages: Vec<String> = result
            .errors
            .iter()
            .map(|e| format!("[{}] {}", e.code, e.message))
            .collect();

        return Err(Error::General(format!(
            "Metadata validation failed:\n{}",
            error_messages.join("\n")
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    

    // Tests would go here - omitted for brevity
}
