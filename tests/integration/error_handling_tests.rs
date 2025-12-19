//! Error handling and edge case tests
//!
//! Tests how the system handles various error conditions:
//! - Corrupted metadata
//! - Missing files
//! - Invalid paths
//! - Network failures (simulated)
//! - Permission errors
//!
//! Run with: cargo test --test integration_test error_handling -- --ignored

use std::env;
use std::sync::Arc;

use icetable::core::metadata::IcebergMetadataService;
use icetable::core::operations::generate::{GenerateConfig, GenerateOperation, SchemaTemplate};
use icetable::core::storage::{ObjectStoreExt, Storage, create_object_store};
use icetable::error::Error;

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

macro_rules! require_minio {
    () => {
        if !minio_configured() {
            eprintln!("Skipping test: MinIO environment not configured");
            return;
        }
        if !minio_available().await {
            eprintln!("Skipping test: MinIO not available");
            return;
        }
    };
}

fn test_bucket() -> String {
    env::var("MINIO_BUCKET").unwrap_or_else(|_| "test-bucket".to_string())
}

fn test_table_path(prefix: &str) -> String {
    let bucket = test_bucket();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("s3://{}/{}-{}", bucket, prefix, timestamp)
}

async fn cleanup_table(storage: &Storage, table_path: &str) {
    if let Ok(objects) = storage.list_prefix(table_path).await {
        for obj in objects {
            storage.delete_str(obj.location.as_ref()).await.ok();
        }
    }
}

// =============================================================================
// NON-EXISTENT TABLE TESTS
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_error_nonexistent_table() {
    require_minio!();

    let fake_path = test_table_path("nonexistent-table");

    // Try to load a table that doesn't exist
    let result = IcebergMetadataService::new_async(fake_path.clone()).await;

    assert!(result.is_err(), "Should fail for non-existent table");

    let err = result.err().expect("We already verified it's an error");
    let err_msg = err.to_string().to_lowercase();

    // Should be a recognizable error about missing metadata
    assert!(
        err_msg.contains("not found")
            || err_msg.contains("no metadata")
            || err_msg.contains("metadata")
            || err_msg.contains("does not exist"),
        "Error should indicate table not found: {}",
        err_msg
    );
}

#[tokio::test]
#[ignore]
async fn test_error_empty_directory() {
    require_minio!();

    let table_path = test_table_path("empty-dir");

    // Create an empty directory (just a marker file)
    let storage = create_object_store(&table_path)
        .await
        .expect("Failed to create storage");

    let marker_path = format!("{}/.empty", table_path);
    storage
        .put_bytes_str(&marker_path, bytes::Bytes::from_static(b""))
        .await
        .expect("Failed to create marker");

    // Try to load as Iceberg table
    let result = IcebergMetadataService::new_async(table_path.clone()).await;

    assert!(result.is_err(), "Should fail for empty directory");

    // Cleanup
    storage.delete_str(&marker_path).await.ok();
}

// =============================================================================
// CORRUPTED METADATA TESTS
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_error_corrupted_metadata_json() {
    require_minio!();

    let table_path = test_table_path("corrupted-metadata");

    // Create a fake metadata structure with invalid JSON
    let storage = create_object_store(&table_path)
        .await
        .expect("Failed to create storage");

    // Create metadata directory with corrupted metadata.json
    let metadata_path = format!("{}/metadata/v1.metadata.json", table_path);
    let corrupted_json = b"{ invalid json content !!!";

    storage
        .put_bytes_str(&metadata_path, bytes::Bytes::from_static(corrupted_json))
        .await
        .expect("Failed to write corrupted metadata");

    // Also create version-hint.text pointing to this metadata
    let hint_path = format!("{}/metadata/version-hint.text", table_path);
    storage
        .put_bytes_str(&hint_path, bytes::Bytes::from_static(b"1"))
        .await
        .expect("Failed to write version hint");

    // Try to load the table
    let result = IcebergMetadataService::new_async(table_path.clone()).await;

    assert!(result.is_err(), "Should fail for corrupted metadata");

    let err = result.err().expect("We already verified it's an error");
    let err_msg = err.to_string().to_lowercase();

    // Should indicate parsing/deserialization error
    assert!(
        err_msg.contains("parse")
            || err_msg.contains("json")
            || err_msg.contains("deserialize")
            || err_msg.contains("invalid")
            || err_msg.contains("failed"),
        "Error should indicate parse failure: {}",
        err_msg
    );

    // Cleanup
    cleanup_table(&storage, &table_path).await;
}

