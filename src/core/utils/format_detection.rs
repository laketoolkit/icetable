//! Table format detection utilities
//!
//! Centralized logic for detecting table formats.

use std::path::Path;

use futures::TryStreamExt;
use object_store::ObjectStore;

use crate::core::storage::{Storage, create_object_store};

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
        Ok(storage) => detect_format(path, &storage).await,
        Err(_) => TableFormat::Unknown,
    }
}

/// Detect the table format for a table path
///
/// Checks for the presence of format-specific directories:
/// - `_delta_log` for Delta Lake (only for import purposes)
/// - `metadata` for Iceberg
pub async fn detect_format(table_path: &str, storage: &Storage) -> TableFormat {
    use crate::core::storage::to_path;

    let table_path = table_path.trim_end_matches('/');

    // Check for Delta Lake (_delta_log directory)
    let delta_prefix = to_path(&format!("{}/_delta_log/", table_path));
    if let Ok(Some(_)) = storage.list(Some(&delta_prefix)).try_next().await {
        return TableFormat::Delta;
    }

    // Check for Iceberg (metadata directory)
    let iceberg_prefix = to_path(&format!("{}/metadata/", table_path));
    if let Ok(Some(_)) = storage.list(Some(&iceberg_prefix)).try_next().await {
        return TableFormat::Iceberg;
    }

    TableFormat::Unknown
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
