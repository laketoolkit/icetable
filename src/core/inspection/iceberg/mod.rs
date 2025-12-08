//! Iceberg table inspection
//!
//! This module provides inspection capabilities for Apache Iceberg tables,
//! including metadata extraction, statistics, layout information, and
//! orphan file detection.
//!
//! # Module Structure
//!
//! - `manifest` - Reading and processing Iceberg manifest files
//! - `orphan` - Detection of orphan (unreferenced) data files
//! - `layout` - Layout and statistics extraction
//! - `factory` - Factory for creating inspectors

mod factory;
mod layout;
mod manifest;
mod orphan;

pub use factory::IcebergInspectorFactory;

use async_trait::async_trait;
use std::path::PathBuf;

use crate::core::inspection::iceberg_metadata_extractor::extract_file_info;
use crate::core::inspection::traits::{
    FileInfo, PhysicalInspectOptions, PhysicalInspector, PhysicalMetadata, SchemaInfo,
    VerbosityLevel,
};
use crate::core::metadata::IcebergMetadataService;
use crate::core::storage::{ObjectStoreExt, Storage};
use crate::core::utils::find_latest_metadata;
use crate::error::{Error, Result};

/// Parse a JSON value that may be stored as string, integer, or float
///
/// Iceberg writers may serialize numeric values in different formats:
/// - As strings: "1234567890" (most common)
/// - As integers: 1234567890 (serde_json i64/u64)
/// - As floats: 1234567890.0 (some writers, e.g., certain Spark/PyIceberg configs)
///
/// This function handles all three cases to ensure robust parsing.
pub(crate) fn parse_summary_value<T>(value: Option<&serde_json::Value>) -> Option<T>
where
    T: std::str::FromStr + TryFrom<i64> + TryFrom<u64>,
{
    value.and_then(|v| {
        // Try string first (most common in Iceberg)
        v.as_str()
            .and_then(|s| s.parse::<T>().ok())
            // Try integer types
            .or_else(|| v.as_i64().and_then(|n| T::try_from(n).ok()))
            .or_else(|| v.as_u64().and_then(|n| T::try_from(n).ok()))
            // Try float (some writers serialize numbers as floats)
            .or_else(|| {
                v.as_f64().and_then(|f| {
                    // Only convert if it's a whole number (no fractional part)
                    if f.fract() == 0.0 && f >= 0.0 && f <= u64::MAX as f64 {
                        // Try to convert through u64 first for unsigned types
                        T::try_from(f as u64).ok()
                    } else if f.fract() == 0.0 && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
                        // Fall back to i64 for signed types
                        T::try_from(f as i64).ok()
                    } else {
                        None
                    }
                })
            })
    })
}

/// Iceberg table inspector
pub struct IcebergInspector {
    path: PathBuf,
    storage: Storage,
}

impl IcebergInspector {
    /// Create a new Iceberg inspector
    pub fn new(path: PathBuf, storage: Storage) -> Self {
        Self { path, storage }
    }

    /// Get the table path as a string
    fn table_path(&self) -> &str {
        self.path.to_str().unwrap_or("")
    }

    async fn read_metadata(&self, metadata_path: &str) -> Result<serde_json::Value> {
        let metadata_bytes = self.storage.get_bytes_str(metadata_path).await?;
        let metadata_str = String::from_utf8(metadata_bytes.to_vec())
            .map_err(|e| Error::General(format!("Invalid UTF-8 in metadata file: {}", e)))?;

        serde_json::from_str(&metadata_str)
            .map_err(|e| Error::General(format!("Failed to parse metadata JSON: {}", e)))
    }

    fn extract_file_info(
        &self,
        metadata: &serde_json::Value,
        metadata_path: &str,
        options: &PhysicalInspectOptions,
        iceberg_metadata: Option<&iceberg::spec::TableMetadata>,
    ) -> FileInfo {
        let path_str = self.path.to_str().unwrap_or("");
        extract_file_info(
            path_str,
            metadata,
            metadata_path,
            options,
            iceberg_metadata,
        )
    }

    fn extract_schema(&self, metadata: &serde_json::Value) -> Result<SchemaInfo> {
        crate::core::inspection::iceberg_metadata_extractor::extract_schema(metadata)
    }