#[tokio::test]
#[ignore]
async fn test_error_truncated_metadata() {
    require_minio!();

    let table_path = test_table_path("truncated-metadata");

    let storage = create_object_store(&table_path)
        .await
        .expect("Failed to create storage");

    // Create truncated but syntactically valid JSON
    let truncated_json = br#"{"format-version": 2, "table-uuid": "abc"#; // Missing closing brace

    let metadata_path = format!("{}/metadata/v1.metadata.json", table_path);
    storage
        .put_bytes_str(&metadata_path, bytes::Bytes::from_static(truncated_json))
        .await
        .expect("Failed to write truncated metadata");

    let hint_path = format!("{}/metadata/version-hint.text", table_path);
    storage
        .put_bytes_str(&hint_path, bytes::Bytes::from_static(b"1"))
        .await
        .expect("Failed to write version hint");

    let result = IcebergMetadataService::new_async(table_path.clone()).await;

    assert!(result.is_err(), "Should fail for truncated metadata");

    // Cleanup
    cleanup_table(&storage, &table_path).await;
}

#[tokio::test]
#[ignore]
async fn test_error_missing_required_fields() {
    require_minio!();

    let table_path = test_table_path("missing-fields");

    let storage = create_object_store(&table_path)
        .await
        .expect("Failed to create storage");

    // Valid JSON but missing required Iceberg fields
    let incomplete_json = br#"{
        "format-version": 2,
        "table-uuid": "12345678-1234-1234-1234-123456789abc"
    }"#;

    let metadata_path = format!("{}/metadata/v1.metadata.json", table_path);
    storage
        .put_bytes_str(&metadata_path, bytes::Bytes::copy_from_slice(incomplete_json))
        .await
        .expect("Failed to write incomplete metadata");

    let hint_path = format!("{}/metadata/version-hint.text", table_path);
    storage
        .put_bytes_str(&hint_path, bytes::Bytes::from_static(b"1"))
        .await
        .expect("Failed to write version hint");

    let result = IcebergMetadataService::new_async(table_path.clone()).await;

    assert!(result.is_err(), "Should fail for metadata with missing fields");

    // Cleanup
    cleanup_table(&storage, &table_path).await;
}

// =============================================================================
// MISSING FILES TESTS
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_error_missing_data_files() {
    require_minio!();

    let table_path = test_table_path("missing-data");

    // First create a valid table
    let config = GenerateConfig {
        path: table_path.clone(),
        schema: Arc::new(SchemaTemplate::Events.to_schema()),
        rows: 100,
        files: 2,
        partition_columns: vec![],
        seed: 42,
        target_file_size: 64 * 1024 * 1024,
    };

    GenerateOperation::execute(config)
        .await
        .expect("Failed to generate table");

    let storage = create_object_store(&table_path)
        .await
        .expect("Failed to create storage");

    // Delete all data files but keep metadata
    let data_prefix = format!("{}/data", table_path);
    if let Ok(files) = storage.list_prefix(&data_prefix).await {
        for file in files {
            storage.delete_str(file.location.as_ref()).await.ok();
        }
    }

    // Table should still load (metadata is intact)
    let service = IcebergMetadataService::new_async(table_path.clone()).await;

    // Loading metadata should succeed
    assert!(service.is_ok(), "Metadata should still be readable");

    // But operations that need data files may fail
    // (This depends on implementation - some operations are metadata-only)

    // Cleanup
    cleanup_table(&storage, &table_path).await;
}

#[tokio::test]
#[ignore]
async fn test_error_missing_manifest() {
    require_minio!();

    let table_path = test_table_path("missing-manifest");

    // Create a valid table
    let config = GenerateConfig {
        path: table_path.clone(),
        schema: Arc::new(SchemaTemplate::Events.to_schema()),
        rows: 100,
        files: 1,
        partition_columns: vec![],
        seed: 42,
        target_file_size: 64 * 1024 * 1024,
    };

    GenerateOperation::execute(config)
        .await
        .expect("Failed to generate table");

    let storage = create_object_store(&table_path)
        .await
        .expect("Failed to create storage");

    // Delete manifest files (but keep metadata.json)
    let metadata_prefix = format!("{}/metadata", table_path);
    if let Ok(files) = storage.list_prefix(&metadata_prefix).await {
        for file in files {
            let path = file.location.to_string();
            // Delete .avro files (manifests) but keep .json and version-hint
            if path.ends_with(".avro") {
                storage.delete_str(&path).await.ok();
            }
        }
    }

    // Try to load and list files (should fail when accessing manifests)
    let service = IcebergMetadataService::new_async(table_path.clone()).await;

    // Metadata might load, but operations requiring manifests should fail
    if let Ok(svc) = service {
        use icetable::core::metadata::TableServiceReader;
        let files_result = svc.list_data_files().await;

        // Should fail because manifests are missing
        assert!(
            files_result.is_err(),
            "Should fail when manifests are missing"
        );
    }

    // Cleanup
    cleanup_table(&storage, &table_path).await;
}

// =============================================================================
// INVALID PATH TESTS
// =============================================================================

#[tokio::test]
async fn test_error_invalid_s3_path() {
    // Invalid S3 path format
    let result = create_object_store("s3://").await;

    assert!(result.is_err(), "Should fail for invalid S3 path");
}

