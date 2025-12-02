//! Delta Lake metadata service implementation
//!
//! Encapsulates the logic for Delta Lake commits:
//! - Creating Add/Remove actions
//! - Committing transactions
//! - Reading table state

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use deltalake::kernel::transaction::CommitBuilder;
use deltalake::kernel::Action;
use deltalake::protocol::DeltaOperation;
use deltalake::DeltaTable;

use super::traits::{
    DataFileChanges, DataFileInfo, MetadataService, OperationType, SnapshotInfo,
};
use crate::core::formats::DeltaHandler;
use crate::error::{Error, Result};

/// Delta Lake metadata service for transactional operations
pub struct DeltaMetadataService {
    table_path: PathBuf,
}

impl DeltaMetadataService {
    /// Create a new Delta metadata service
    pub fn new(table_path: PathBuf) -> Result<Self> {
        Ok(Self { table_path })
    }

    /// Open the Delta table
    async fn open_table(&self) -> Result<DeltaTable> {
        deltalake::open_table(&self.table_path.to_string_lossy())
            .await
            .map_err(|e| Error::General(format!("Failed to open Delta table: {}", e)))
    }

    /// Convert DataFileInfo to Delta Add action
    fn to_add_action(&self, info: &DataFileInfo) -> Action {
        let rel_path = info
            .path
            .strip_prefix("file://")
            .unwrap_or(&info.path)
            .strip_prefix(&self.table_path.to_string_lossy().as_ref())
            .unwrap_or(&info.path)
            .trim_start_matches('/');

        Action::Add(deltalake::kernel::Add {
            path: rel_path.to_string(),
            partition_values: info
                .partition
                .iter()
                .map(|(k, v)| (k.clone(), Some(v.clone())))
                .collect(),
            size: info.size as i64,
            modification_time: chrono::Utc::now().timestamp_millis(),
            data_change: true,
            stats: None,
            tags: None,
            deletion_vector: None,
            base_row_id: None,
            default_row_commit_version: None,
            clustering_provider: None,
        })
    }

    /// Convert DataFileInfo to Delta Remove action
    fn to_remove_action(&self, info: &DataFileInfo) -> Action {
        let rel_path = info
            .path
            .strip_prefix("file://")
            .unwrap_or(&info.path)
            .strip_prefix(&self.table_path.to_string_lossy().as_ref())
            .unwrap_or(&info.path)
            .trim_start_matches('/');

        Action::Remove(deltalake::kernel::Remove {
            path: rel_path.to_string(),
            deletion_timestamp: Some(chrono::Utc::now().timestamp_millis()),
            data_change: true,
            extended_file_metadata: None,
            partition_values: None,
            size: Some(info.size as i64),
            deletion_vector: None,
            base_row_id: None,
            default_row_commit_version: None,
            tags: None,
        })
    }

    /// Convert OperationType to DeltaOperation
    fn to_delta_operation(&self, op: OperationType, info: &HashMap<String, String>) -> DeltaOperation {
        match op {
            OperationType::Append => DeltaOperation::Write {
                mode: deltalake::protocol::SaveMode::Append,
                partition_by: None,
                predicate: None,
            },
            OperationType::Replace | OperationType::Overwrite => DeltaOperation::Optimize {
                predicate: None,
                target_size: info
                    .get("target_size")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(268435456),
            },
            OperationType::Delete => DeltaOperation::Delete { predicate: None },
            OperationType::Restore => DeltaOperation::Restore {
                version: info.get("version").and_then(|s| s.parse().ok()),
                datetime: None,
            },
            OperationType::Repair => DeltaOperation::FileSystemCheck {},
        }
    }

    /// Convert Delta file URI to DataFileInfo
    fn uri_to_data_file_info(&self, uri: &str) -> DataFileInfo {
        let path = crate::core::utils::normalize_path(uri);

        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

        // Try to read record count from parquet
        let record_count = crate::core::utils::read_parquet_record_count(&path);

        DataFileInfo {
            path,
            size,
            record_count,
            partition: HashMap::new(),
        }
    }
}

#[async_trait]
impl MetadataService for DeltaMetadataService {
    async fn current_snapshot(&self) -> Result<Option<SnapshotInfo>> {
        let table = self.open_table().await?;
        let version = table.version().unwrap_or(0);

        // Try to get commit info
        let timestamp_ms = chrono::Utc::now().timestamp_millis();

        Ok(Some(SnapshotInfo {
            id: version,
            timestamp_ms,
            operation: "UNKNOWN".to_string(),
            summary: HashMap::new(),
            parent_id: if version > 0 { Some(version - 1) } else { None },
        }))
    }

