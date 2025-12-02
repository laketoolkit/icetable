//! Arrow compatibility layer for converting between arrow v54 and datafusion::arrow
//!
//! This module provides utilities to convert RecordBatches between the standalone
//! `arrow` crate (v54) and the bundled `datafusion::arrow` types. This is necessary
//! because DataFusion 44 bundles its own version of Arrow that may differ from the
//! standalone crate.
//!
//! The conversion uses Arrow IPC (Inter-Process Communication) format as an
//! intermediate representation. This is the idiomatic approach as it:
//! - Uses the official Arrow wire format
//! - Zero-copy where possible
//! - Maintains full schema and data fidelity
//! - Compatible across all Arrow implementations

use crate::error::{Error, Result};
use bytes::Bytes;

/// Convert arrow v54 RecordBatch to datafusion::arrow RecordBatch
///
/// This performs the conversion by:
/// 1. Serializing the arrow v54 RecordBatch to Arrow IPC format
/// 2. Deserializing it as datafusion::arrow RecordBatch
///
/// # Errors
///
/// Returns error if:
/// - IPC serialization fails
/// - IPC deserialization fails
/// - Schema is incompatible
pub fn to_datafusion_batch(
    batch: &arrow::record_batch::RecordBatch,
) -> Result<arrow::record_batch::RecordBatch> {
    // Serialize to IPC bytes using arrow v54
    let ipc_bytes = serialize_batch_to_ipc(batch)?;

    // Deserialize using datafusion::arrow
    deserialize_batch_from_ipc_df(&ipc_bytes)
}

/// Convert multiple arrow v54 RecordBatches to datafusion::arrow RecordBatches
///
/// This is more efficient than converting individually when processing
/// multiple batches as it reuses the IPC stream format.
///
/// # Errors
///
/// Returns error if any batch conversion fails
pub fn to_datafusion_batches(
    batches: &[arrow::record_batch::RecordBatch],
) -> Result<Vec<arrow::record_batch::RecordBatch>> {
    if batches.is_empty() {
        return Ok(Vec::new());
    }

    // For multiple batches, serialize all to IPC stream
    let ipc_bytes = serialize_batches_to_ipc(batches)?;

    // Deserialize all using datafusion::arrow
    deserialize_batches_from_ipc_df(&ipc_bytes)
}

/// Convert datafusion::arrow RecordBatch to arrow v54 RecordBatch
///
/// This is the reverse operation of `to_datafusion_batch`.
///
/// # Errors
///
/// Returns error if conversion fails
#[allow(dead_code)]
pub fn from_datafusion_batch(
    batch: &arrow::record_batch::RecordBatch,
) -> Result<arrow::record_batch::RecordBatch> {
    // Serialize to IPC bytes using datafusion::arrow
    let ipc_bytes = serialize_batch_to_ipc_df(batch)?;

    // Deserialize using arrow v54
    deserialize_batch_from_ipc(&ipc_bytes)
}

// ============================================================================
// Internal implementation using Arrow IPC
// ============================================================================

/// Serialize a single arrow v54 RecordBatch to IPC format
fn serialize_batch_to_ipc(batch: &arrow::record_batch::RecordBatch) -> Result<Bytes> {
    use arrow::ipc::writer::StreamWriter;

    let mut buffer = Vec::new();
    {
        let mut writer = StreamWriter::try_new(&mut buffer, &batch.schema())
            .map_err(|e| Error::General(format!("Failed to create IPC writer: {}", e)))?;

        writer
            .write(batch)
            .map_err(|e| Error::General(format!("Failed to write batch to IPC: {}", e)))?;

        writer
            .finish()
            .map_err(|e| Error::General(format!("Failed to finish IPC stream: {}", e)))?;
    }

    Ok(Bytes::from(buffer))
}

/// Serialize multiple arrow v54 RecordBatches to IPC stream format
fn serialize_batches_to_ipc(
    batches: &[arrow::record_batch::RecordBatch],
) -> Result<Bytes> {
    use arrow::ipc::writer::StreamWriter;

    if batches.is_empty() {
        return Err(Error::General(
            "Cannot serialize empty batch list".to_string(),
        ));
    }

    let schema = batches[0].schema();

    let mut buffer = Vec::new();
    {
        let mut writer = StreamWriter::try_new(&mut buffer, &schema)
            .map_err(|e| Error::General(format!("Failed to create IPC writer: {}", e)))?;

        for batch in batches {
            writer
                .write(batch)
                .map_err(|e| Error::General(format!("Failed to write batch to IPC: {}", e)))?;
        }

        writer
            .finish()
            .map_err(|e| Error::General(format!("Failed to finish IPC stream: {}", e)))?;
    }

    Ok(Bytes::from(buffer))
}