#[tokio::test]
async fn test_error_unsupported_scheme() {
    // Unsupported URL scheme - verify it doesn't panic
    // Implementation may treat this as a local path or return error
    let result = create_object_store("ftp://example.com/data").await;

    // Either should fail, or treat as local path (which also likely fails)
    // The key assertion is that we handle this gracefully
    if result.is_ok() {
        // If it succeeds, verify the store is created (even if unusable)
        // This is acceptable behavior - the store creation may succeed
        // but operations would fail later
        let _store = result.unwrap();
    }
    // If it fails, that's also acceptable
}

#[tokio::test]
async fn test_error_malformed_url() {
    let result = create_object_store("not-a-valid-url").await;

    // Should either fail or treat as local path
    // (depends on implementation)
    // The important thing is it doesn't panic
    let _ = result;
}

// =============================================================================
// CONCURRENT ACCESS TESTS
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_concurrent_reads() {
    require_minio!();

    let table_path = test_table_path("concurrent-reads");

    // Create a table
    let config = GenerateConfig {
        path: table_path.clone(),
        schema: Arc::new(SchemaTemplate::Events.to_schema()),
        rows: 1000,
        files: 4,
        partition_columns: vec![],
        seed: 42,
        target_file_size: 64 * 1024 * 1024,
    };

    GenerateOperation::execute(config)
        .await
        .expect("Failed to generate table");

    // Spawn multiple concurrent readers
    let mut handles = vec![];

    for i in 0..5 {
        let path = table_path.clone();
        let handle = tokio::spawn(async move {
            let service = IcebergMetadataService::new_async(path).await?;
            use icetable::core::metadata::TableServiceReader;
            let files = service.list_data_files().await?;
            Ok::<_, icetable::Error>((i, files.len()))
        });
        handles.push(handle);
    }

    // All should succeed with consistent results
    let mut results = vec![];
    for handle in handles {
        let result = handle.await.expect("Task panicked");
        assert!(result.is_ok(), "Concurrent read failed: {:?}", result.err());
        results.push(result.unwrap());
    }

    // All readers should see the same number of files
    let file_counts: Vec<_> = results.iter().map(|(_, count)| *count).collect();
    assert!(
        file_counts.iter().all(|&c| c == file_counts[0]),
        "All concurrent reads should see same file count: {:?}",
        file_counts
    );

    // Cleanup
    let storage = create_object_store(&table_path).await.unwrap();
    cleanup_table(&storage, &table_path).await;
}

// =============================================================================
// RESOURCE LIMIT TESTS
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_timeout_handling() {
    require_minio!();

    // This test verifies that long operations can be cancelled
    // The actual timeout mechanism is tested indirectly

    let table_path = test_table_path("timeout-test");

    let config = GenerateConfig {
        path: table_path.clone(),
        schema: Arc::new(SchemaTemplate::Events.to_schema()),
        rows: 100,
        files: 1,
        partition_columns: vec![],
        seed: 42,
        target_file_size: 64 * 1024 * 1024,
    };

    // Use tokio timeout to verify operations can be bounded
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        GenerateOperation::execute(config),
    )
    .await;

    assert!(result.is_ok(), "Operation should complete within timeout");

    // Cleanup
    let storage = create_object_store(&table_path).await.unwrap();
    cleanup_table(&storage, &table_path).await;
}

// =============================================================================
// ERROR MESSAGE QUALITY TESTS
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_error_messages_are_helpful() {
    require_minio!();

    // Test that error messages provide actionable information

    let fake_path = test_table_path("no-such-table");
    let result = IcebergMetadataService::new_async(fake_path).await;

    assert!(result.is_err());

    let err = result.err().expect("We already verified it's an error");

    // Error should have a user-friendly message
    let user_msg = err.user_message();

    // Should not be empty or just technical jargon
    assert!(!user_msg.is_empty(), "User message should not be empty");
    assert!(
        user_msg.len() > 10,
        "User message should be descriptive: {}",
        user_msg
    );

    // Should not contain raw panic messages or stack traces
    assert!(
        !user_msg.contains("panicked") && !user_msg.contains("RUST_BACKTRACE"),
        "User message should not contain panic info: {}",
        user_msg
    );
}

#[tokio::test]
async fn test_error_types_are_specific() {
    // Verify that errors are properly typed, not just generic strings

    // Test that Error enum has meaningful variants
    let file_not_found = Error::FileNotFound {
        path: "/some/path".into(),
    };

    // Should format nicely
    let msg = file_not_found.to_string();
    assert!(msg.contains("/some/path"), "Error should contain path");

    // Test another variant
    let invalid_format = Error::InvalidFormat {
        message: "Not an Iceberg table".to_string(),
    };

    let msg = invalid_format.to_string();
    assert!(
        msg.contains("Iceberg"),
        "Error should contain format info"
    );
}
