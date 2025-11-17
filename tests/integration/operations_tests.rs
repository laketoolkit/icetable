//! Tests for ValidateOperation and ConvertOperation

use std::path::Path;
use std::sync::Arc;

use tablectl::core::formats::{ArrowHandler, FormatHandler, ParquetHandler, WriteOptions};
use tablectl::core::operations::convert::ConvertOperation;
use tablectl::core::operations::validate::ValidateOperation;
use tablectl::core::storage::{LocalBackend, StorageBackend};

// ========== ValidateOperation Tests ==========

#[tokio::test]
async fn test_validate_valid_parquet() {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());
    let handler = ParquetHandler::new(Path::new("tests/fixtures/sample.parquet"), storage)
        .expect("Failed to create handler");

    let operation = ValidateOperation::new(Arc::new(handler));
    let result = operation.execute(false).await;

    assert!(result.is_ok(), "Validation should succeed");
    let result = result.unwrap();

    assert!(result.is_valid, "File should be valid");
    assert!(result.errors.is_empty(), "Should have no errors");
    assert_eq!(result.format_name, "Apache Parquet");
    assert_eq!(result.num_rows, Some(5));
}

#[tokio::test]
async fn test_validate_valid_arrow() {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());
    let handler = ArrowHandler::new(Path::new("tests/fixtures/sample.arrow"), storage)
        .expect("Failed to create handler");

    let operation = ValidateOperation::new(Arc::new(handler));
    let result = operation.execute(false).await;

    assert!(result.is_ok(), "Validation should succeed");
    let result = result.unwrap();

    assert!(result.is_valid, "File should be valid");
    assert!(result.errors.is_empty(), "Should have no errors");
    assert_eq!(result.format_name, "Apache Arrow IPC");
    assert_eq!(result.num_rows, Some(5));
}

#[tokio::test]
async fn test_validate_quick_mode() {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());
    let handler = ParquetHandler::new(Path::new("tests/fixtures/larger.parquet"), storage)
        .expect("Failed to create handler");

    let operation = ValidateOperation::new(Arc::new(handler));
    let result = operation.execute(true).await;

    assert!(result.is_ok(), "Validation should succeed");
    let result = result.unwrap();

    assert!(result.is_valid, "File should be valid");
    assert!(result.quick_mode, "Should be in quick mode");
}

#[tokio::test]
async fn test_validate_nonexistent_file() {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());
    let handler =
        ParquetHandler::new(Path::new("tests/fixtures/nonexistent.parquet"), storage).unwrap();

    let operation = ValidateOperation::new(Arc::new(handler));
    let result = operation.execute(false).await;

    // Validation should succeed but mark file as invalid
    assert!(result.is_ok(), "Validation operation should complete");
    let result = result.unwrap();

    assert!(!result.is_valid, "File should be marked as invalid");
    assert!(!result.errors.is_empty(), "Should have error messages");
    assert!(
        result.errors[0].contains("File not found"),
        "Should report file not found"
    );
}

// ========== ConvertOperation Tests ==========

#[tokio::test]
async fn test_convert_parquet_to_arrow() {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());

    // Source: Parquet
    let source_handler =
        ParquetHandler::new(Path::new("tests/fixtures/sample.parquet"), storage.clone())
            .expect("Failed to create source handler");

    // Target: Arrow
    let target_path = "tests/fixtures/test_convert_p2a.arrow";
    let target_handler = ArrowHandler::new(Path::new(target_path), storage.clone())
        .expect("Failed to create target handler");

    // Convert
    let operation = ConvertOperation::new(Arc::new(source_handler), Arc::new(target_handler));
    let result = operation.execute(&WriteOptions::default()).await;

    assert!(result.is_ok(), "Conversion should succeed");
    let result = result.unwrap();

    assert_eq!(result.source_format, "Apache Parquet");
    assert_eq!(result.target_format, "Apache Arrow IPC");
    assert_eq!(result.rows_converted, 5);
    assert!(result.source_size > 0);
    assert!(result.target_size > 0);

    // Verify the converted file can be read
    let verify_handler = ArrowHandler::new(Path::new(target_path), storage.clone()).unwrap();
    let schema = verify_handler.read_schema().await;
    assert!(schema.is_ok(), "Should be able to read converted file");

    // Clean up
    storage.delete(target_path).await.ok();
}

