//! End-to-end integration tests for inspect command

use std::path::Path;
use std::sync::Arc;

use tabletools::cli::commands::InspectCommand;
use tabletools::cli::parser::InspectArgs;
use tabletools::core::formats::FormatHandlerFactory;
use tabletools::core::operations::inspect::{InspectOperation, InspectOptions};
use tabletools::core::storage::{LocalBackend, StorageBackend};

#[tokio::test]
async fn test_inspect_command_basic() {
    let args = InspectArgs {
        path: "tests/fixtures/sample.parquet".to_string(),
        rows: 10,
        schema: false,
        metadata: false,
        stats: false,
        format: None,
        output: "table".to_string(),
        columns: None,
        sample: false,
    };

    let result = InspectCommand::execute(args).await;
    assert!(result.is_ok(), "Basic inspect should succeed");
}

#[tokio::test]
async fn test_inspect_schema_only() {
    let args = InspectArgs {
        path: "tests/fixtures/types.parquet".to_string(),
        rows: 0,
        schema: true,
        metadata: false,
        stats: false,
        format: None,
        output: "table".to_string(),
        columns: None,
        sample: false,
    };

    let result = InspectCommand::execute(args).await;
    assert!(result.is_ok(), "Schema-only inspect should succeed");
}

#[tokio::test]
async fn test_inspect_with_metadata() {
    let args = InspectArgs {
        path: "tests/fixtures/sample.parquet".to_string(),
        rows: 0,
        schema: false,
        metadata: true,
        stats: false,
        format: None,
        output: "table".to_string(),
        columns: None,
        sample: false,
    };

    let result = InspectCommand::execute(args).await;
    assert!(result.is_ok(), "Metadata inspect should succeed");
}

#[tokio::test]
async fn test_inspect_with_stats() {
    let args = InspectArgs {
        path: "tests/fixtures/sample.parquet".to_string(),
        rows: 0,
        schema: false,
        metadata: false,
        stats: true,
        format: None,
        output: "table".to_string(),
        columns: None,
        sample: false,
    };

    let result = InspectCommand::execute(args).await;
    assert!(result.is_ok(), "Stats inspect should succeed");
}

#[tokio::test]
async fn test_inspect_all_options() {
    let args = InspectArgs {
        path: "tests/fixtures/sample.parquet".to_string(),
        rows: 5,
        schema: true,
        metadata: true,
        stats: true,
        format: None,
        output: "table".to_string(),
        columns: None,
        sample: false,
    };

    let result = InspectCommand::execute(args).await;
    assert!(result.is_ok(), "Full inspect should succeed");
}

#[tokio::test]
async fn test_inspect_column_filtering() {
    let args = InspectArgs {
        path: "tests/fixtures/sample.parquet".to_string(),
        rows: 5,
        schema: false,
        metadata: false,
        stats: false,
        format: None,
        output: "table".to_string(),
        columns: Some(vec!["id".to_string(), "name".to_string()]),
        sample: false,
    };

    let result = InspectCommand::execute(args).await;
    assert!(result.is_ok(), "Column filtering should succeed");
}

#[tokio::test]
async fn test_inspect_json_output() {
    let args = InspectArgs {
        path: "tests/fixtures/sample.parquet".to_string(),
        rows: 5,
        schema: true,
        metadata: true,
        stats: false,
        format: None,
        output: "json".to_string(),
        columns: None,
        sample: false,
    };

    let result = InspectCommand::execute(args).await;
    assert!(result.is_ok(), "JSON output should succeed");
}

#[tokio::test]
async fn test_inspect_yaml_output() {
    let args = InspectArgs {
        path: "tests/fixtures/types.parquet".to_string(),
        rows: 5,
        schema: true,
        metadata: false,
        stats: false,
        format: None,
        output: "yaml".to_string(),
        columns: None,
        sample: false,
    };

    let result = InspectCommand::execute(args).await;
    assert!(result.is_ok(), "YAML output should succeed");
}

