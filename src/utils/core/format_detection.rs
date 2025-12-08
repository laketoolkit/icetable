//! Table format detection utilities
//!
//! Centralized logic for detecting table formats.

use std::path::Path;

use futures::TryStreamExt;
use object_store::ObjectStore;

use crate::core::storage::{create_object_store, Storage, StoragePath};

/// Supported table formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableFormat {
    /// Delta Lake table (has _delta_log directory)
    Delta,
    /// Apache Iceberg table (has metadata directory)
    Iceberg,
    /// Unknown or unsupported format
    Unknown,
}

impl std::fmt::Display for TableFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Delta => write!(f, "Delta Lake"),
            Self::Iceberg => write!(f, "Iceberg"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Detect the table format at the given path (local filesystem only)
///
/// Checks for the presence of format-specific directories:
/// - `_delta_log` for Delta Lake
/// - `metadata` for Iceberg
pub fn detect_table_format(path: &Path) -> TableFormat {
    if path.join("_delta_log").exists() {
        TableFormat::Delta
    } else if path.join("metadata").exists() {
        TableFormat::Iceberg
    } else {
        TableFormat::Unknown
    }
}

/// Detect the table format at the given path (supports remote storage)
///
/// Creates a storage backend and checks for format-specific directories.
pub async fn detect_table_format_async(path: &str) -> TableFormat {
    match create_object_store(path).await {
        Ok(storage) => detect_format(&storage).await,
        Err(_) => TableFormat::Unknown,
    }
}

/// Detect the table format using an existing storage (object_store)
///
/// Checks for the presence of format-specific directories:
/// - `_delta_log` for Delta Lake
/// - `metadata` for Iceberg
pub async fn detect_format(storage: &Storage) -> TableFormat {
    // Check for Delta Lake (_delta_log directory)
    // The storage is already prefixed to the table path, so we just check relative paths
    let delta_prefix = StoragePath::from("_delta_log");
    if let Ok(item) = storage.list(Some(&delta_prefix)).try_next().await {
        if item.is_some() {
            return TableFormat::Delta;
        }
    }

    // Check for Iceberg (metadata directory)
    let iceberg_prefix = StoragePath::from("metadata");
    if let Ok(item) = storage.list(Some(&iceberg_prefix)).try_next().await {
        if item.is_some() {
            return TableFormat::Iceberg;
        }
    }

    TableFormat::Unknown
}

/// Legacy function for backward compatibility during migration
pub async fn detect_table_format_with_storage(
    _path: &str,
    storage: &Storage,
) -> TableFormat {
    detect_format(storage).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_detect_delta() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("_delta_log")).unwrap();

        assert_eq!(detect_table_format(dir.path()), TableFormat::Delta);
    }

    #[test]
    fn test_detect_iceberg() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("metadata")).unwrap();

        assert_eq!(detect_table_format(dir.path()), TableFormat::Iceberg);
    }

    #[test]
    fn test_detect_unknown() {
        let dir = tempdir().unwrap();

        assert_eq!(detect_table_format(dir.path()), TableFormat::Unknown);
    }

    #[test]
    fn test_delta_takes_precedence() {
        // If both exist, Delta takes precedence (checked first)
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("_delta_log")).unwrap();
        fs::create_dir(dir.path().join("metadata")).unwrap();

        assert_eq!(detect_table_format(dir.path()), TableFormat::Delta);
    }
}
