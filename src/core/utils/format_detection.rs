//! Table format detection utilities
//!
//! Centralized logic for detecting table formats.

use std::path::Path;
use std::sync::Arc;

use crate::core::storage::traits::ListOptions;
use crate::core::storage::{StorageBackend, StorageBackendFactory};

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
    match StorageBackendFactory::create_backend(path).await {
        Ok(storage) => detect_table_format_with_storage(path, &storage).await,
        Err(_) => TableFormat::Unknown,
    }
}

/// Detect the table format using an existing storage backend
///
/// Checks for the presence of format-specific directories:
/// - `_delta_log` for Delta Lake
/// - `metadata` for Iceberg
pub async fn detect_table_format_with_storage(
    path: &str,
    storage: &Arc<dyn StorageBackend>,
) -> TableFormat {
    let base_path = path.trim_end_matches('/');

    // Check for Delta Lake (_delta_log directory)
    let delta_prefix = format!("{}/_delta_log/", base_path);
    let list_opts = ListOptions {
        prefix: Some(delta_prefix),
        delimiter: None,
        max_results: Some(1),
        continuation_token: None,
    };
    if let Ok(result) = storage.list(&list_opts).await
        && !result.objects.is_empty()
    {
        return TableFormat::Delta;
    }

    // Check for Iceberg (metadata directory)
    let iceberg_prefix = format!("{}/metadata/", base_path);
    let list_opts = ListOptions {
        prefix: Some(iceberg_prefix),
        delimiter: None,
        max_results: Some(1),
        continuation_token: None,
    };
    if let Ok(result) = storage.list(&list_opts).await
        && !result.objects.is_empty()
    {
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