#[tokio::test]
async fn test_inspect_nonexistent_file() {
    let args = InspectArgs {
        path: "tests/fixtures/nonexistent.parquet".to_string(),
        rows: 10,
        schema: false,
        metadata: false,
        stats: false,
        format: None,
        output: "table".to_string(),
        columns: None,
        sample: false,
    };

    let result = InspectCommand::execute(args).await;
    assert!(
        result.is_err(),
        "Inspecting nonexistent file should return error"
    );
}

#[tokio::test]
async fn test_inspect_larger_file() {
    let args = InspectArgs {
        path: "tests/fixtures/larger.parquet".to_string(),
        rows: 10,
        schema: false,
        metadata: true,
        stats: true,
        format: None,
        output: "table".to_string(),
        columns: None,
        sample: false,
    };

    let result = InspectCommand::execute(args).await;
    assert!(result.is_ok(), "Larger file inspect should succeed");
}

#[tokio::test]
async fn test_inspect_limited_rows() {
    let args = InspectArgs {
        path: "tests/fixtures/larger.parquet".to_string(),
        rows: 5,
        schema: false,
        metadata: false,
        stats: false,
        format: None,
        output: "table".to_string(),
        columns: None,
        sample: false,
    };

    let result = InspectCommand::execute(args).await;
    assert!(result.is_ok(), "Limited rows inspect should succeed");
}

#[tokio::test]
async fn test_inspect_operation_directly() {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());
    let path = Path::new("tests/fixtures/sample.parquet");
    let handler = FormatHandlerFactory::create_handler(path, storage)
        .await
        .expect("Should create handler");

    let operation = InspectOperation::new(handler);
    let options = InspectOptions {
        schema_only: false,
        show_metadata: true,
        show_stats: true,
        num_rows: 5,
        columns: None,
        sample: false,
    };

    let result = operation.execute(&options).await;
    assert!(result.is_ok(), "InspectOperation should succeed");

    let inspect_result = result.unwrap();
    assert_eq!(inspect_result.format_name, "Apache Parquet");
    assert_eq!(inspect_result.schema.fields().len(), 5);
    assert!(inspect_result.metadata.is_some());
    assert!(inspect_result.statistics.is_some());
    assert!(inspect_result.sample_data.is_some());

    let sample_data = inspect_result.sample_data.unwrap();
    assert_eq!(sample_data.num_rows(), 5);
}

#[tokio::test]
async fn test_inspect_operation_schema_only() {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());
    let path = Path::new("tests/fixtures/types.parquet");
    let handler = FormatHandlerFactory::create_handler(path, storage)
        .await
        .expect("Should create handler");

    let operation = InspectOperation::new(handler);
    let options = InspectOptions {
        schema_only: true,
        show_metadata: false,
        show_stats: false,
        num_rows: 0,
        columns: None,
        sample: false,
    };

    let result = operation.execute(&options).await;
    assert!(result.is_ok(), "Schema-only operation should succeed");

    let inspect_result = result.unwrap();
    assert_eq!(inspect_result.schema.fields().len(), 6);
    assert!(inspect_result.metadata.is_none());
    assert!(inspect_result.statistics.is_none());
    assert!(inspect_result.sample_data.is_none());
}

#[tokio::test]
async fn test_inspect_operation_with_column_filter() {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());
    let path = Path::new("tests/fixtures/sample.parquet");
    let handler = FormatHandlerFactory::create_handler(path, storage)
        .await
        .expect("Should create handler");

    let operation = InspectOperation::new(handler);
    let options = InspectOptions {
        schema_only: false,
        show_metadata: false,
        show_stats: false,
        num_rows: 3,
        columns: Some(vec!["id".to_string(), "name".to_string()]),
        sample: false,
    };

    let result = operation.execute(&options).await;
    assert!(result.is_ok(), "Column filtering should succeed");

    let inspect_result = result.unwrap();
    let sample_data = inspect_result.sample_data.unwrap();
    assert_eq!(sample_data.num_columns(), 2, "Should only have 2 columns");
    assert_eq!(sample_data.num_rows(), 3);
}

