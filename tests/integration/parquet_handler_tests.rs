//! Unit tests for ParquetHandler

use std::path::Path;
use std::sync::Arc;

use arrow::datatypes::DataType;
use tablectl::core::formats::{FormatHandler, ParquetHandler, ReadOptions};
use tablectl::core::storage::{LocalBackend, StorageBackend};

async fn create_parquet_handler(path: &str) -> ParquetHandler {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());
    ParquetHandler::new(Path::new(path), storage).expect("Failed to create ParquetHandler")
}

#[tokio::test]
async fn test_can_handle_parquet_extension() {
    let handler = create_parquet_handler("tests/fixtures/sample.parquet").await;
    let result = handler
        .can_handle(Path::new("tests/fixtures/sample.parquet"))
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap(), "Should handle .parquet files");
}

#[tokio::test]
async fn test_format_name() {
    let handler = create_parquet_handler("tests/fixtures/sample.parquet").await;
    assert_eq!(handler.format_name(), "Apache Parquet");
}

#[tokio::test]
async fn test_read_schema_sample() {
    let handler = create_parquet_handler("tests/fixtures/sample.parquet").await;
    let schema = handler.read_schema().await.expect("Failed to read schema");

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
    let handler = create_parquet_handler("tests/fixtures/types.parquet").await;
    let schema = handler.read_schema().await.expect("Failed to read schema");

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
    let handler = create_parquet_handler("tests/fixtures/sample.parquet").await;
    let metadata = handler
        .read_metadata()
        .await
        .expect("Failed to read metadata");

    // Verify metadata fields
    assert_eq!(metadata.num_rows, Some(5));
    assert!(metadata.compressed_size.is_some());
    assert!(metadata.uncompressed_size.is_some());
    assert!(metadata.compression.is_some());
    assert!(metadata.format_version.is_some());

    // Compression should be SNAPPY (as set in generate_fixtures)
    assert!(
        metadata
            .compression
            .unwrap()
            .to_uppercase()
            .contains("SNAPPY")
    );
}

#[tokio::test]
async fn test_read_metadata_larger_file() {
    let handler = create_parquet_handler("tests/fixtures/larger.parquet").await;
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
    let handler = create_parquet_handler("tests/fixtures/sample.parquet").await;
    let batch = handler
        .read_batch(&ReadOptions::default())
        .await
        .expect("Failed to read batch");

    assert_eq!(batch.num_rows(), 5);
    assert_eq!(batch.num_columns(), 5);
}

#[tokio::test]
async fn test_read_batch_with_limit() {
    let handler = create_parquet_handler("tests/fixtures/larger.parquet").await;
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
    let handler = create_parquet_handler("tests/fixtures/larger.parquet").await;
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
    let handler = create_parquet_handler("tests/fixtures/sample.parquet").await;
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
    let handler = create_parquet_handler("tests/fixtures/sample.parquet").await;
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
    let handler = create_parquet_handler("tests/fixtures/sample.parquet").await;
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
    let handler = create_parquet_handler("tests/fixtures/sample.parquet").await;
    let report = handler.validate(true).await.expect("Failed to validate");

    assert!(report.is_valid, "Valid file should pass validation");
    assert!(report.errors.is_empty());
}

#[tokio::test]
async fn test_validate_full() {
    let handler = create_parquet_handler("tests/fixtures/sample.parquet").await;
    let report = handler.validate(false).await.expect("Failed to validate");

    assert!(report.is_valid, "Valid file should pass full validation");
    assert!(report.errors.is_empty());
}

#[tokio::test]
async fn test_has_native_statistics() {
    let handler = create_parquet_handler("tests/fixtures/sample.parquet").await;
    assert!(
        handler.has_native_statistics(),
        "Parquet should have native statistics"
    );
}

#[tokio::test]
async fn test_read_nonexistent_file() {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());
    let handler = ParquetHandler::new(Path::new("tests/fixtures/nonexistent.parquet"), storage)
        .expect("Handler creation should succeed");

    let result = handler.read_schema().await;
    assert!(result.is_err(), "Reading nonexistent file should fail");
}

#[tokio::test]
async fn test_different_compression_types() {
    // Test SNAPPY compression
    let handler_snappy = create_parquet_handler("tests/fixtures/sample.parquet").await;
    let metadata_snappy = handler_snappy.read_metadata().await.unwrap();
    assert!(metadata_snappy.compression.unwrap().contains("SNAPPY"));

    // Test GZIP compression
    let handler_gzip = create_parquet_handler("tests/fixtures/types.parquet").await;
    let metadata_gzip = handler_gzip.read_metadata().await.unwrap();
    assert!(metadata_gzip.compression.unwrap().contains("GZIP"));

    // Test ZSTD compression
    let handler_zstd = create_parquet_handler("tests/fixtures/larger.parquet").await;
    let metadata_zstd = handler_zstd.read_metadata().await.unwrap();
    assert!(metadata_zstd.compression.unwrap().contains("ZSTD"));
}