#[tokio::test]
async fn test_convert_arrow_to_parquet() {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());

    // Source: Arrow
    let source_handler =
        ArrowHandler::new(Path::new("tests/fixtures/sample.arrow"), storage.clone())
            .expect("Failed to create source handler");

    // Target: Parquet
    let target_path = "tests/fixtures/test_convert_a2p.parquet";
    let target_handler = ParquetHandler::new(Path::new(target_path), storage.clone())
        .expect("Failed to create target handler");

    // Convert
    let operation = ConvertOperation::new(Arc::new(source_handler), Arc::new(target_handler));
    let result = operation.execute(&WriteOptions::default()).await;

    assert!(result.is_ok(), "Conversion should succeed");
    let result = result.unwrap();

    assert_eq!(result.source_format, "Apache Arrow IPC");
    assert_eq!(result.target_format, "Apache Parquet");
    assert_eq!(result.rows_converted, 5);
    assert!(result.source_size > 0);
    assert!(result.target_size > 0);
    assert!(result.compression_ratio.is_some());

    // Verify the converted file can be read
    let verify_handler = ParquetHandler::new(Path::new(target_path), storage.clone()).unwrap();
    let schema = verify_handler.read_schema().await;
    assert!(schema.is_ok(), "Should be able to read converted file");

    // Clean up
    storage.delete(target_path).await.ok();
}

#[tokio::test]
async fn test_convert_preserves_data() {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());

    // Convert from Parquet to Arrow
    let source =
        ParquetHandler::new(Path::new("tests/fixtures/types.parquet"), storage.clone()).unwrap();
    let target_path = "tests/fixtures/test_convert_types.arrow";
    let target = ArrowHandler::new(Path::new(target_path), storage.clone()).unwrap();

    let operation = ConvertOperation::new(Arc::new(source), Arc::new(target));
    let result = operation.execute(&WriteOptions::default()).await;
    assert!(result.is_ok());

    // Read both files and compare
    let original =
        ParquetHandler::new(Path::new("tests/fixtures/types.parquet"), storage.clone()).unwrap();
    let converted = ArrowHandler::new(Path::new(target_path), storage.clone()).unwrap();

    let original_schema = original.read_schema().await.unwrap();
    let converted_schema = converted.read_schema().await.unwrap();

    // Schemas should match
    assert_eq!(
        original_schema.fields().len(),
        converted_schema.fields().len()
    );
    for (orig_field, conv_field) in original_schema
        .fields()
        .iter()
        .zip(converted_schema.fields().iter())
    {
        assert_eq!(orig_field.name(), conv_field.name());
        assert_eq!(orig_field.data_type(), conv_field.data_type());
    }

    // Clean up
    storage.delete(target_path).await.ok();
}

#[tokio::test]
async fn test_convert_larger_file() {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());

    let source =
        ParquetHandler::new(Path::new("tests/fixtures/larger.parquet"), storage.clone()).unwrap();
    let target_path = "tests/fixtures/test_convert_larger.arrow";
    let target = ArrowHandler::new(Path::new(target_path), storage.clone()).unwrap();

    let operation = ConvertOperation::new(Arc::new(source), Arc::new(target));
    let result = operation.execute(&WriteOptions::default()).await;

    assert!(result.is_ok());
    let result = result.unwrap();

    assert_eq!(result.rows_converted, 100);
    assert!(result.source_size > 0);
    assert!(result.target_size > 0);

    // Clean up
    storage.delete(target_path).await.ok();
}

#[tokio::test]
async fn test_convert_roundtrip() {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());

    // Parquet -> Arrow -> Parquet
    let step1_source =
        ParquetHandler::new(Path::new("tests/fixtures/sample.parquet"), storage.clone()).unwrap();
    let step1_target_path = "tests/fixtures/test_roundtrip_step1.arrow";
    let step1_target = ArrowHandler::new(Path::new(step1_target_path), storage.clone()).unwrap();

    let op1 = ConvertOperation::new(Arc::new(step1_source), Arc::new(step1_target));
    let result1 = op1.execute(&WriteOptions::default()).await;
    assert!(result1.is_ok());

    let step2_source = ArrowHandler::new(Path::new(step1_target_path), storage.clone()).unwrap();
    let step2_target_path = "tests/fixtures/test_roundtrip_step2.parquet";
    let step2_target = ParquetHandler::new(Path::new(step2_target_path), storage.clone()).unwrap();

    let op2 = ConvertOperation::new(Arc::new(step2_source), Arc::new(step2_target));
    let result2 = op2.execute(&WriteOptions::default()).await;
    assert!(result2.is_ok());

    // Verify final file
    let final_handler = ParquetHandler::new(Path::new(step2_target_path), storage.clone()).unwrap();
    let metadata = final_handler.read_metadata().await;
    assert!(metadata.is_ok());
    assert_eq!(metadata.unwrap().num_rows, Some(5));

    // Clean up
    storage.delete(step1_target_path).await.ok();
    storage.delete(step2_target_path).await.ok();
}
