//! Iceberg-specific utility functions
//!
//! Common utilities for working with Iceberg tables.

use std::sync::Arc;

use crate::core::storage::StorageBackend;
use crate::core::storage::traits::{GetOptions, ListOptions};
use crate::error::{Error, Result};

use iceberg::spec::{PrimitiveType, Type};

/// Find the latest metadata file for an Iceberg table
///
/// Uses version-hint.text first (authoritative), falls back to listing
/// if version-hint doesn't exist or is invalid.
pub async fn find_latest_metadata(
    table_path: &str,
    storage: &Arc<dyn StorageBackend>,
) -> Result<String> {
    let metadata_dir = format!("{}/metadata", table_path.trim_end_matches('/'));

    // First try to read version-hint.text for authoritative version
    // This avoids S3 eventual consistency issues with list operations
    let version_hint_path = format!("{}/version-hint.text", metadata_dir);
    let get_opts = GetOptions::default();

    if let Ok(version_bytes) = storage.get(&version_hint_path, &get_opts).await
        && let Ok(version_str) = String::from_utf8(version_bytes.to_vec())
        && let Ok(version) = version_str.trim().parse::<i32>()
    {
        let metadata_path = format!("{}/v{}.metadata.json", metadata_dir, version);
        // Verify file exists by trying to read it
        if storage.get(&metadata_path, &get_opts).await.is_ok() {
            return Ok(metadata_path);
        }
    }

    // Fallback to listing if version-hint doesn't exist or is invalid
    let list_opts = ListOptions {
        prefix: Some(format!("{}/", metadata_dir)),
        delimiter: None,
        max_results: Some(500),
        continuation_token: None,
    };

    let files = storage.list(&list_opts).await?;

    // Find the latest metadata.json file by version number
    // Supports both formats: v1.metadata.json and 00001-uuid.metadata.json
    let metadata_file = files
        .objects
        .iter()
        .filter(|obj| obj.path.contains(".metadata.json"))
        .max_by_key(|obj| {
            let name = obj.path.rsplit('/').next().unwrap_or("");
            if name.starts_with('v') {
                name.trim_start_matches('v')
                    .split('.')
                    .next()
                    .and_then(|n| n.parse::<i64>().ok())
                    .unwrap_or(0)
            } else {
                name.split('-')
                    .next()
                    .and_then(|n| n.parse::<i64>().ok())
                    .unwrap_or(0)
            }
        })
        .ok_or_else(|| {
            Error::General("No metadata.json file found in metadata/ directory".to_string())
        })?;

    Ok(metadata_file.path.clone())
}

/// Extract version number from a metadata filename
///
/// Supports both formats:
/// - v1.metadata.json -> 1
/// - 00001-uuid.metadata.json -> 1
pub fn extract_version_from_filename(filename: &str) -> i32 {
    let name = filename.rsplit('/').next().unwrap_or(filename);
    if name.starts_with('v') {
        name.trim_start_matches('v')
            .split('.')
            .next()
            .and_then(|n| n.parse::<i32>().ok())
            .unwrap_or(1)
    } else {
        name.split('-')
            .next()
            .and_then(|n| n.parse::<i32>().ok())
            .unwrap_or(1)
    }
}

/// Convert Iceberg type to Arrow type (simplified)
///
/// This provides a basic mapping from Iceberg primitive types to Arrow data types.
/// Complex types (structs, lists, maps) are currently mapped to UTF8 strings as a fallback.
pub fn iceberg_to_arrow_type(iceberg_type: &Type) -> arrow::datatypes::DataType {
    use arrow::datatypes::DataType;

    match iceberg_type {
        Type::Primitive(p) => match p {
            PrimitiveType::Boolean => DataType::Boolean,
            PrimitiveType::Int => DataType::Int32,
            PrimitiveType::Long => DataType::Int64,
            PrimitiveType::Float => DataType::Float32,
            PrimitiveType::Double => DataType::Float64,
            PrimitiveType::String => DataType::Utf8,
            PrimitiveType::Binary => DataType::Binary,
            PrimitiveType::Date => DataType::Date32,
            PrimitiveType::Timestamp => {
                DataType::Timestamp(arrow::datatypes::TimeUnit::Microsecond, None)
            }
            PrimitiveType::Timestamptz => {
                DataType::Timestamp(arrow::datatypes::TimeUnit::Microsecond, Some("UTC".into()))
            }
            _ => DataType::Utf8, // Fallback for other types
        },
        _ => arrow::datatypes::DataType::Utf8, // Fallback for complex types
    }
}
