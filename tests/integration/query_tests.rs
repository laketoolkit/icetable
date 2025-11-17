//! Integration tests for query operations

use std::path::Path;

use arrow::array::Array;
use tablectl::core::operations::query::{QueryOperation, SqlPathExtractor};

#[tokio::test]
async fn test_extract_single_file_path() {
    let sql = "SELECT * FROM 'tests/fixtures/sample.parquet'";
    let refs = SqlPathExtractor::extract_file_paths(sql).expect("Failed to extract paths");

    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].path, "tests/fixtures/sample.parquet");
    assert_eq!(refs[0].table_name, "sample");
}

#[tokio::test]
async fn test_extract_file_path_with_alias() {
    let sql = "SELECT * FROM 'tests/fixtures/sample.parquet' s WHERE s.age > 25";
    let refs = SqlPathExtractor::extract_file_paths(sql).expect("Failed to extract paths");

    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].path, "tests/fixtures/sample.parquet");
    assert_eq!(refs[0].alias, Some("s".to_string()));
    assert_eq!(refs[0].table_name, "s");
}

#[tokio::test]
async fn test_query_single_file() {
    let operation = QueryOperation::new();

    let sql = "SELECT * FROM 'tests/fixtures/sample.parquet'";
    let result = operation.execute(sql, None).await.expect("Query failed");

    assert!(result.row_count > 0);
    assert_eq!(result.tables_accessed, 1);
    assert_eq!(result.schema.fields().len(), 5); // id, name, age, salary, active
}

#[tokio::test]
async fn test_query_with_filter() {
    let operation = QueryOperation::new();

    let sql = "SELECT * FROM 'tests/fixtures/sample.parquet' WHERE age > 25";
    let result = operation.execute(sql, None).await.expect("Query failed");

    assert!(result.row_count > 0);
    assert_eq!(result.tables_accessed, 1);

    // Verify all returned rows have age > 25
    for batch in &result.batches {
        let age_col = batch
            .column(2) // age is the 3rd column (0-indexed)
            .as_any()
            .downcast_ref::<arrow::array::Int32Array>()
            .expect("Age column should be Int32");

        for i in 0..batch.num_rows() {
            if !age_col.is_null(i) {
                assert!(age_col.value(i) > 25, "All ages should be > 25");
            }
        }
    }
}

#[tokio::test]
async fn test_query_with_projection() {
    let operation = QueryOperation::new();

    let sql = "SELECT name, age FROM 'tests/fixtures/sample.parquet'";
    let result = operation.execute(sql, None).await.expect("Query failed");

    assert!(result.row_count > 0);
    assert_eq!(result.schema.fields().len(), 2); // Only name and age

    let field_names: Vec<&str> = result
        .schema
        .fields()
        .iter()
        .map(|f| f.name().as_str())
        .collect();

    assert!(field_names.contains(&"name"));
    assert!(field_names.contains(&"age"));
}

#[tokio::test]
async fn test_query_with_limit() {
    let operation = QueryOperation::new();

    let sql = "SELECT * FROM 'tests/fixtures/sample.parquet'";

    // Execute without limit first to get total count
    let full_result = operation.execute(sql, None).await.expect("Query failed");
    let total_rows = full_result.row_count;

    // Execute with limit
    let limit = 3;
    let limited_result = operation
        .execute(sql, Some(limit))
        .await
        .expect("Query failed");

    assert_eq!(limited_result.row_count, limit);
    assert!(limited_result.row_count < total_rows);
}

#[tokio::test]
async fn test_query_with_aggregation() {
    let operation = QueryOperation::new();

    let sql = "SELECT COUNT(*) as count, AVG(age) as avg_age FROM 'tests/fixtures/sample.parquet'";
    let result = operation.execute(sql, None).await.expect("Query failed");

    assert_eq!(result.row_count, 1); // Aggregation returns single row
    assert_eq!(result.schema.fields().len(), 2); // count and avg_age

    let batch = &result.batches[0];
    let count_col = batch
        .column(0)
        .as_any()
        .downcast_ref::<arrow::array::Int64Array>()
        .expect("Count should be Int64");

    assert!(count_col.value(0) > 0);
}

#[tokio::test]
async fn test_query_with_order_by() {
    let operation = QueryOperation::new();

    let sql = "SELECT name, age FROM 'tests/fixtures/sample.parquet' ORDER BY age DESC LIMIT 5";
    let result = operation.execute(sql, None).await.expect("Query failed");

    assert!(result.row_count > 0);

    // Verify results are ordered by age descending
    for batch in &result.batches {
        let age_col = batch
            .column(1)
            .as_any()
            .downcast_ref::<arrow::array::Int32Array>()
            .expect("Age should be Int32");

        let mut prev_age = i32::MAX;
        for i in 0..batch.num_rows() {
            if !age_col.is_null(i) {
                let current_age = age_col.value(i);
                assert!(
                    current_age <= prev_age,
                    "Ages should be in descending order"
                );
                prev_age = current_age;
            }
        }
    }
}

#[tokio::test]
async fn test_query_file_not_found() {
    let operation = QueryOperation::new();

    let sql = "SELECT * FROM 'nonexistent/file.parquet'";
    let result = operation.execute(sql, None).await;

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(
        error.to_string().contains("File not found") || error.to_string().contains("No such file")
    );
}

#[tokio::test]
async fn test_query_invalid_sql() {
    let operation = QueryOperation::new();

    let sql = "SELCT * FORM 'tests/fixtures/sample.parquet'"; // Typos
    let result = operation.execute(sql, None).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_query_unquoted_path_validation() {
    let sql = "SELECT * FROM tests/fixtures/sample.parquet"; // Missing quotes
    let result = SqlPathExtractor::validate_quoted_paths(sql);

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.to_string().contains("must be quoted"));
}

#[tokio::test]
async fn test_schema_only() {
    let operation = QueryOperation::new();

    let sql = "SELECT name, age FROM 'tests/fixtures/sample.parquet'";
    let schema = operation
        .schema_only(sql)
        .await
        .expect("Schema query failed");

    assert_eq!(schema.fields().len(), 2);

    let field_names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();

    assert!(field_names.contains(&"name"));
    assert!(field_names.contains(&"age"));
}

#[tokio::test]
async fn test_empty_result() {
    let operation = QueryOperation::new();

    // Query that returns no rows
    let sql = "SELECT * FROM 'tests/fixtures/sample.parquet' WHERE 1 = 0";
    let result = operation.execute(sql, None).await.expect("Query failed");

    assert_eq!(result.row_count, 0);
    assert!(result.is_empty());
}

// Note: JOIN tests require multiple fixture files
// These can be added once we have CSV fixtures or multiple parquet files
//
// #[tokio::test]
// async fn test_query_join_parquet_csv() {
//     let operation = QueryOperation::new();
//
//     let sql = r#"
//         SELECT f.*, a.name as airline_name
//         FROM 'tests/fixtures/flights.parquet' f
//         JOIN 'tests/fixtures/airlines.csv' a ON f.airline_id = a.id
//     "#;
//
//     let result = operation.execute(sql, None).await.expect("Query failed");
//     assert_eq!(result.tables_accessed, 2);
// }
