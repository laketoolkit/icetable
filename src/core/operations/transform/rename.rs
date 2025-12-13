//! Column renaming operations

use arrow::datatypes::{Field, Schema};
use arrow::record_batch::RecordBatch;
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{Error, Result};

/// Apply column renames
pub fn apply_renames(batch: RecordBatch, renames: &HashMap<String, String>) -> Result<RecordBatch> {
    let schema = batch.schema();

    // Create new schema with renamed fields
    let new_fields: Vec<Arc<Field>> = schema
        .fields()
        .iter()
        .map(|field| {
            if let Some(new_name) = renames.get(field.name()) {
                Arc::new(Field::new(
                    new_name,
                    field.data_type().clone(),
                    field.is_nullable(),
                ))
            } else {
                field.clone()
            }
        })
        .collect();

    let new_schema = Arc::new(Schema::new(new_fields));

    RecordBatch::try_new(new_schema, batch.columns().to_vec()).map_err(|e| {
        Error::SchemaValidation {
            message: format!("Failed to rename columns: {}", e),
        }
    })
}
