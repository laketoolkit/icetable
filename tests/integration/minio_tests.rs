//! MinIO integration tests for S3-compatible storage
//!
//! These tests require a running MinIO instance with environment variables set.
//!
//! Setup:
//! ```bash
//! docker-compose -f docker-compose.test.yml up -d
//! export AWS_ACCESS_KEY_ID=minioadmin
//! export AWS_SECRET_ACCESS_KEY=minioadmin
//! export AWS_ENDPOINT_URL=http://localhost:9000
//! export AWS_ALLOW_HTTP=true
//! export AWS_REGION=us-east-1
//! export MINIO_BUCKET=test-bucket
//! cargo test --test integration_test minio -- --ignored
//! ```

use std::env;
use std::sync::Arc;

use icetable::core::operations::generate::{GenerateConfig, GenerateOperation, SchemaTemplate};
use icetable::core::storage::{ObjectStoreExt, Storage, create_object_store};

/// Get test bucket from environment or default
fn test_bucket() -> String {
    env::var("MINIO_BUCKET").unwrap_or_else(|_| "test-bucket".to_string())
}

/// Check if MinIO environment is configured
fn minio_configured() -> bool {
    env::var("AWS_ENDPOINT_URL").is_ok() && env::var("AWS_ACCESS_KEY_ID").is_ok()
}

