//! Column projection operations

use arrow::array::Array;
use arrow::datatypes::{Field, Schema};
use arrow::record_batch::RecordBatch;
use std::sync::Arc;

use crate::error::{Error, Result};

/// Apply column projection (select specific columns)
pub fn apply_projection(batch: RecordBatch, columns: &[String]) -> Result<RecordBatch> {
    let schema = batch.schema();

    // Find indices of requested columns
    let mut indices = Vec::new();
    for col_name in columns {
        match schema.index_of(col_name) {
            Ok(idx) => indices.push(idx),
            Err(_) => {
                return Err(Error::General(format!(
                    "Column '{}' not found in schema. Available columns: {:?}",
                    col_name,
                    schema.fields().iter().map(|f| f.name()).collect::<Vec<_>>()
                )));
            }
        }
    }

    // Project the batch
    let projected_columns: Vec<Arc<dyn Array>> = indices
        .iter()
        .map(|&idx| batch.column(idx).clone())
        .collect();

    let projected_fields: Vec<Arc<Field>> = indices
        .iter()
        .map(|&idx| Arc::new(schema.field(idx).clone()))
        .collect();

    let projected_schema = Arc::new(Schema::new(projected_fields));

    RecordBatch::try_new(projected_schema, projected_columns)
        .map_err(|e| Error::General(format!("Failed to create projected batch: {}", e)))
}
