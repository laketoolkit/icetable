//! Unit tests for LocalStorageBackend

use bytes::Bytes;
use icectl::core::storage::{GetOptions, ListOptions, LocalBackend, PutOptions, StorageBackend};
use tempfile::TempDir;

fn create_backend() -> LocalBackend {
    LocalBackend::new().unwrap()
}

/// Create a temporary directory for write tests to avoid interfering with listing tests
fn create_temp_dir() -> TempDir {
    tempfile::tempdir().expect("Failed to create temp directory")
}

#[tokio::test]
async fn test_exists_file() {
    let backend = create_backend();
    let result = backend.exists("tests/fixtures/sample.parquet").await;

    assert!(result.is_ok());
    assert!(result.unwrap(), "File should exist");
}

#[tokio::test]
async fn test_exists_nonexistent_file() {
    let backend = create_backend();
    let result = backend.exists("tests/fixtures/nonexistent.parquet").await;

    assert!(result.is_ok());
    assert!(!result.unwrap(), "File should not exist");
}

#[tokio::test]
async fn test_get_file() {
    let backend = create_backend();
    let options = GetOptions::default();
    let result = backend.get("tests/fixtures/sample.parquet", &options).await;

    assert!(result.is_ok(), "Should read existing file");
    let data = result.unwrap();
    assert!(!data.is_empty(), "File should have content");

    // Check for Parquet magic bytes
    assert_eq!(&data[0..4], b"PAR1", "Should have Parquet magic bytes");
}

#[tokio::test]
async fn test_get_nonexistent_file() {
    let backend = create_backend();
    let options = GetOptions::default();
    let result = backend
        .get("tests/fixtures/nonexistent.parquet", &options)
        .await;

    assert!(result.is_err(), "Should fail for nonexistent file");
}

#[tokio::test]
async fn test_get_range() {
    let backend = create_backend();
    let result = backend
        .get_range("tests/fixtures/sample.parquet", 0, 4)
        .await;

    assert!(result.is_ok(), "Should read file range");
    let data = result.unwrap();
    assert_eq!(data.len(), 4);
    assert_eq!(&data[..], b"PAR1", "Should read Parquet magic bytes");
}

#[tokio::test]
async fn test_get_range_beyond_file() {
    let backend = create_backend();
    // Try to read beyond file size
    let result = backend
        .get_range("tests/fixtures/sample.parquet", 0, 1_000_000)
        .await;

    assert!(result.is_ok(), "Should handle large range request");
    let data = result.unwrap();
    // Should only return actual file size
    assert!(data.len() < 1_000_000);
}

#[tokio::test]
async fn test_head() {
    let backend = create_backend();
    let result = backend.head("tests/fixtures/sample.parquet").await;

    assert!(result.is_ok(), "Should get file metadata");
    let metadata = result.unwrap();

    assert!(metadata.size > 0, "File should have size");
    // last_modified is DateTime<Utc>, not Option
    // content_type is None for local backend
    assert_eq!(metadata.content_type, None);
}

#[tokio::test]
async fn test_head_nonexistent_file() {
    let backend = create_backend();
    let result = backend.head("tests/fixtures/nonexistent.parquet").await;

    assert!(result.is_err(), "Should fail for nonexistent file");
}

#[tokio::test]
async fn test_list_directory() {
    let backend = create_backend();
    let options = ListOptions {
        prefix: Some("tests/fixtures/".to_string()),
        delimiter: None,
        max_results: None,
        continuation_token: None,
    };

    let result = backend.list(&options).await;

    assert!(result.is_ok(), "Should list directory");
    let listing = result.unwrap();

    assert!(!listing.objects.is_empty(), "Should find files");

    // Check that expected files are in listing
    let paths: Vec<&str> = listing.objects.iter().map(|o| o.path.as_str()).collect();
    assert!(
        paths.iter().any(|p| p.contains("sample.parquet")),
        "Should find sample.parquet"
    );
    assert!(
        paths.iter().any(|p| p.contains("larger.parquet")),
        "Should find larger.parquet"
    );
}

#[tokio::test]
async fn test_list_with_max_results() {
    let backend = create_backend();
    let options = ListOptions {
        prefix: Some("tests/fixtures/".to_string()),
        delimiter: None,
        max_results: Some(2),
        continuation_token: None,
    };

    let result = backend.list(&options).await;

    assert!(result.is_ok(), "Should list with limit");
    let listing = result.unwrap();

    assert!(
        listing.objects.len() <= 2,
        "Should respect max_results limit"
    );
}