/// Check if MinIO is available
async fn minio_available() -> bool {
    let endpoint = match env::var("AWS_ENDPOINT_URL") {
        Ok(e) => e,
        Err(_) => return false,
    };

    let client = reqwest::Client::new();
    match client
        .get(format!("{}/minio/health/live", endpoint))
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Skip test if MinIO is not configured or available
macro_rules! require_minio {
    () => {
        if !minio_configured() {
            eprintln!("Skipping test: MinIO environment not configured");
            eprintln!("Set AWS_ENDPOINT_URL, AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY");
            return;
        }
        if !minio_available().await {
            eprintln!(
                "Skipping test: MinIO not available at {}",
                env::var("AWS_ENDPOINT_URL").unwrap_or_default()
            );
            eprintln!("Start MinIO with: docker-compose -f docker-compose.test.yml up -d");
            return;
        }
    };
}

/// Generate a unique table path for tests
fn test_table_path(prefix: &str) -> String {
    let bucket = test_bucket();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("s3://{}/{}-{}", bucket, prefix, timestamp)
}

// =============================================================================
// Basic Storage Tests
// =============================================================================

#[tokio::test]
#[ignore] // Run with: cargo test minio -- --ignored
async fn test_minio_connectivity() {
    require_minio!();

    let endpoint = env::var("AWS_ENDPOINT_URL").unwrap();
    println!("MinIO is available at {}", endpoint);

    // Verify we can create a storage backend
    let bucket = test_bucket();
    let path = format!("s3://{}/test-connectivity", bucket);

    // Try to create storage backend (reads env vars internally)
    let result = create_object_store(&path).await;

    assert!(
        result.is_ok(),
        "Failed to create S3 storage backend: {:?}",
        result.err()
    );
}

#[tokio::test]
#[ignore]
async fn test_minio_write_read() {
    require_minio!();

    let bucket = test_bucket();
    let test_key = format!(
        "test-{}/data.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let path = format!("s3://{}/{}", bucket, test_key);

    let storage = create_object_store(&path)
        .await
        .expect("Failed to create storage");

    // Write test data
    let test_data = b"Hello from icetable integration test!";
    storage
        .put_bytes_str(&path, bytes::Bytes::from_static(test_data))
        .await
        .expect("Failed to write data");

    // Read it back
    let read_data = storage
        .get_bytes_str(&path)
        .await
        .expect("Failed to read data");

    assert_eq!(read_data.as_ref(), test_data);

    // Cleanup
    storage
        .delete_str(&path)
        .await
        .expect("Failed to delete test file");
}

#[tokio::test]
#[ignore]
async fn test_minio_list_objects() {
    require_minio!();

    let bucket = test_bucket();
    let prefix = format!(
        "test-list-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let base_path = format!("s3://{}/{}", bucket, prefix);

    let storage = create_object_store(&base_path)
        .await
        .expect("Failed to create storage");

    // Write multiple test files
    for i in 0..3 {
        let path = format!("{}/file{}.txt", base_path, i);
        storage
            .put_bytes_str(&path, bytes::Bytes::from(format!("content {}", i)))
            .await
            .expect("Failed to write file");
    }

    // List objects
    let result = storage
        .list_prefix(&base_path)
        .await
        .expect("Failed to list");
    let paths: Vec<_> = result.iter().map(|o| o.location.to_string()).collect();

    assert_eq!(paths.len(), 3, "Expected 3 objects, got: {:?}", paths);

    // Cleanup
    for i in 0..3 {
        let path = format!("{}/file{}.txt", base_path, i);
        storage.delete_str(&path).await.ok();
    }
}

// =============================================================================
// Generate Command Tests
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_minio_generate_synthetic_table() {
    require_minio!();

    let table_path = test_table_path("test-generate");

    // Generate synthetic table on MinIO
    let config = GenerateConfig {
        path: table_path.clone(),
        schema: Arc::new(SchemaTemplate::Events.to_schema()),
        rows: 1000,
        files: 2,
        partition_columns: vec![],
        seed: 42,
        target_file_size: 64 * 1024 * 1024,
    };

    let result = GenerateOperation::execute(config).await;
    assert!(
        result.is_ok(),
        "Failed to generate table: {:?}",
        result.err()
    );

    let result = result.unwrap();
    assert_eq!(result.total_rows, 1000);
    assert_eq!(result.files_created, 2);
    assert_eq!(result.data_files.len(), 2);
    assert!(result.snapshot_id > 0, "Snapshot ID should be positive");

    // Verify files exist in MinIO
    let storage = create_object_store(&table_path)
        .await
        .expect("Failed to create storage");

    // Check data files
    let data_prefix = format!("{}/data", table_path);
    let list_result = storage
        .list_prefix(&data_prefix)
        .await
        .expect("Failed to list");
    assert_eq!(
        list_result.len(),
        2,
        "Expected 2 parquet files, found: {:?}",
        list_result
    );

    // Verify each parquet file can be read
    for file in &result.data_files {
        let data = storage
            .get_bytes_str(&file.path)
            .await
            .expect("Failed to read parquet file");
        assert!(!data.is_empty(), "Parquet file is empty");
    }

    // Check metadata files (should have metadata.json, manifest, manifest list)
    let metadata_prefix = format!("{}/metadata", table_path);
    let metadata_list = storage
        .list_prefix(&metadata_prefix)
        .await
        .expect("Failed to list metadata");

    // Should have: metadata.json, version-hint.text, manifest avro, manifest list avro
    assert!(
        metadata_list.len() >= 3,
        "Expected at least 3 metadata files, found: {:?}",
        metadata_list
            .iter()
            .map(|o| o.location.to_string())
            .collect::<Vec<_>>()
    );

    println!("Generated table successfully:");
    println!("  Path: {}", result.table_path);
    println!("  Rows: {}", result.total_rows);
    println!("  Files: {}", result.files_created);
    println!("  Snapshot: {}", result.snapshot_id);
    println!("  Total bytes: {}", result.total_bytes);

    // Cleanup
    cleanup_table(&storage, &table_path).await;
}

#[tokio::test]
#[ignore]
async fn test_minio_generate_with_seed_reproducibility() {
    require_minio!();

    let seed = 12345u64;

    let table1_path = test_table_path("test-seed-a");
    let table2_path = test_table_path("test-seed-b");

    let config1 = GenerateConfig {
        path: table1_path.clone(),
        schema: Arc::new(SchemaTemplate::Users.to_schema()),
        rows: 100,
        files: 1,
        partition_columns: vec![],
        seed,
        target_file_size: 64 * 1024 * 1024,
    };

    let config2 = GenerateConfig {
        path: table2_path.clone(),
        schema: Arc::new(SchemaTemplate::Users.to_schema()),
        rows: 100,
        files: 1,
        partition_columns: vec![],
        seed,
        target_file_size: 64 * 1024 * 1024,
    };

    let result1 = GenerateOperation::execute(config1)
        .await
        .expect("Failed to generate table 1");
    let result2 = GenerateOperation::execute(config2)
        .await
        .expect("Failed to generate table 2");

    // File sizes should be identical with the same seed
    assert_eq!(
        result1.data_files[0].size, result2.data_files[0].size,
        "Same seed should produce same file sizes"
    );

    // Cleanup
    let storage1 = create_object_store(&table1_path).await.unwrap();
    let storage2 = create_object_store(&table2_path).await.unwrap();

    cleanup_table(&storage1, &table1_path).await;
    cleanup_table(&storage2, &table2_path).await;
}

#[tokio::test]
#[ignore]
async fn test_minio_generate_all_templates() {
    require_minio!();

    let templates = [
        (SchemaTemplate::Events, "events"),
        (SchemaTemplate::Transactions, "transactions"),
        (SchemaTemplate::Sensors, "sensors"),
        (SchemaTemplate::Users, "users"),
        (SchemaTemplate::WebLogs, "weblogs"),
    ];

    for (template, name) in templates {
        let table_path = test_table_path(&format!("test-template-{}", name));

        let config = GenerateConfig {
            path: table_path.clone(),
            schema: Arc::new(template.to_schema()),
            rows: 500,
            files: 1,
            partition_columns: vec![],
            seed: 42,
            target_file_size: 64 * 1024 * 1024,
        };

        let result = GenerateOperation::execute(config).await;
        assert!(
            result.is_ok(),
            "Failed to generate {} template: {:?}",
            name,
            result.err()
        );

        let result = result.unwrap();
        assert_eq!(
            result.total_rows, 500,
            "Template {} has wrong row count",
            name
        );
        println!(
            "  Generated {} template: {} rows, {} bytes",
            name, result.total_rows, result.total_bytes
        );

        // Cleanup
        let storage = create_object_store(&table_path).await.unwrap();
        cleanup_table(&storage, &table_path).await;
    }
}

// =============================================================================
// Table Operations Tests (using generated tables)
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_minio_inspect_generated_table() {
    require_minio!();

    let table_path = test_table_path("test-inspect");

    // Generate table
    let config = GenerateConfig {
        path: table_path.clone(),
        schema: Arc::new(SchemaTemplate::Events.to_schema()),
        rows: 500,
        files: 2,
        partition_columns: vec![],
        seed: 42,
        target_file_size: 64 * 1024 * 1024,
    };

    let gen_result = GenerateOperation::execute(config)
        .await
        .expect("Failed to generate table");

    // Load and inspect the table metadata
    let storage = create_object_store(&table_path)
        .await
        .expect("Failed to create storage");

    // Read metadata JSON
    let metadata_bytes = storage
        .get_bytes_str(&gen_result.metadata_path)
        .await
        .expect("Failed to read metadata");

    let metadata: serde_json::Value =
        serde_json::from_slice(&metadata_bytes).expect("Failed to parse metadata JSON");

    // Verify metadata structure
    assert_eq!(metadata["format-version"], 2);
    assert!(metadata["current-snapshot-id"].as_i64().is_some());
    assert!(metadata["snapshots"].as_array().is_some());
    assert_eq!(metadata["snapshots"].as_array().unwrap().len(), 1);

    // Verify schema
    let schema = &metadata["schemas"][0];
    assert_eq!(schema["fields"].as_array().unwrap().len(), 5); // Events has 5 fields

    println!("Metadata verified:");
    println!("  Format version: {}", metadata["format-version"]);
    println!("  Current snapshot: {}", metadata["current-snapshot-id"]);
    println!("  UUID: {}", metadata["table-uuid"]);

    // Cleanup
    cleanup_table(&storage, &table_path).await;
}

#[tokio::test]
#[ignore]
async fn test_minio_multiple_appends() {
    require_minio!();

    let table_path = test_table_path("test-appends");

    // Generate initial table
    let config1 = GenerateConfig {
        path: table_path.clone(),
        schema: Arc::new(SchemaTemplate::Events.to_schema()),
        rows: 500,
        files: 1,
        partition_columns: vec![],
        seed: 100,
        target_file_size: 64 * 1024 * 1024,
    };

    let result1 = GenerateOperation::execute(config1)
        .await
        .expect("Failed to generate first batch");

    println!(
        "First batch: {} rows, snapshot {}",
        result1.total_rows, result1.snapshot_id
    );

    // Note: GenerateOperation creates a new table, not appends to existing
    // This test verifies that we can create tables with different seeds
    let table_path2 = test_table_path("test-appends-2");

    let config2 = GenerateConfig {
        path: table_path2.clone(),
        schema: Arc::new(SchemaTemplate::Events.to_schema()),
        rows: 500,
        files: 1,
        partition_columns: vec![],
        seed: 200,
        target_file_size: 64 * 1024 * 1024,
    };

    let result2 = GenerateOperation::execute(config2)
        .await
        .expect("Failed to generate second batch");

    println!(
        "Second batch: {} rows, snapshot {}",
        result2.total_rows, result2.snapshot_id
    );

    // Different seeds should produce different data
    assert_ne!(
        result1.snapshot_id, result2.snapshot_id,
        "Different tables should have different snapshots"
    );

    // Cleanup
    let storage1 = create_object_store(&table_path).await.unwrap();
    let storage2 = create_object_store(&table_path2).await.unwrap();
    cleanup_table(&storage1, &table_path).await;
    cleanup_table(&storage2, &table_path2).await;
}

// =============================================================================
// Helper Functions
// =============================================================================

async fn cleanup_table(storage: &Storage, table_path: &str) {
    // List all objects under the table path
    let list_result = storage.list_prefix(table_path).await;

    if let Ok(objects) = list_result {
        for obj in objects {
            storage.delete_str(obj.location.as_ref()).await.ok();
        }
    }
}
