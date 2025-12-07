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

use icetable::core::storage::{GetOptions, ListOptions, PutOptions, StorageBackendFactory};
use std::env;

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
    let result = StorageBackendFactory::create_backend(&path).await;

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
    let test_key = format!("test-{}/data.txt", uuid::Uuid::new_v4());
    let path = format!("s3://{}/{}", bucket, test_key);

    let storage = StorageBackendFactory::create_backend(&path)
        .await
        .expect("Failed to create storage");

    // Write test data
    let test_data = b"Hello from icetable integration test!";
    storage
        .put(&path, bytes::Bytes::from_static(test_data), &PutOptions::default())
        .await
        .expect("Failed to write data");

    // Read it back
    let read_data = storage
        .get(&path, &GetOptions::default())
        .await
        .expect("Failed to read data");

    assert_eq!(read_data.as_ref(), test_data);

    // Cleanup
    storage
        .delete(&path)
        .await
        .expect("Failed to delete test file");
}

#[tokio::test]
#[ignore]
async fn test_minio_list_objects() {
    require_minio!();

    let bucket = test_bucket();
    let prefix = format!("test-list-{}", uuid::Uuid::new_v4());
    let base_path = format!("s3://{}/{}", bucket, prefix);

    let storage = StorageBackendFactory::create_backend(&base_path)
        .await
        .expect("Failed to create storage");

    // Write multiple test files
    for i in 0..3 {
        let path = format!("{}/file{}.txt", base_path, i);
        storage
            .put(
                &path,
                bytes::Bytes::from(format!("content {}", i)),
                &PutOptions::default(),
            )
            .await
            .expect("Failed to write file");
    }

    // List objects
    let options = ListOptions {
        prefix: Some(base_path.clone()),
        ..Default::default()
    };
    let result = storage.list(&options).await.expect("Failed to list");
    let paths: Vec<_> = result.objects.iter().map(|o| o.path.clone()).collect();

    assert_eq!(paths.len(), 3, "Expected 3 objects, got: {:?}", paths);

    // Cleanup
    for i in 0..3 {
        let path = format!("{}/file{}.txt", base_path, i);
        storage.delete(&path).await.ok();
    }
}
