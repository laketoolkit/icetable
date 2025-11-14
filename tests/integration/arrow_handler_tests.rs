//! Unit tests for ArrowHandler

use std::path::Path;
use std::sync::Arc;

use arrow::datatypes::DataType;
use tabletools::core::formats::{ArrowHandler, FormatHandler, ReadOptions};
use tabletools::core::storage::{LocalBackend, StorageBackend};

async fn create_arrow_handler(path: &str) -> ArrowHandler {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());
    ArrowHandler::new(Path::new(path), storage).expect("Failed to create ArrowHandler")
}

#[tokio::test]
async fn test_can_handle_arrow_extension() {
    let handler = create_arrow_handler("tests/fixtures/sample.arrow").await;
    let result = handler
        .can_handle(Path::new("tests/fixtures/sample.arrow"))
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap(), "Should handle .arrow files");
}

#[tokio::test]
async fn test_format_name() {
    let handler = create_arrow_handler("tests/fixtures/sample.arrow").await;
    assert_eq!(handler.format_name(), "Apache Arrow IPC");
}

#[tokio::test]
async fn test_read_schema_sample() {
    let handler = create_arrow_handler("tests/fixtures/sample.arrow").await;
    let schema = handler
        .read_schema()
        .await
        .expect("Failed to read schema");

    // Verify schema has expected fields
    assert_eq!(schema.fields().len(), 5);

    // Check field names and types
    let field_names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
    assert!(field_names.contains(&"id"));
    assert!(field_names.contains(&"name"));
    assert!(field_names.contains(&"age"));
    assert!(field_names.contains(&"salary"));
    assert!(field_names.contains(&"active"));

    // Check specific field types
    let id_field = schema.field_with_name("id").unwrap();
    assert_eq!(id_field.data_type(), &DataType::Int32);

    let name_field = schema.field_with_name("name").unwrap();
    assert_eq!(name_field.data_type(), &DataType::Utf8);
    assert!(name_field.is_nullable());

    let salary_field = schema.field_with_name("salary").unwrap();
    assert_eq!(salary_field.data_type(), &DataType::Float64);
}

#[tokio::test]
async fn test_read_schema_types() {
    let handler = create_arrow_handler("tests/fixtures/types.arrow").await;
    let schema = handler
        .read_schema()
        .await
        .expect("Failed to read schema");

    assert_eq!(schema.fields().len(), 6);

    // Verify different data types
    assert_eq!(
        schema.field_with_name("int32_col").unwrap().data_type(),
        &DataType::Int32
    );
    assert_eq!(
        schema.field_with_name("int64_col").unwrap().data_type(),
        &DataType::Int64
    );
    assert_eq!(
        schema.field_with_name("float_col").unwrap().data_type(),
        &DataType::Float64
    );
    assert_eq!(
        schema.field_with_name("string_col").unwrap().data_type(),
        &DataType::Utf8
    );
    assert_eq!(
        schema.field_with_name("bool_col").unwrap().data_type(),
        &DataType::Boolean
    );
}

#[tokio::test]
async fn test_read_metadata() {
    let handler = create_arrow_handler("tests/fixtures/sample.arrow").await;
    let metadata = handler
        .read_metadata()
        .await
        .expect("Failed to read metadata");

    // Verify metadata fields
    assert_eq!(metadata.num_rows, Some(5));
    assert!(metadata.compressed_size.is_some());
    assert!(metadata.uncompressed_size.is_some());
    assert_eq!(metadata.compression, None); // Arrow IPC not compressed
    assert!(metadata.format_version.is_some());
}

#[tokio::test]
async fn test_read_metadata_larger_file() {
    let handler = create_arrow_handler("tests/fixtures/larger.arrow").await;
    let metadata = handler
        .read_metadata()
        .await
        .expect("Failed to read metadata");

    assert_eq!(metadata.num_rows, Some(100));
    assert!(metadata.compressed_size.unwrap() > 0);
    assert!(metadata.uncompressed_size.unwrap() > 0);
}

#[tokio::test]
async fn test_read_batch_default_options() {
    let handler = create_arrow_handler("tests/fixtures/sample.arrow").await;
    let batch = handler
        .read_batch(&ReadOptions::default())
        .await
        .expect("Failed to read batch");

    assert_eq!(batch.num_rows(), 5);
    assert_eq!(batch.num_columns(), 5);
}

#[tokio::test]
async fn test_read_batch_with_limit() {
    let handler = create_arrow_handler("tests/fixtures/larger.arrow").await;
    let options = ReadOptions {
        limit: Some(10),
        ..Default::default()
    };

    let batch = handler
        .read_batch(&options)
        .await
        .expect("Failed to read batch");

    assert_eq!(batch.num_rows(), 10, "Should limit rows to 10");
}