#[tokio::test]
async fn test_put_and_get_roundtrip() {
    let backend = create_backend();
    let temp_dir = create_temp_dir();
    let test_path = temp_dir.path().join("test_put.txt");
    let test_path_str = test_path.to_str().unwrap();
    let test_data = Bytes::from("Hello, World!");
    let put_options = PutOptions::default();

    // Write data
    let put_result = backend
        .put(test_path_str, test_data.clone(), &put_options)
        .await;
    assert!(put_result.is_ok(), "Should write file");

    // Read it back
    let get_options = GetOptions::default();
    let get_result = backend.get(test_path_str, &get_options).await;
    assert!(get_result.is_ok(), "Should read written file");

    let retrieved_data = get_result.unwrap();
    assert_eq!(retrieved_data, test_data, "Data should match");

    // Clean up (temp_dir drops automatically, but explicit delete tests the method)
    let delete_result = backend.delete(test_path_str).await;
    assert!(delete_result.is_ok(), "Should delete file");
}

#[tokio::test]
async fn test_delete_existing_file() {
    let backend = create_backend();
    let temp_dir = create_temp_dir();
    let test_path = temp_dir.path().join("test_delete.txt");
    let test_path_str = test_path.to_str().unwrap();
    let test_data = Bytes::from("Delete me");
    let put_options = PutOptions::default();

    // Create file
    backend
        .put(test_path_str, test_data, &put_options)
        .await
        .unwrap();

    // Verify it exists
    assert!(backend.exists(test_path_str).await.unwrap());

    // Delete it
    let delete_result = backend.delete(test_path_str).await;
    assert!(delete_result.is_ok(), "Should delete file");

    // Verify it's gone
    assert!(!backend.exists(test_path_str).await.unwrap());
}

#[tokio::test]
async fn test_delete_nonexistent_file() {
    let backend = create_backend();
    let result = backend.delete("tests/fixtures/nonexistent.txt").await;

    // Deleting nonexistent file should error with FileNotFound
    assert!(result.is_err(), "Deleting nonexistent file should fail");
}

#[tokio::test]
async fn test_copy_file() {
    let backend = create_backend();
    let temp_dir = create_temp_dir();
    let source = temp_dir.path().join("test_copy_source.txt");
    let dest = temp_dir.path().join("test_copy_dest.txt");
    let source_str = source.to_str().unwrap();
    let dest_str = dest.to_str().unwrap();
    let test_data = Bytes::from("Copy this data");
    let put_options = PutOptions::default();

    // Create source file
    backend
        .put(source_str, test_data.clone(), &put_options)
        .await
        .unwrap();

    // Copy it
    let copy_result = backend.copy(source_str, dest_str).await;
    assert!(copy_result.is_ok(), "Should copy file");

    // Verify destination exists and has same content
    let get_options = GetOptions::default();
    let dest_data = backend.get(dest_str, &get_options).await.unwrap();
    assert_eq!(dest_data, test_data, "Copied data should match");

    // Clean up (temp_dir drops automatically)
}

#[tokio::test]
async fn test_object_metadata_in_listing() {
    let backend = create_backend();
    let options = ListOptions {
        prefix: Some("tests/fixtures/".to_string()),
        delimiter: None,
        max_results: Some(5),
        continuation_token: None,
    };

    let result = backend.list(&options).await;
    assert!(result.is_ok(), "List should succeed");

    let listing = result.unwrap();
    assert!(!listing.objects.is_empty(), "Should find objects");

    let obj = &listing.objects[0];
    assert!(obj.size > 0, "Object should have size");
    // last_modified is DateTime<Utc>, not Option
}

#[tokio::test]
async fn test_concurrent_reads() {
    let _backend = create_backend();
    let get_options = GetOptions::default();

    // Launch multiple concurrent reads
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let backend = create_backend();
            let opts = get_options.clone();
            tokio::spawn(async move { backend.get("tests/fixtures/sample.parquet", &opts).await })
        })
        .collect();

    // Wait for all to complete
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok(), "Concurrent read should succeed");
    }
}

#[tokio::test]
async fn test_file_size_accuracy() {
    let backend = create_backend();

    // Get file size via head
    let metadata = backend.head("tests/fixtures/sample.parquet").await.unwrap();
    let head_size = metadata.size;

    // Get actual file content
    let get_options = GetOptions::default();
    let content = backend
        .get("tests/fixtures/sample.parquet", &get_options)
        .await
        .unwrap();

    assert_eq!(
        head_size,
        content.len() as u64,
        "Head size should match actual file size"
    );
}
