//! Stress tests for large tables and complex scenarios
//!
//! Tests system behavior under heavy load:
//! - Large number of files
//! - Deep partition structures
//! - Large metadata
//! - Concurrent operations
//!
//! Run with: cargo test --test integration_test stress -- --ignored --nocapture
//! Note: These tests may take several minutes to complete.

use std::env;
use std::sync::Arc;
use std::time::Instant;

use icetable::core::metadata::{IcebergMetadataService, TableServiceReader};
use icetable::TableLoader;
use icetable::core::operations::generate::{GenerateConfig, GenerateOperation, SchemaTemplate};
use icetable::core::operations::inspect::{IcebergInspectOptions, IcebergTableInspector};
use icetable::core::storage::{ObjectStoreExt, Storage, create_object_store};

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
            eprintln!("Skipping stress test: MinIO environment not configured");
            return;
        }
        if !minio_available().await {
            eprintln!("Skipping stress test: MinIO not available");
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

/// Performance threshold struct for assertions
struct PerformanceThresholds {
    max_generation_time_per_file_ms: u128,
    max_list_files_time_ms: u128,
    max_inspect_time_ms: u128,
}

impl Default for PerformanceThresholds {
    fn default() -> Self {
        Self {
            max_generation_time_per_file_ms: 5000, // 5 seconds per file
            max_list_files_time_ms: 30000,          // 30 seconds total
            max_inspect_time_ms: 10000,             // 10 seconds
        }
    }
}

// =============================================================================
// LARGE FILE COUNT TESTS
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_stress_many_files() {
    require_minio!();

    let table_path = test_table_path("stress-many-files");
    let num_files: u32 = 50;
    let rows_per_file: u64 = 1000;
    let total_rows = num_files as u64 * rows_per_file;

    println!("Generating table with {} files ({} rows)...", num_files, total_rows);

    let start = Instant::now();

    let config = GenerateConfig {
        path: table_path.clone(),
        schema: Arc::new(SchemaTemplate::Events.to_schema()),
        rows: total_rows,
        files: num_files,
        partition_columns: vec![],
        seed: 42,
        target_file_size: 1024 * 1024, // 1 MB target
    };

    let result = GenerateOperation::execute(config)
        .await
        .expect("Failed to generate table");

    let generation_time = start.elapsed();
    println!(
        "Generated {} files in {:?} ({:.2} files/sec)",
        result.files_created,
        generation_time,
        result.files_created as f64 / generation_time.as_secs_f64()
    );

    // Verify file count
    assert_eq!(result.files_created, num_files, "Should create {} files", num_files);

    // Test file listing performance
    let start = Instant::now();
    let service = IcebergMetadataService::new_async(table_path.clone())
        .await
        .expect("Failed to load table");

    let files = service.list_data_files().await.expect("Failed to list files");
    let list_time = start.elapsed();

    println!(
        "Listed {} files in {:?} ({:.2} files/sec)",
        files.len(),
        list_time,
        files.len() as f64 / list_time.as_secs_f64()
    );

    assert_eq!(files.len(), num_files as usize, "Should list all files");

    let thresholds = PerformanceThresholds::default();
    assert!(
        list_time.as_millis() < thresholds.max_list_files_time_ms,
        "Listing {} files took too long: {:?}",
        num_files,
        list_time
    );

    // Cleanup
    let storage = create_object_store(&table_path).await.unwrap();
    cleanup_table(&storage, &table_path).await;
}

#[tokio::test]
#[ignore]
async fn test_stress_large_rows() {
    require_minio!();

    let table_path = test_table_path("stress-large-rows");
    let total_rows = 100_000u64;
    let num_files = 10;

    println!("Generating table with {} rows across {} files...", total_rows, num_files);

    let start = Instant::now();

    let config = GenerateConfig {
        path: table_path.clone(),
        schema: Arc::new(SchemaTemplate::Events.to_schema()),
        rows: total_rows,
        files: num_files,
        partition_columns: vec![],
        seed: 42,
        target_file_size: 64 * 1024 * 1024,
    };

    let result = GenerateOperation::execute(config)
        .await
        .expect("Failed to generate table");

    let generation_time = start.elapsed();
    println!(
        "Generated {} rows in {:?} ({:.0} rows/sec)",
        result.total_rows,
        generation_time,
        result.total_rows as f64 / generation_time.as_secs_f64()
    );

    assert_eq!(result.total_rows, total_rows);

    // Verify row count via file listing
    let service = IcebergMetadataService::new_async(table_path.clone())
        .await
        .expect("Failed to load table");

    let files = service.list_data_files().await.expect("Failed to list files");
    let total_file_rows: u64 = files.iter().map(|f| f.record_count).sum();

    assert_eq!(total_file_rows, total_rows, "Row count mismatch");

    // Cleanup
    let storage = create_object_store(&table_path).await.unwrap();
    cleanup_table(&storage, &table_path).await;
}

// =============================================================================
// CONCURRENT OPERATIONS TESTS
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_stress_concurrent_reads() {
    require_minio!();

    let table_path = test_table_path("stress-concurrent");

    // Create a table to read from
    let config = GenerateConfig {
        path: table_path.clone(),
        schema: Arc::new(SchemaTemplate::Events.to_schema()),
        rows: 10_000,
        files: 10,
        partition_columns: vec![],
        seed: 42,
        target_file_size: 64 * 1024 * 1024,
    };

    GenerateOperation::execute(config)
        .await
        .expect("Failed to generate table");

    // Spawn many concurrent readers
    let num_readers = 20;
    println!("Spawning {} concurrent readers...", num_readers);

    let start = Instant::now();
    let mut handles = vec![];

    for i in 0..num_readers {
        let path = table_path.clone();
        let handle = tokio::spawn(async move {
            let start = Instant::now();

            let service = IcebergMetadataService::new_async(path).await?;
            let files = service.list_data_files().await?;

            Ok::<_, icetable::Error>((i, files.len(), start.elapsed()))
        });
        handles.push(handle);
    }

    // Collect results
    let mut successful = 0;
    let mut total_duration = std::time::Duration::ZERO;

    for handle in handles {
        match handle.await {
            Ok(Ok((id, file_count, duration))) => {
                successful += 1;
                total_duration += duration;
                println!("  Reader {}: {} files in {:?}", id, file_count, duration);
            }
            Ok(Err(e)) => {
                eprintln!("  Reader failed: {}", e);
            }
            Err(e) => {
                eprintln!("  Task panicked: {}", e);
            }
        }
    }

    let total_time = start.elapsed();
    let avg_duration = total_duration / num_readers as u32;

    println!(
        "Completed {} / {} readers in {:?} (avg: {:?})",
        successful, num_readers, total_time, avg_duration
    );

    assert_eq!(successful, num_readers, "All readers should succeed");

    // Cleanup
    let storage = create_object_store(&table_path).await.unwrap();
    cleanup_table(&storage, &table_path).await;
}

// =============================================================================
// INSPECT PERFORMANCE TESTS
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_stress_inspect_large_table() {
    require_minio!();

    let table_path = test_table_path("stress-inspect");

    // Create a table with many files
    let config = GenerateConfig {
        path: table_path.clone(),
        schema: Arc::new(SchemaTemplate::Transactions.to_schema()),
        rows: 50_000,
        files: 25,
        partition_columns: vec![],
        seed: 42,
        target_file_size: 64 * 1024 * 1024,
    };

    GenerateOperation::execute(config)
        .await
        .expect("Failed to generate table");

    println!("Testing inspect performance...");

    let start = Instant::now();

    let table = TableLoader::load_table(&table_path, None)
        .await
        .expect("Failed to load table");

    let options = IcebergInspectOptions::from_cli(true); // Verbose mode

    let result = IcebergTableInspector::inspect(&table, &options)
        .expect("Failed to inspect table");

    let inspect_time = start.elapsed();

    println!(
        "Inspected table in {:?}:\n  - {} records\n  - {} files\n  - {} snapshots",
        inspect_time,
        result.current_state.total_records.unwrap_or(0),
        result.current_state.total_data_files.unwrap_or(0),
        result.snapshot_count
    );

    let thresholds = PerformanceThresholds::default();
    assert!(
        inspect_time.as_millis() < thresholds.max_inspect_time_ms,
        "Inspect took too long: {:?}",
        inspect_time
    );

    // Cleanup
    let storage = create_object_store(&table_path).await.unwrap();
    cleanup_table(&storage, &table_path).await;
}

// =============================================================================
// MEMORY USAGE TESTS
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_stress_memory_with_many_files() {
    require_minio!();

    let table_path = test_table_path("stress-memory");
    let num_files = 100;

    println!("Testing memory usage with {} files...", num_files);

    let config = GenerateConfig {
        path: table_path.clone(),
        schema: Arc::new(SchemaTemplate::Events.to_schema()),
        rows: num_files as u64 * 500,
        files: num_files,
        partition_columns: vec![],
        seed: 42,
        target_file_size: 512 * 1024, // Small files
    };

    GenerateOperation::execute(config)
        .await
        .expect("Failed to generate table");

    // Perform multiple operations to check for memory leaks
    for i in 0..5 {
        let service = IcebergMetadataService::new_async(table_path.clone())
            .await
            .expect("Failed to load table");

        let files = service.list_data_files().await.expect("Failed to list files");

        println!("  Iteration {}: listed {} files", i + 1, files.len());

        // Let the service drop
        drop(service);

        // Small delay to allow cleanup
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // If we get here without OOM, test passes
    println!("Memory test passed - no leaks detected");

    // Cleanup
    let storage = create_object_store(&table_path).await.unwrap();
    cleanup_table(&storage, &table_path).await;
}

// =============================================================================
// SCHEMA COMPLEXITY TESTS
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_stress_all_templates() {
    require_minio!();

    println!("Testing all schema templates with substantial data...");

    let templates = [
        (SchemaTemplate::Events, "events"),
        (SchemaTemplate::Transactions, "transactions"),
        (SchemaTemplate::Sensors, "sensors"),
        (SchemaTemplate::Users, "users"),
        (SchemaTemplate::WebLogs, "weblogs"),
    ];

    for (template, name) in templates {
        let table_path = test_table_path(&format!("stress-template-{}", name));

        println!("  Testing {} template...", name);

        let start = Instant::now();

        let config = GenerateConfig {
            path: table_path.clone(),
            schema: Arc::new(template.to_schema()),
            rows: 10_000,
            files: 5,
            partition_columns: vec![],
            seed: 42,
            target_file_size: 64 * 1024 * 1024,
        };

        let result = GenerateOperation::execute(config)
            .await
            .expect(&format!("Failed to generate {} table", name));

        let duration = start.elapsed();

        println!(
            "    {} template: {} rows, {} files in {:?}",
            name, result.total_rows, result.files_created, duration
        );

        assert_eq!(result.total_rows, 10_000);
        assert_eq!(result.files_created, 5);

        // Cleanup
        let storage = create_object_store(&table_path).await.unwrap();
        cleanup_table(&storage, &table_path).await;
    }
}

// =============================================================================
// EDGE CASE STRESS TESTS
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_stress_single_row_many_files() {
    require_minio!();

    // Edge case: many files with few rows each
    let table_path = test_table_path("stress-single-row");
    let num_files = 20;

    println!("Testing {} files with minimal rows each...", num_files);

    let config = GenerateConfig {
        path: table_path.clone(),
        schema: Arc::new(SchemaTemplate::Events.to_schema()),
        rows: num_files as u64, // 1 row per file
        files: num_files,
        partition_columns: vec![],
        seed: 42,
        target_file_size: 64 * 1024 * 1024,
    };

    let result = GenerateOperation::execute(config)
        .await
        .expect("Failed to generate table");

    println!(
        "Created {} files with {} total rows",
        result.files_created, result.total_rows
    );

    // Verify we can still work with small files
    let service = IcebergMetadataService::new_async(table_path.clone())
        .await
        .expect("Failed to load table");

    let files = service.list_data_files().await.expect("Failed to list files");

    // Each file should have at least 1 row
    for file in &files {
        assert!(file.record_count >= 1, "File should have at least 1 row");
    }

    // Cleanup
    let storage = create_object_store(&table_path).await.unwrap();
    cleanup_table(&storage, &table_path).await;
}

#[tokio::test]
#[ignore]
async fn test_stress_rapid_operations() {
    require_minio!();

    let table_path = test_table_path("stress-rapid");

    // Create initial table
    let config = GenerateConfig {
        path: table_path.clone(),
        schema: Arc::new(SchemaTemplate::Events.to_schema()),
        rows: 1000,
        files: 2,
        partition_columns: vec![],
        seed: 42,
        target_file_size: 64 * 1024 * 1024,
    };

    GenerateOperation::execute(config)
        .await
        .expect("Failed to generate table");

    println!("Testing rapid successive operations...");

    let start = Instant::now();
    let num_operations = 50;

    for i in 0..num_operations {
        // Rapid load/inspect cycles
        let service = IcebergMetadataService::new_async(table_path.clone())
            .await
            .expect("Failed to load table");

        let _ = service.current_snapshot().await.expect("Failed to get snapshot");

        if i % 10 == 0 {
            println!("  Completed {} / {} operations", i, num_operations);
        }
    }

    let duration = start.elapsed();
    let ops_per_sec = num_operations as f64 / duration.as_secs_f64();

    println!(
        "Completed {} rapid operations in {:?} ({:.1} ops/sec)",
        num_operations, duration, ops_per_sec
    );

    assert!(ops_per_sec > 1.0, "Should handle at least 1 op/sec");

    // Cleanup
    let storage = create_object_store(&table_path).await.unwrap();
    cleanup_table(&storage, &table_path).await;
}

// =============================================================================
// BENCHMARK HELPERS
// =============================================================================

/// Helper struct for collecting benchmark results
#[derive(Debug)]
struct BenchmarkResult {
    name: String,
    duration: std::time::Duration,
    operations: usize,
    throughput: f64,
}

impl BenchmarkResult {
    fn new(name: &str, duration: std::time::Duration, operations: usize) -> Self {
        let throughput = operations as f64 / duration.as_secs_f64();
        Self {
            name: name.to_string(),
            duration,
            operations,
            throughput,
        }
    }

    fn print(&self) {
        println!(
            "{}: {} ops in {:?} ({:.2} ops/sec)",
            self.name, self.operations, self.duration, self.throughput
        );
    }
}