    async fn list_data_files(&self) -> Result<Vec<DataFileInfo>> {
        let table = self.open_table().await?;

        let file_uris: Vec<String> = table
            .get_file_uris()
            .map_err(|e| Error::General(format!("Failed to get file URIs: {}", e)))?
            .collect();

        Ok(file_uris
            .iter()
            .map(|uri| self.uri_to_data_file_info(uri))
            .collect())
    }

    async fn list_snapshots(&self, limit: Option<usize>) -> Result<Vec<SnapshotInfo>> {
        let _table = self.open_table().await?;

        let log_dir = self.table_path.join("_delta_log");
        let mut snapshots = Vec::new();

        // Read commit files to get history
        if let Ok(entries) = std::fs::read_dir(&log_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();

                if name.ends_with(".json") && !name.contains("checkpoint") {
                    if let Ok(version) = name.trim_end_matches(".json").parse::<i64>() {
                        // Read commit info from the file
                        let mut operation = "UNKNOWN".to_string();
                        let mut timestamp_ms = 0i64;
                        let mut summary = HashMap::new();

                        if let Ok(content) = std::fs::read_to_string(entry.path()) {
                            for line in content.lines() {
                                if let Ok(json) =
                                    serde_json::from_str::<serde_json::Value>(line)
                                {
                                    if let Some(commit) = json.get("commitInfo") {
                                        if let Some(op) = commit
                                            .get("operation")
                                            .and_then(|v| v.as_str())
                                        {
                                            operation = op.to_string();
                                        }
                                        if let Some(ts) =
                                            commit.get("timestamp").and_then(|v| v.as_i64())
                                        {
                                            timestamp_ms = ts;
                                        }
                                        if let Some(metrics) = commit.get("operationMetrics") {
                                            if let Some(obj) = metrics.as_object() {
                                                for (k, v) in obj {
                                                    if let Some(s) = v.as_str() {
                                                        summary.insert(k.clone(), s.to_string());
                                                    } else {
                                                        summary.insert(k.clone(), v.to_string());
                                                    }
                                                }
                                            }
                                        }
                                        break;
                                    }
                                }
                            }
                        }

                        snapshots.push(SnapshotInfo {
                            id: version,
                            timestamp_ms,
                            operation,
                            summary,
                            parent_id: if version > 0 { Some(version - 1) } else { None },
                        });
                    }
                }
            }
        }

        // Sort by version descending
        snapshots.sort_by_key(|s| -s.id);

        if let Some(n) = limit {
            snapshots.truncate(n);
        }

        Ok(snapshots)
    }

    async fn write_snapshot(
        &self,
        changes: DataFileChanges,
        operation: OperationType,
        summary: HashMap<String, String>,
    ) -> Result<SnapshotInfo> {
        let table = self.open_table().await?;

        // Build actions
        let mut actions: Vec<Action> = Vec::new();

        // Add new files
        for file_info in &changes.added {
            actions.push(self.to_add_action(file_info));
        }

        // Remove old files
        for file_info in &changes.removed {
            actions.push(self.to_remove_action(file_info));
        }

        // Get operation
        let delta_operation = self.to_delta_operation(operation, &summary);

        // Commit
        let log_store = table.log_store();
        let snapshot = table
            .snapshot()
            .map_err(|e| Error::General(format!("Failed to get snapshot: {}", e)))?;

        let commit_result = CommitBuilder::default()
            .with_actions(actions)
            .build(Some(snapshot), log_store, delta_operation)
            .await
            .map_err(|e| Error::General(format!("Failed to commit: {}", e)))?;

        let new_version = commit_result.version;

        Ok(SnapshotInfo {
            id: new_version,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            operation: operation.to_string(),
            summary,
            parent_id: Some(new_version - 1),
        })
    }

    fn data_directory(&self) -> PathBuf {
        self.table_path.clone()
    }

    async fn schema(&self) -> Result<Arc<arrow::datatypes::Schema>> {
        let table = self.open_table().await?;
        let snapshot = table
            .snapshot()
            .map_err(|e| Error::General(format!("Failed to get snapshot: {}", e)))?;

        let delta_schema = snapshot.schema();
        let arrow_schema = DeltaHandler::delta_schema_to_arrow(delta_schema);

        Ok(Arc::new(arrow_schema))
    }
}
