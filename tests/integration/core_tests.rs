//! Integration tests for core modules
//!
//! Tests for format detection, utils, and other core functionality
//! that doesn't require external services.

use icetable::core::utils::{
    TableFormat, detect_table_format, extract_version_from_path, format_bytes,
    normalize_path, normalize_relative_path, parse_bytes,
};
use tempfile::TempDir;

// ============================================================================
// Format Detection Tests
// ============================================================================

#[test]
fn test_detect_table_format_local_iceberg() {
    let temp_dir = TempDir::new().unwrap();
    let table_path = temp_dir.path();

    // Create Iceberg metadata structure
    let metadata_dir = table_path.join("metadata");
    std::fs::create_dir_all(&metadata_dir).unwrap();
    std::fs::write(
        metadata_dir.join("v1.metadata.json"),
        r#"{"format-version": 2}"#,
    )
    .unwrap();

    let format = detect_table_format(table_path);
    assert_eq!(format, TableFormat::Iceberg);
}

#[test]
fn test_detect_table_format_local_delta() {
    let temp_dir = TempDir::new().unwrap();
    let table_path = temp_dir.path();

    // Create Delta log structure
    let delta_log = table_path.join("_delta_log");
    std::fs::create_dir_all(&delta_log).unwrap();
    std::fs::write(delta_log.join("00000000000000000000.json"), "{}").unwrap();

    let format = detect_table_format(table_path);
    assert_eq!(format, TableFormat::Delta);
}

#[test]
fn test_detect_table_format_unknown() {
    let temp_dir = TempDir::new().unwrap();
    let format = detect_table_format(temp_dir.path());
    assert_eq!(format, TableFormat::Unknown);
}

#[test]
fn test_delta_takes_precedence_over_iceberg() {
    let temp_dir = TempDir::new().unwrap();
    let table_path = temp_dir.path();

    // Create both Delta and Iceberg structures
    std::fs::create_dir_all(table_path.join("_delta_log")).unwrap();
    std::fs::create_dir_all(table_path.join("metadata")).unwrap();

    // Delta should take precedence
    let format = detect_table_format(table_path);
    assert_eq!(format, TableFormat::Delta);
}

// ============================================================================
// Path Utilities Tests
// ============================================================================

#[test]
fn test_normalize_path_trailing_slash() {
    assert_eq!(normalize_path("/path/to/table/"), "/path/to/table");
    assert_eq!(normalize_path("/path/to/table"), "/path/to/table");
}

#[test]
fn test_normalize_path_s3() {
    assert_eq!(
        normalize_path("s3://bucket/path/"),
        "s3://bucket/path"
    );
}

#[test]
fn test_normalize_relative_path() {
    assert_eq!(
        normalize_relative_path("/base/data/file.parquet", "/base"),
        Some("data/file.parquet".to_string())
    );
    assert_eq!(
        normalize_relative_path("/base/data/file.parquet", "/base/data"),
        Some("file.parquet".to_string())
    );
    // Non-matching base returns None
    assert_eq!(
        normalize_relative_path("/other/file.parquet", "/base"),
        None
    );
}

#[test]
fn test_extract_version_from_metadata_path() {
    // Test valid metadata paths in Iceberg standard format: <version>-<uuid>.metadata.json
    let result = extract_version_from_path("metadata/00005-abc123.metadata.json");
    assert_eq!(result, Some(5), "Should extract version 5");

    let result = extract_version_from_path("s3://bucket/table/metadata/00123-uuid.metadata.json");
    assert_eq!(result, Some(123), "Should extract version from S3 path");

    // Test invalid paths
    let result = extract_version_from_path("not-a-version.json");
    assert!(result.is_none(), "Should return None for invalid path");

    // Old format (v1.metadata.json) is not supported
    let result = extract_version_from_path("metadata/v5.metadata.json");
    assert!(result.is_none(), "Old format not supported");
}

// ============================================================================
// Byte Parsing and Formatting Tests
// ============================================================================

#[test]
fn test_parse_bytes_units() {
    assert_eq!(parse_bytes("1024").unwrap(), 1024);
    assert_eq!(parse_bytes("1KB").unwrap(), 1024);
    assert_eq!(parse_bytes("1kb").unwrap(), 1024);
    assert_eq!(parse_bytes("1MB").unwrap(), 1024 * 1024);
    assert_eq!(parse_bytes("1GB").unwrap(), 1024 * 1024 * 1024);
    assert_eq!(parse_bytes("2GB").unwrap(), 2 * 1024 * 1024 * 1024);
}

