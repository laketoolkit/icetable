//! Iceberg conflict detection for optimistic concurrency control
//!
//! Detects concurrent writes by verifying that expected metadata versions
//! haven't changed before committing new snapshots.

use std::sync::Arc;

use crate::core::storage::StorageBackend;
use crate::core::utils::{extract_version_from_path, find_latest_metadata};
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
        // Get current metadata path using standard format
        let current_metadata = find_latest_metadata(&self.table_path, &self.storage).await?;
        let current_version = extract_version_from_path(&current_metadata);

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

        // For standard format, we don't need to check next version file since
        // each version has a unique UUID in its filename. The version check above
        // is sufficient for conflict detection.

        Ok(ConflictCheckResult::no_conflict(expected_version))
    }

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
    fn test_extract_version_from_path_standard_format() {
        // Standard Iceberg format: <version>-<uuid>.metadata.json
        assert_eq!(
            extract_version_from_path("s3://bucket/table/metadata/00005-abc123.metadata.json"),
            5
        );
        assert_eq!(
            extract_version_from_path("/path/to/table/metadata/00123-uuid.metadata.json"),
            123
        );
        assert_eq!(extract_version_from_path("00001-test.metadata.json"), 1);
        assert_eq!(extract_version_from_path("invalid"), 0);
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
