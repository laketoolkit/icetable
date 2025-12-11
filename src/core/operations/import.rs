//! Import operations for Iceberg tables
//!
//! Business logic for importing data from external sources (Parquet files, Delta Lake)
//! into Iceberg tables.

use std::collections::HashMap;

use crate::core::metadata::{
    DataFileChanges, DataFileInfo, IcebergMetadataService, MetadataService, OperationType,
};
use crate::core::storage::{ObjectMeta, ObjectStoreExt, Storage};
use crate::error::{Error, Result};

/// Result of an import operation
#[derive(Debug, Clone)]
pub struct ImportResult {
    /// Number of files imported
    pub files_imported: usize,
    /// Total bytes imported
    pub bytes_imported: u64,
    /// Total records imported
    pub records_imported: u64,
    /// Snapshot ID created
    pub snapshot_id: i64,
}

/// Configuration for import operations
#[derive(Debug, Clone, Default)]
pub struct ImportConfig {
    /// Source identifier for summary
    pub source: String,
}

/// Service for importing data into Iceberg tables
pub struct ImportService {
    config: ImportConfig,
}

impl ImportService {
    /// Create a new import service with default config
    pub fn new() -> Self {
        Self {
            config: ImportConfig::default(),
        }
    }

    /// Create import service with custom config
    pub fn with_config(config: ImportConfig) -> Self {
        Self { config }
    }

    /// Import Parquet files into an Iceberg table
    ///
    /// Reads parquet file metadata to get row counts and adds them to the table.
    pub async fn import_parquet(
        &self,
        target_path: &str,
        files: &[&ObjectMeta],
        storage: &Storage,
    ) -> Result<ImportResult> {
        use parquet::file::reader::FileReader;

        // Load Iceberg table
        let metadata_service = IcebergMetadataService::new_async(target_path.to_string())
            .await
            .map_err(|_| {
                Error::General(format!(
                    "Target Iceberg table does not exist at '{}'. Use 'icetable init iceberg {}' first.",
                    target_path, target_path
                ))
            })?;

        // Build data file changes
        let mut changes = DataFileChanges::new();
        let mut total_records = 0u64;
        let mut total_bytes = 0u64;

        for file in files {
            // Read parquet file to get row count
            let file_path = file.location.to_string();
            let data = storage.get_bytes_str(&file_path).await?;
            let reader = parquet::file::reader::SerializedFileReader::new(data)
                .map_err(|e| Error::General(format!("Failed to read parquet: {}", e)))?;
            let metadata = reader.metadata();
            let row_count: i64 = metadata.row_groups().iter().map(|rg| rg.num_rows()).sum();

            total_records += row_count as u64;
            total_bytes += file.size;

            changes.added.push(DataFileInfo {
                path: file_path,
                size: file.size,
                record_count: row_count as u64,
                partition: HashMap::new(),
            });
        }

        // Write snapshot
        let mut summary = HashMap::new();
        let source = if self.config.source.is_empty() {
            "parquet-import"
        } else {
            &self.config.source
        };
        summary.insert("source".to_string(), source.to_string());

        let write_result = metadata_service
            .write_snapshot(changes, OperationType::Append, summary)
            .await?;

        Ok(ImportResult {
            files_imported: files.len(),
            bytes_imported: total_bytes,
            records_imported: total_records,
            snapshot_id: write_result.id,
        })
    }

    /// Import Delta Lake files into an Iceberg table
    ///
    /// Takes pre-extracted file information from Delta Lake and adds them to Iceberg.
    pub async fn import_delta(
        &self,
        target_path: &str,
        source_path: &str,
        files: &[deltalake::kernel::Add],
    ) -> Result<ImportResult> {
        // Load Iceberg table
        let metadata_service = IcebergMetadataService::new_async(target_path.to_string())
            .await
            .map_err(|_| {
                Error::General(format!(
                    "Target Iceberg table does not exist at '{}'. Use 'icetable init iceberg {}' first.",
                    target_path, target_path
                ))
            })?;

        // Build data file changes
        let mut changes = DataFileChanges::new();
        let mut total_records = 0u64;
        let mut total_bytes = 0u64;

        for file in files {
            // Construct the source file path
            let file_path = if file.path.starts_with("s3://") || file.path.starts_with('/') {
                file.path.clone()
            } else {
                format!("{}/{}", source_path.trim_end_matches('/'), file.path)
            };

            let record_count = file
                .stats
                .as_ref()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                .and_then(|v| v.get("numRecords").and_then(|n| n.as_u64()))
                .unwrap_or(0);

            total_records += record_count;
            total_bytes += file.size as u64;

            changes.added.push(DataFileInfo {
                path: file_path,
                size: file.size as u64,
                record_count,
                partition: HashMap::new(),
            });
        }

        // Write snapshot
        let mut summary = HashMap::new();
        let source = if self.config.source.is_empty() {
            "delta-import"
        } else {
            &self.config.source
        };
        summary.insert("source".to_string(), source.to_string());

        let write_result = metadata_service
            .write_snapshot(changes, OperationType::Append, summary)
            .await?;

        Ok(ImportResult {
            files_imported: files.len(),
            bytes_imported: total_bytes,
            records_imported: total_records,
            snapshot_id: write_result.id,
        })
    }
}

impl Default for ImportService {
    fn default() -> Self {
        Self::new()
    }
}