#[tokio::test]
async fn test_read_batch_with_offset() {
    let handler = create_arrow_handler("tests/fixtures/larger.arrow").await;
    let options = ReadOptions {
        offset: Some(50),
        limit: Some(10),
        ..Default::default()
    };

    let batch = handler
        .read_batch(&options)
        .await
        .expect("Failed to read batch");

    assert_eq!(batch.num_rows(), 10, "Should read 10 rows after offset");
}

#[tokio::test]
async fn test_read_batch_with_columns() {
    let handler = create_arrow_handler("tests/fixtures/sample.arrow").await;
    let options = ReadOptions {
        columns: Some(vec!["id".to_string(), "name".to_string()]),
        ..Default::default()
    };

    let batch = handler
        .read_batch(&options)
        .await
        .expect("Failed to read batch");

    assert_eq!(batch.num_columns(), 2, "Should only have 2 columns");
    assert_eq!(batch.schema().field(0).name(), "id");
    assert_eq!(batch.schema().field(1).name(), "name");
}

#[tokio::test]
async fn test_read_batches() {
    let handler = create_arrow_handler("tests/fixtures/sample.arrow").await;
    let batches = handler
        .read_batches(&ReadOptions::default())
        .await
        .expect("Failed to read batches");

    assert!(!batches.is_empty());
    let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();
    assert_eq!(total_rows, 5);
}

#[tokio::test]
async fn test_read_statistics() {
    let handler = create_arrow_handler("tests/fixtures/sample.arrow").await;
    let stats = handler
        .read_statistics()
        .await
        .expect("Failed to read statistics");

    assert_eq!(stats.len(), 5, "Should have stats for all 5 columns");

    // Find stats for specific columns
    let id_stats = stats.iter().find(|s| s.name == "id");
    assert!(id_stats.is_some());

    let name_stats = stats.iter().find(|s| s.name == "name");
    assert!(name_stats.is_some());
    // Name column has 1 null value
    assert!(name_stats.unwrap().null_count.unwrap() >= 1);
}

#[tokio::test]
async fn test_validate_quick() {
    let handler = create_arrow_handler("tests/fixtures/sample.arrow").await;
    let report = handler
        .validate(true)
        .await
        .expect("Failed to validate");

    assert!(report.is_valid, "Valid file should pass validation");
    assert!(report.errors.is_empty());
}

#[tokio::test]
async fn test_validate_full() {
    let handler = create_arrow_handler("tests/fixtures/sample.arrow").await;
    let report = handler
        .validate(false)
        .await
        .expect("Failed to validate");

    assert!(report.is_valid, "Valid file should pass full validation");
    assert!(report.errors.is_empty());
}

#[tokio::test]
async fn test_has_native_statistics() {
    let handler = create_arrow_handler("tests/fixtures/sample.arrow").await;
    assert!(
        !handler.has_native_statistics(),
        "Arrow IPC should not have native statistics"
    );
}

#[tokio::test]
async fn test_read_nonexistent_file() {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());
    let handler = ArrowHandler::new(Path::new("tests/fixtures/nonexistent.arrow"), storage)
        .expect("Handler creation should succeed");

    let result = handler.read_schema().await;
    assert!(result.is_err(), "Reading nonexistent file should fail");
}

#[tokio::test]
async fn test_write_and_read_roundtrip() {
    use arrow::array::{ArrayRef, Int32Array, StringArray};
    use arrow::datatypes::{DataType, Field, Schema};
    use arrow::record_batch::RecordBatch;

    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());
    let test_path = "tests/fixtures/test_write.arrow";
    let handler = ArrowHandler::new(Path::new(test_path), storage.clone()).unwrap();

    // Create test data
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int32, false),
        Field::new("name", DataType::Utf8, true),
    ]));

    let id_array = Arc::new(Int32Array::from(vec![1, 2, 3])) as ArrayRef;
    let name_array = Arc::new(StringArray::from(vec![
        Some("Alice"),
        Some("Bob"),
        None,
    ])) as ArrayRef;

    let batch = RecordBatch::try_new(schema.clone(), vec![id_array, name_array]).unwrap();

    // Write
    let write_result = handler
        .write(vec![batch.clone()], &Default::default())
        .await;
    assert!(write_result.is_ok(), "Should write file");

    // Read it back
    let read_handler = ArrowHandler::new(Path::new(test_path), storage.clone()).unwrap();
    let read_batch = read_handler.read_batch(&ReadOptions::default()).await;
    assert!(read_batch.is_ok(), "Should read written file");

    let read_batch = read_batch.unwrap();
    assert_eq!(read_batch.num_rows(), 3);
    assert_eq!(read_batch.num_columns(), 2);

    // Clean up
    use tabletools::core::storage::StorageBackend;
    storage.delete(test_path).await.ok();
}