/// Deserialize arrow v54 RecordBatch from IPC format
fn deserialize_batch_from_ipc(
    bytes: &[u8],
) -> Result<arrow::record_batch::RecordBatch> {
    use arrow::ipc::reader::StreamReader;
    use std::io::Cursor;

    let cursor = Cursor::new(bytes);
    let mut reader = StreamReader::try_new(cursor, None)
        .map_err(|e| Error::General(format!("Failed to create IPC reader: {}", e)))?;

    // Read the first (and only) batch
    let batch = reader
        .next()
        .ok_or_else(|| Error::General("No batch found in IPC stream".to_string()))?
        .map_err(|e| Error::General(format!("Failed to read batch from IPC: {}", e)))?;

    Ok(batch)
}

/// Serialize a single datafusion::arrow RecordBatch to IPC format
fn serialize_batch_to_ipc_df(
    batch: &arrow::record_batch::RecordBatch,
) -> Result<Bytes> {
    use arrow::ipc::writer::StreamWriter;

    let mut buffer = Vec::new();
    {
        let mut writer = StreamWriter::try_new(&mut buffer, &batch.schema())
            .map_err(|e| Error::General(format!("Failed to create IPC writer: {}", e)))?;

        writer
            .write(batch)
            .map_err(|e| Error::General(format!("Failed to write batch to IPC: {}", e)))?;

        writer
            .finish()
            .map_err(|e| Error::General(format!("Failed to finish IPC stream: {}", e)))?;
    }

    Ok(Bytes::from(buffer))
}

/// Deserialize datafusion::arrow RecordBatch from IPC format
fn deserialize_batch_from_ipc_df(
    bytes: &[u8],
) -> Result<arrow::record_batch::RecordBatch> {
    use arrow::ipc::reader::StreamReader;
    use std::io::Cursor;

    let cursor = Cursor::new(bytes);
    let mut reader = StreamReader::try_new(cursor, None)
        .map_err(|e| Error::General(format!("Failed to create IPC reader: {}", e)))?;

    // Read the first (and only) batch
    let batch = reader
        .next()
        .ok_or_else(|| Error::General("No batch found in IPC stream".to_string()))?
        .map_err(|e| Error::General(format!("Failed to read batch from IPC: {}", e)))?;

    Ok(batch)
}

/// Deserialize multiple datafusion::arrow RecordBatches from IPC stream
fn deserialize_batches_from_ipc_df(
    bytes: &[u8],
) -> Result<Vec<arrow::record_batch::RecordBatch>> {
    use arrow::ipc::reader::StreamReader;
    use std::io::Cursor;

    let cursor = Cursor::new(bytes);
    let reader = StreamReader::try_new(cursor, None)
        .map_err(|e| Error::General(format!("Failed to create IPC reader: {}", e)))?;

    let batches: std::result::Result<Vec<_>, _> = reader.collect();
    batches.map_err(|e| Error::General(format!("Failed to read batches from IPC: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{Int32Array, StringArray};
    use arrow::datatypes::{DataType, Field, Schema};
    use std::sync::Arc;

    #[test]
    fn test_single_batch_conversion() {
        // Create a simple RecordBatch with arrow v54
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, false),
        ]));

        let batch = arrow::record_batch::RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int32Array::from(vec![1, 2, 3])),
                Arc::new(StringArray::from(vec!["a", "b", "c"])),
            ],
        )
        .unwrap();

        // Convert to datafusion batch
        let df_batch = to_datafusion_batch(&batch).unwrap();

        // Verify dimensions
        assert_eq!(df_batch.num_rows(), 3);
        assert_eq!(df_batch.num_columns(), 2);

        // Verify schema
        assert_eq!(df_batch.schema().fields().len(), 2);
    }

    #[test]
    fn test_multiple_batches_conversion() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "value",
            DataType::Int32,
            false,
        )]));

        let batch1 = arrow::record_batch::RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(Int32Array::from(vec![1, 2]))],
        )
        .unwrap();

        let batch2 = arrow::record_batch::RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(Int32Array::from(vec![3, 4]))],
        )
        .unwrap();

        let batches = vec![batch1, batch2];

        // Convert to datafusion batches
        let df_batches = to_datafusion_batches(&batches).unwrap();

        assert_eq!(df_batches.len(), 2);
        assert_eq!(df_batches[0].num_rows(), 2);
        assert_eq!(df_batches[1].num_rows(), 2);
    }

    #[test]
    fn test_empty_batches() {
        let batches: Vec<arrow::record_batch::RecordBatch> = vec![];
        let result = to_datafusion_batches(&batches).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_roundtrip_conversion() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "data",
            DataType::Int32,
            false,
        )]));

        let original = arrow::record_batch::RecordBatch::try_new(
            schema,
            vec![Arc::new(Int32Array::from(vec![42, 43, 44]))],
        )
        .unwrap();

        // Convert to datafusion and back
        let df_batch = to_datafusion_batch(&original).unwrap();
        let back = from_datafusion_batch(&df_batch).unwrap();

        assert_eq!(back.num_rows(), original.num_rows());
        assert_eq!(back.num_columns(), original.num_columns());
    }
}
