//! Iceberg-specific utility functions
//!
//! Common utilities for working with Iceberg tables.

use std::str::FromStr;
use std::sync::Arc;

use iceberg::MetadataLocation;
use iceberg::spec::{PrimitiveType, Type};

use crate::core::storage::StorageBackend;
use crate::core::storage::traits::ListOptions;
use crate::error::{Error, Result};

/// Find the latest metadata file for an Iceberg table
///
/// Lists the metadata directory and finds the file with highest version number.
/// Supports the standard Iceberg format: `<version>-<uuid>.metadata.json`
pub async fn find_latest_metadata(
    table_path: &str,
    storage: &Arc<dyn StorageBackend>,
) -> Result<String> {
    let metadata_dir = format!("{}/metadata", table_path.trim_end_matches('/'));

    let list_opts = ListOptions {
        prefix: Some(format!("{}/", metadata_dir)),
        delimiter: None,
        max_results: Some(1000),
        continuation_token: None,
    };

    let files = storage.list(&list_opts).await?;

    // Find the latest metadata.json file by version number
    // Supports both formats:
    // - Standard: <version>-<uuid>.metadata.json (e.g., 00015-abc123.metadata.json)
    // - Legacy Hadoop: v<version>.metadata.json (e.g., v15.metadata.json)
    let metadata_file = files
        .objects
        .iter()
        .filter(|obj| obj.path.ends_with(".metadata.json"))
        .filter_map(|obj| {
            // extract_version_from_path returns None if it can't parse, Some(version) otherwise
            extract_version_from_path(&obj.path).map(|version| (obj, version))
        })
        .max_by_key(|(_, version)| *version)
        .map(|(obj, _)| obj)
        .ok_or_else(|| {
            Error::General(format!(
                "No valid metadata.json file found in {}/",
                metadata_dir
            ))
        })?;

    Ok(metadata_file.path.clone())
}

/// Extract version number from a metadata file path
///
/// Only supports standard Iceberg format: `00015-uuid.metadata.json` -> 15
///
/// Returns None if the path cannot be parsed.
pub fn extract_version_from_path(path: &str) -> Option<i32> {
    let filename = path.split('/').next_back().unwrap_or(path);

    // Standard format: <version>-<uuid>.metadata.json
    // The version is zero-padded, e.g., 00015-abc123.metadata.json
    if filename.ends_with(".metadata.json") {
        if let Some(version_part) = filename.split('-').next() {
            if let Ok(version) = version_part.parse::<i32>() {
                return Some(version);
            }
        }
    }

    None
}

/// Get just the filename from a MetadataLocation
///
/// Returns the filename portion of the metadata location path (e.g., "00001-uuid.metadata.json")
pub fn metadata_location_filename(location: &MetadataLocation) -> String {
    let full_path = location.to_string();
    full_path.split('/').next_back().unwrap_or(&full_path).to_string()
}

/// Create a new metadata location for the next version
///
/// Given the current metadata path, creates a new MetadataLocation
/// with incremented version and new UUID.
pub fn next_metadata_location(current_path: &str) -> Result<MetadataLocation> {
    let current = MetadataLocation::from_str(current_path).map_err(|e| {
        Error::General(format!("Failed to parse metadata location '{}': {}", current_path, e))
    })?;
    Ok(current.with_next_version())
}

/// Create a new metadata location for a new table
///
/// Creates the initial MetadataLocation (version 0) for a new table.
pub fn new_metadata_location(table_location: &str) -> MetadataLocation {
    MetadataLocation::new_with_table_location(table_location)
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