    fn extract_sort_order(&self, metadata: &serde_json::Value) -> String {
        crate::core::inspection::iceberg_metadata_extractor::extract_sort_order(metadata)
    }
}

#[async_trait]
impl PhysicalInspector for IcebergInspector {
    async fn extract_metadata(&self, options: &PhysicalInspectOptions) -> Result<PhysicalMetadata> {
        let metadata_path = find_latest_metadata(self.table_path(), &self.storage).await?;
        let metadata = self.read_metadata(&metadata_path).await?;

        // Load iceberg-rs TableMetadata for accurate snapshot count (consistent with vacuum)
        let iceberg_meta =
            match IcebergMetadataService::new_async(self.path.to_str().unwrap_or("").to_string())
                .await
            {
                Ok(service) => service.load_metadata().await.ok().map(|(m, _)| m),
                Err(_) => None,
            };

        let file_info = self.extract_file_info(
            &metadata,
            &metadata_path,
            options,
            iceberg_meta.as_ref().map(|m| m.as_ref()),
        );

        let schema = if options.show_schema {
            Some(self.extract_schema(&metadata)?)
        } else {
            None
        };

        let layout = if options.show_layout {
            Some(layout::extract_layout_info(&metadata, options, |m| {
                self.extract_sort_order(m)
            })?)
        } else {
            None
        };

        let statistics = if options.show_stats {
            Some(
                layout::extract_statistics(&self.storage, self.table_path(), &metadata, options)
                    .await?,
            )
        } else {
            None
        };

        // Detect orphan files only in verbose mode
        let orphan_files = if options.verbosity >= VerbosityLevel::Verbose {
            match orphan::detect_orphan_files(
                self.table_path(),
                &self.storage,
                &metadata,
                options.deep_scan,
            )
            .await
            {
                Ok(info) if info.count > 0 => Some(info),
                _ => None,
            }
        } else {
            None
        };

        Ok(PhysicalMetadata {
            format_name: "Apache Iceberg".to_string(),
            file_info,
            schema,
            layout,
            statistics,
            orphan_files,
        })
    }

    fn format_name(&self) -> &str {
        "Apache Iceberg"
    }

