//! Column type casting operations

use datafusion::arrow::compute;
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{Error, Result};

/// Apply type casts to columns
pub fn apply_casts(batch: RecordBatch, casts: &HashMap<String, DataType>) -> Result<RecordBatch> {
    let schema = batch.schema();
    let mut new_columns = Vec::new();
    let mut new_fields = Vec::new();

    for (idx, field) in schema.fields().iter().enumerate() {
        if let Some(target_type) = casts.get(field.name()) {
            // Cast this column
            let array = batch.column(idx);
            let casted = compute::cast(array, target_type).map_err(|e| {
                Error::General(format!(
                    "Failed to cast column '{}' from {:?} to {:?}: {}",
                    field.name(),
                    field.data_type(),
                    target_type,
                    e
                ))
            })?;

            new_columns.push(casted);
            new_fields.push(Arc::new(Field::new(
                field.name(),
                target_type.clone(),
                field.is_nullable(),
            )));
        } else {
            // Keep original column
            new_columns.push(batch.column(idx).clone());
            new_fields.push(field.clone());
        }
    }

    let new_schema = Arc::new(Schema::new(new_fields));

    RecordBatch::try_new(new_schema, new_columns)
        .map_err(|e| Error::General(format!("Failed to apply casts: {}", e)))
}
