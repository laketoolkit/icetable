//! Iceberg conflict detection for optimistic concurrency control
//!
//! Detects concurrent writes by verifying that expected metadata versions
//! haven't changed before committing new snapshots.

use std::sync::Arc;

use crate::core::storage::StorageBackend;
use crate::core::utils::find_latest_metadata;
use crate::error::{Error, Result};

/// Result of conflict detection
#[derive(Debug)]
pub struct ConflictCheckResult {
    /// Whether a conflict was detected
    pub has_conflict: bool,
    /// The expected version when the operation started
    pub expected_version: i32,
    /// The current version found on storage
    pub current_version: i32,
    /// Additional details about the conflict
    pub details: Option<String>,
}

impl ConflictCheckResult {
    /// Create a result indicating no conflict
    pub fn no_conflict(version: i32) -> Self {
        Self {
            has_conflict: false,
            expected_version: version,
            current_version: version,
            details: None,
        }
    }

    /// Create a result indicating a conflict
    pub fn conflict(expected: i32, current: i32, details: String) -> Self {
        Self {
            has_conflict: true,
            expected_version: expected,
            current_version: current,
            details: Some(details),
        }
    }
}

/// Conflict detector for Iceberg table operations
pub struct ConflictDetector {
    table_path: String,
    storage: Arc<dyn StorageBackend>,
}

impl ConflictDetector {
    /// Create a new conflict detector
    pub fn new(table_path: String, storage: Arc<dyn StorageBackend>) -> Self {
        Self {
            table_path,
            storage,
        }
    }

    /// Check for conflicts before committing
    ///
    /// Verifies that the table metadata version hasn't changed since the operation started.
    /// This implements optimistic concurrency control.
    ///
    /// # Arguments
    /// * `expected_version` - The metadata version when the operation started
    ///
    /// # Returns
    /// * `Ok(ConflictCheckResult)` with conflict status
    pub async fn check_for_conflicts(&self, expected_version: i32) -> Result<ConflictCheckResult> {
        // Get current metadata path
        let current_metadata = find_latest_metadata(&self.table_path, &self.storage).await?;
        let current_version = extract_version(&current_metadata);

        if current_version != expected_version {
            return Ok(ConflictCheckResult::conflict(
                expected_version,
                current_version,
                format!(
                    "Table was modified by another process. Expected version v{}, but current is v{}.",
                    expected_version, current_version
                ),
            ));
        }

        // Also check if the next version file already exists (shouldn't happen but good to verify)
        let next_version = expected_version + 1;
        let next_metadata_path = format!(
            "{}/metadata/v{}.metadata.json",
            self.table_path.trim_end_matches('/'),
            next_version
        );

        if self.file_exists(&next_metadata_path).await? {
            return Ok(ConflictCheckResult::conflict(
                expected_version,
                next_version,
                format!(
                    "Version v{}.metadata.json already exists. Another process may have committed.",
                    next_version
                ),
            ));
        }

        Ok(ConflictCheckResult::no_conflict(expected_version))
    }

    /// Check if a file exists on storage
    async fn file_exists(&self, path: &str) -> Result<bool> {
        use crate::core::storage::GetOptions;

        match self.storage.get(path, &GetOptions::default()).await {
            Ok(_) => Ok(true),
            Err(e) => {
                // Check if it's a "not found" error
                let error_str = e.to_string().to_lowercase();
                if error_str.contains("not found")
                    || error_str.contains("does not exist")
                    || error_str.contains("no such file")
                    || error_str.contains("404")
                {
                    Ok(false)
                } else {
                    Err(e)
                }
            }
        }
    }
}

/// Extract version number from metadata filename
fn extract_version(path: &str) -> i32 {
    // Path like: s3://bucket/table/metadata/v5.metadata.json
    path.split('/')
        .last()
        .and_then(|filename| {
            filename
                .strip_prefix('v')
                .and_then(|rest| rest.strip_suffix(".metadata.json"))
                .and_then(|num| num.parse().ok())
        })
        .unwrap_or(0)
}

/// Helper function to check for conflicts and return error if found
pub async fn check_and_fail_on_conflict(
    table_path: &str,
    storage: &Arc<dyn StorageBackend>,
    expected_version: i32,
) -> Result<()> {
    let detector = ConflictDetector::new(table_path.to_string(), Arc::clone(storage));
    let result = detector.check_for_conflicts(expected_version).await?;

    if result.has_conflict {
        return Err(Error::Conflict(
            result.details.unwrap_or_else(|| {
                format!(
                    "Concurrent modification detected: expected v{}, found v{}",
                    result.expected_version, result.current_version
                )
            }),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_version() {
        assert_eq!(
            extract_version("s3://bucket/table/metadata/v5.metadata.json"),
            5
        );
        assert_eq!(
            extract_version("/path/to/table/metadata/v123.metadata.json"),
            123
        );
        assert_eq!(extract_version("v1.metadata.json"), 1);
        assert_eq!(extract_version("invalid"), 0);
    }

    #[test]
    fn test_conflict_result() {
        let no_conflict = ConflictCheckResult::no_conflict(5);
        assert!(!no_conflict.has_conflict);
        assert_eq!(no_conflict.expected_version, 5);
        assert_eq!(no_conflict.current_version, 5);

        let conflict = ConflictCheckResult::conflict(5, 6, "Test conflict".to_string());
        assert!(conflict.has_conflict);
        assert_eq!(conflict.expected_version, 5);
        assert_eq!(conflict.current_version, 6);
    }
}