    fn can_inspect(&self, path: &str) -> bool {
        // For local paths, check if metadata directory exists
        // For URLs, this is a quick heuristic - actual detection is done in factory
        let local_path = std::path::Path::new(path);
        let metadata_path = local_path.join("metadata");

        metadata_path.exists()
            || path.ends_with("/metadata")
            || path.contains("/metadata/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use object_store::local::LocalFileSystem;
    use std::sync::Arc;

    #[test]
    fn test_iceberg_inspector_format_name() {
        let storage: Arc<dyn object_store::ObjectStore> = Arc::new(LocalFileSystem::new());
        let inspector = IcebergInspector::new(PathBuf::from("/test"), storage);
        assert_eq!(inspector.format_name(), "Apache Iceberg");
    }

    #[test]
    fn test_iceberg_inspector_can_inspect() {
        let storage: Arc<dyn object_store::ObjectStore> = Arc::new(LocalFileSystem::new());
        let inspector = IcebergInspector::new(PathBuf::from("/test"), storage);

        assert!(inspector.can_inspect(Path::new("/path/to/table/metadata")));
        assert!(!inspector.can_inspect(Path::new("/path/to/file.parquet")));
    }
}

#[cfg(test)]
mod parse_summary_value_tests {
    use super::*;
    use serde_json::json;

    // === String value tests (most common in Iceberg) ===
    #[test]
    fn test_parse_from_string() {
        let val = json!("1234567890");
        let result: Option<u64> = parse_summary_value(Some(&val));
        assert_eq!(result, Some(1234567890));
    }

    #[test]
    fn test_parse_large_string() {
        let val = json!("1234567890123"); // ~1.2TB
        let result: Option<u64> = parse_summary_value(Some(&val));
        assert_eq!(result, Some(1234567890123));
    }

    // === Integer value tests ===
    #[test]
    fn test_parse_from_i64() {
        let val = json!(1234567890123_i64);
        let result: Option<u64> = parse_summary_value(Some(&val));
        assert_eq!(result, Some(1234567890123));
    }

    #[test]
    fn test_parse_from_large_u64() {
        // Number greater than i64::MAX
        let val = json!(9223372036854775808_u64);
        let result: Option<u64> = parse_summary_value(Some(&val));
        assert_eq!(result, Some(9223372036854775808));
    }

    // === Float value tests (the bug case!) ===
    #[test]
    fn test_parse_from_f64() {
        // Some Iceberg writers serialize numbers as floats
        let val = json!(1234567890123.0_f64);
        let result: Option<u64> = parse_summary_value(Some(&val));
        // This was the bug: before the fix, this returned None
        assert_eq!(result, Some(1234567890123));
    }

    #[test]
    fn test_parse_from_f64_rejects_fractional() {
        // Floats with fractional parts should NOT be converted
        let val = json!(1234567890123.5_f64);
        let result: Option<u64> = parse_summary_value(Some(&val));
        assert_eq!(result, None);
    }

    // === Edge cases ===
    #[test]
    fn test_parse_null_returns_none() {
        let val = json!(null);
        let result: Option<u64> = parse_summary_value(Some(&val));
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_missing_returns_none() {
        let result: Option<u64> = parse_summary_value(None);
        assert_eq!(result, None);
    }

    // === Integration test: simulate real Iceberg snapshot ===
    #[test]
    fn test_real_world_iceberg_snapshot() {
        // Simulate a real Iceberg metadata file structure
        let metadata = json!({
            "current-snapshot-id": 12345,
            "snapshots": [{
                "snapshot-id": 12345,
                "summary": {
                    "total-data-files": "5350",
                    "total-records": "29950000",
                    "total-files-size": "1234567890123"
                }
            }]
        });

        let current_id = metadata
            .get("current-snapshot-id")
            .and_then(|id| id.as_i64())
            .unwrap();
        let snapshots = metadata
            .get("snapshots")
            .and_then(|s| s.as_array())
            .unwrap();
        let snapshot = snapshots
            .iter()
            .find(|s| s.get("snapshot-id").and_then(|id| id.as_i64()) == Some(current_id))
            .unwrap();
        let summary = snapshot.get("summary").and_then(|s| s.as_object()).unwrap();

        let num_files: usize = parse_summary_value(summary.get("total-data-files")).unwrap_or(0);
        let total_size: u64 = parse_summary_value(summary.get("total-files-size")).unwrap_or(0);
        let total_rows: i64 = parse_summary_value(summary.get("total-records")).unwrap_or(0);

        assert_eq!(num_files, 5350);
        assert_eq!(total_size, 1234567890123);
        assert_eq!(total_rows, 29950000);
    }

    #[test]
    fn test_real_world_iceberg_snapshot_with_floats() {
        // Some writers (e.g., certain Spark/PyIceberg configs) serialize as floats
        let metadata = json!({
            "current-snapshot-id": 12345,
            "snapshots": [{
                "snapshot-id": 12345,
                "summary": {
                    "total-data-files": 5350.0,
                    "total-records": 29950000.0,
                    "total-files-size": 1234567890123.0
                }
            }]
        });

        let current_id = metadata
            .get("current-snapshot-id")
            .and_then(|id| id.as_i64())
            .unwrap();
        let snapshots = metadata
            .get("snapshots")
            .and_then(|s| s.as_array())
            .unwrap();
        let snapshot = snapshots
            .iter()
            .find(|s| s.get("snapshot-id").and_then(|id| id.as_i64()) == Some(current_id))
            .unwrap();
        let summary = snapshot.get("summary").and_then(|s| s.as_object()).unwrap();

        let num_files: usize = parse_summary_value(summary.get("total-data-files")).unwrap_or(0);
        let total_size: u64 = parse_summary_value(summary.get("total-files-size")).unwrap_or(0);
        let total_rows: i64 = parse_summary_value(summary.get("total-records")).unwrap_or(0);

        // Before the fix, these would all be 0!
        assert_eq!(num_files, 5350);
        assert_eq!(total_size, 1234567890123);
        assert_eq!(total_rows, 29950000);
    }
}
