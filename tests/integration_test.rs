//! Integration tests for TableTools

use tabletools::cli::commands::InspectCommand;
use tabletools::cli::parser::InspectArgs;

#[tokio::test]
async fn test_inspect_sample_parquet() {
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
    assert!(result.is_ok(), "Inspect command should succeed");
}

#[tokio::test]
async fn test_inspect_schema_only() {
    let args = InspectArgs {
        path: "tests/fixtures/types.parquet".to_string(),
        rows: 10,
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
async fn test_inspect_with_stats() {
    let args = InspectArgs {
        path: "tests/fixtures/sample.parquet".to_string(),
        rows: 5,
        schema: false,
        metadata: false,
        stats: true,
        format: None,
        output: "table".to_string(),
        columns: None,
        sample: false,
    };

    let result = InspectCommand::execute(args).await;
    assert!(result.is_ok(), "Inspect with stats should succeed");
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
        columns: Some(vec!["name".to_string(), "salary".to_string()]),
        sample: false,
    };

    let result = InspectCommand::execute(args).await;
    assert!(
        result.is_ok(),
        "Inspect with column filtering should succeed"
    );
}

#[tokio::test]
async fn test_inspect_json_output() {
    let args = InspectArgs {
        path: "tests/fixtures/sample.parquet".to_string(),
        rows: 5,
        schema: false,
        metadata: false,
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
    assert!(result.is_err(), "Nonexistent file should return error");
}

#[tokio::test]
async fn test_inspect_larger_file() {
    let args = InspectArgs {
        path: "tests/fixtures/larger.parquet".to_string(),
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
