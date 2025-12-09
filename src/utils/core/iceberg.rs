//! Iceberg-specific utility functions
//!
//! Common utilities for working with Iceberg tables.

use std::str::FromStr;

use iceberg::MetadataLocation;

use crate::core::storage::{ObjectStoreExt, Storage};
use crate::error::{Error, Result};

/// Find the latest metadata file for an Iceberg table
///
/// Lists the metadata directory and finds the file with highest version number.
/// Supports the standard Iceberg format: `<version>-<uuid>.metadata.json`
///
/// Returns the full path/URL to the metadata file (e.g., `/path/to/table/metadata/00001-xxx.json`
/// or `s3://bucket/table/metadata/00001-xxx.json`).
pub async fn find_latest_metadata(table_path: &str, storage: &Storage) -> Result<String> {
    let table_path = table_path.trim_end_matches('/');
    let metadata_dir = format!("{}/metadata/", table_path);

    let files = storage.list_prefix(&metadata_dir).await?;

    // Find the latest metadata.json file by version number
    // Note: obj.location is relative to the storage root, so we extract just the filename
    // and reconstruct the full path using the original table_path
    let metadata_filename = files
        .iter()
        .filter(|obj| obj.location.to_string().ends_with(".metadata.json"))
        .filter_map(|obj| {
            let path_str = obj.location.to_string();
            let filename = path_str.split('/').next_back().unwrap_or(&path_str);
            extract_version_from_path(filename).map(|version| (filename.to_string(), version))
        })
        .max_by_key(|(_, version)| *version)
        .map(|(filename, _)| filename)
        .ok_or_else(|| {
            Error::General(format!(
                "No valid metadata.json file found in {}",
                metadata_dir
            ))
        })?;

    // Return full path: table_path/metadata/filename
    Ok(format!("{}/metadata/{}", table_path, metadata_filename))
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
    if filename.ends_with(".metadata.json")
        && let Some(version_part) = filename.split('-').next()
        && let Ok(version) = version_part.parse::<i32>()
    {
        return Some(version);
    }

    None
}

/// Get just the filename from a MetadataLocation
///
/// Returns the filename portion of the metadata location path (e.g., "00001-uuid.metadata.json")
pub fn metadata_location_filename(location: &MetadataLocation) -> String {
    let full_path = location.to_string();
    full_path
        .split('/')
        .next_back()
        .unwrap_or(&full_path)
        .to_string()
}

/// Create a new metadata location for the next version
///
/// Given the current metadata path, creates a new MetadataLocation
/// with incremented version and new UUID.
pub fn next_metadata_location(current_path: &str) -> Result<MetadataLocation> {
    let current = MetadataLocation::from_str(current_path).map_err(|e| {
        Error::General(format!(
            "Failed to parse metadata location '{}': {}",
            current_path, e
        ))
    })?;
    Ok(current.with_next_version())
}

/// Create a new metadata location for a new table
///
/// Creates the initial MetadataLocation (version 0) for a new table.
pub fn new_metadata_location(table_location: &str) -> MetadataLocation {
    MetadataLocation::new_with_table_location(table_location)
}

/// Result of writing metadata file
pub struct WriteMetadataResult {
    /// Path to the new metadata file
    pub path: String,
    /// Version number of the new metadata
    pub version: i64,
}

/// Write metadata file using standard Iceberg naming convention
///
/// This is the canonical function for writing metadata files.
/// It handles version numbering and path generation.
///
/// Returns both the path and version number of the new metadata file.
pub async fn write_metadata_file(
    table_path: &str,
    metadata: &iceberg::spec::TableMetadata,
    storage: &Storage,
) -> Result<WriteMetadataResult> {
    let table_path = table_path.trim_end_matches('/');
    let metadata_dir = format!("{}/metadata", table_path);

    // Find current metadata to derive next version
    let current_metadata_path = find_latest_metadata(table_path, storage).await?;

    // Generate next metadata location with standard naming
    let next_location = next_metadata_location(&current_metadata_path)
        .unwrap_or_else(|_| new_metadata_location(table_path));

    let version = extract_version_from_path(&next_location.to_string()).unwrap_or(0) as i64;
    let path = format!("{}/{}", metadata_dir, metadata_location_filename(&next_location));

    let metadata_bytes = serde_json::to_vec_pretty(metadata)
        .map_err(|e| Error::General(format!("Failed to serialize metadata: {}", e)))?;

    storage
        .put_bytes_str(&path, bytes::Bytes::from(metadata_bytes))
        .await?;

    Ok(WriteMetadataResult { path, version })
}