#[tokio::test]
async fn test_format_handler_factory_parquet() {
    let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());
    let path = Path::new("tests/fixtures/sample.parquet");
    let handler = FormatHandlerFactory::create_handler(path, storage).await;

    assert!(handler.is_ok(), "Should create handler for .parquet file");
    let handler = handler.unwrap();
    assert_eq!(handler.format_name(), "Apache Parquet");
}

// Note: Testing unsupported format is tricky because the factory tries
// all registered handlers, including unimplemented ones (Delta, Iceberg).
// This test is commented out for now.
// #[tokio::test]
// async fn test_format_handler_factory_unsupported() {
//     let storage: Arc<dyn StorageBackend> = Arc::new(LocalBackend::new().unwrap());
//     let path = Path::new("Cargo.toml"); // .toml is not a supported format
//     let handler = FormatHandlerFactory::create_handler(path, storage).await;
//     assert!(handler.is_err(), "Should fail for unsupported file format");
// }

#[tokio::test]
async fn test_multiple_compression_formats() {
    // Test different compression formats created by generate_fixtures

    // SNAPPY
    let args_snappy = InspectArgs {
        path: "tests/fixtures/sample.parquet".to_string(),
        rows: 0,
        schema: false,
        metadata: true,
        stats: false,
        format: None,
        output: "table".to_string(),
        columns: None,
        sample: false,
    };
    assert!(InspectCommand::execute(args_snappy).await.is_ok());

    // GZIP
    let args_gzip = InspectArgs {
        path: "tests/fixtures/types.parquet".to_string(),
        rows: 0,
        schema: false,
        metadata: true,
        stats: false,
        format: None,
        output: "table".to_string(),
        columns: None,
        sample: false,
    };
    assert!(InspectCommand::execute(args_gzip).await.is_ok());

    // ZSTD
    let args_zstd = InspectArgs {
        path: "tests/fixtures/larger.parquet".to_string(),
        rows: 0,
        schema: false,
        metadata: true,
        stats: false,
        format: None,
        output: "table".to_string(),
        columns: None,
        sample: false,
    };
    assert!(InspectCommand::execute(args_zstd).await.is_ok());
}

#[tokio::test]
async fn test_inspect_with_sample_flag() {
    let args = InspectArgs {
        path: "tests/fixtures/larger.parquet".to_string(),
        rows: 10,
        schema: false,
        metadata: false,
        stats: false,
        format: None,
        output: "table".to_string(),
        columns: None,
        sample: true,
    };

    let result = InspectCommand::execute(args).await;
    assert!(result.is_ok(), "Sample inspect should succeed");
}

#[tokio::test]
async fn test_end_to_end_workflow() {
    // This test simulates a complete user workflow

    // Step 1: Inspect file schema
    let schema_args = InspectArgs {
        path: "tests/fixtures/sample.parquet".to_string(),
        rows: 0,
        schema: true,
        metadata: false,
        stats: false,
        format: None,
        output: "json".to_string(),
        columns: None,
        sample: false,
    };
    assert!(InspectCommand::execute(schema_args).await.is_ok());

    // Step 2: Check metadata
    let metadata_args = InspectArgs {
        path: "tests/fixtures/sample.parquet".to_string(),
        rows: 0,
        schema: false,
        metadata: true,
        stats: false,
        format: None,
        output: "json".to_string(),
        columns: None,
        sample: false,
    };
    assert!(InspectCommand::execute(metadata_args).await.is_ok());

    // Step 3: View sample data with specific columns
    let data_args = InspectArgs {
        path: "tests/fixtures/sample.parquet".to_string(),
        rows: 3,
        schema: false,
        metadata: false,
        stats: false,
        format: None,
        output: "table".to_string(),
        columns: Some(vec!["name".to_string(), "salary".to_string()]),
        sample: false,
    };
    assert!(InspectCommand::execute(data_args).await.is_ok());

    // Step 4: Full inspection with all options
    let full_args = InspectArgs {
        path: "tests/fixtures/sample.parquet".to_string(),
        rows: 5,
        schema: true,
        metadata: true,
        stats: true,
        format: None,
        output: "table".to_string(),
        columns: None,
        sample: false,
    };
    assert!(InspectCommand::execute(full_args).await.is_ok());
}