#[test]
fn test_parse_bytes_with_spaces() {
    assert_eq!(parse_bytes("  512MB  ").unwrap(), 512 * 1024 * 1024);
}

#[test]
fn test_parse_bytes_invalid() {
    assert!(parse_bytes("invalid").is_err());
    assert!(parse_bytes("1XB").is_err());
    assert!(parse_bytes("").is_err());
}

#[test]
fn test_format_bytes() {
    assert_eq!(format_bytes(0), "0 bytes");
    assert!(format_bytes(512).contains("512"));
    assert!(format_bytes(1024).contains("KB") || format_bytes(1024).contains("1"));
    assert!(format_bytes(1024 * 1024).contains("MB") || format_bytes(1024 * 1024).contains("1"));
    assert!(format_bytes(1024 * 1024 * 1024).contains("GB") || format_bytes(1024 * 1024 * 1024).contains("1"));
}

// ============================================================================
// Storage Type Detection Tests
// ============================================================================

#[test]
fn test_storage_type_detection() {
    use icetable::core::storage::detect_storage_type;

    assert_eq!(detect_storage_type("s3://bucket/path"), "s3");
    assert_eq!(detect_storage_type("s3a://bucket/path"), "s3");
    assert_eq!(detect_storage_type("gs://bucket/path"), "gcs");
    assert_eq!(detect_storage_type("az://container/path"), "azure");
    assert_eq!(detect_storage_type("abfs://container/path"), "azure");
    assert_eq!(detect_storage_type("/local/path"), "local");
    assert_eq!(detect_storage_type("./relative/path"), "local");
}

// ============================================================================
// Resource Limits Tests
// ============================================================================

#[test]
fn test_resource_limits_parse_memory() {
    use icetable::utils::ResourceLimits;

    assert_eq!(ResourceLimits::parse_memory("2GB").unwrap(), 2 * 1024 * 1024 * 1024);
    assert_eq!(ResourceLimits::parse_memory("512MB").unwrap(), 512 * 1024 * 1024);
    assert_eq!(ResourceLimits::parse_memory("0").unwrap(), 0);
}

#[test]
fn test_resource_limits_format_memory() {
    use icetable::utils::ResourceLimits;

    assert_eq!(ResourceLimits::format_memory(0), "unlimited");
    assert_eq!(ResourceLimits::format_memory(1024 * 1024 * 1024), "1.0GB");
    assert_eq!(ResourceLimits::format_memory(512 * 1024 * 1024), "512.0MB");
}

// ============================================================================
// Time Parsing Tests
// ============================================================================

#[test]
fn test_parse_relative_duration() {
    use icetable::utils::parse_relative_duration;

    // Test various duration formats
    let duration = parse_relative_duration("7d").unwrap();
    assert_eq!(duration.num_days(), 7);

    let duration = parse_relative_duration("24h").unwrap();
    assert_eq!(duration.num_hours(), 24);

    let duration = parse_relative_duration("30m").unwrap();
    assert_eq!(duration.num_minutes(), 30);
}

#[test]
fn test_parse_timestamp() {
    use icetable::utils::parse_timestamp;
    use chrono::Utc;

    // Relative duration format (7d = 7 days ago)
    let ts = parse_timestamp("7d").unwrap();
    let now = Utc::now();
    // 7 days ago should be before now
    assert!(ts < now);

    // Hours
    let ts = parse_timestamp("24h").unwrap();
    assert!(ts < now);
}

// ============================================================================
// Data Type Parsing Tests
// ============================================================================

#[test]
fn test_parse_data_type() {
    use icetable::utils::parse_data_type;
    use arrow::datatypes::DataType;

    // Standard Arrow type names
    assert_eq!(parse_data_type("Int32").unwrap(), DataType::Int32);
    assert_eq!(parse_data_type("Int64").unwrap(), DataType::Int64);
    assert_eq!(parse_data_type("String").unwrap(), DataType::Utf8);
    assert_eq!(parse_data_type("Boolean").unwrap(), DataType::Boolean);
    assert_eq!(parse_data_type("Float32").unwrap(), DataType::Float32);
    assert_eq!(parse_data_type("Float64").unwrap(), DataType::Float64);
    assert_eq!(parse_data_type("Date32").unwrap(), DataType::Date32);

    // Invalid types should error
    assert!(parse_data_type("invalid_type").is_err());
}
