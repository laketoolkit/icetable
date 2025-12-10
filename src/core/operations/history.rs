//! History service for retrieving table version history
//!
//! Provides functionality to retrieve and format snapshot history
//! from Iceberg tables.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};

use crate::core::{IcebergTable, TableExt};
use crate::error::Result;

/// A single version/snapshot entry in history
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// Snapshot ID
    pub version: i64,
    /// Timestamp of the version
    pub timestamp: DateTime<Utc>,
    /// Operation type (e.g., "Append", "Overwrite")
    pub operation: String,
    /// Additional details about the operation
    pub details: HashMap<String, String>,
    /// Whether this is the current snapshot
    pub is_current: bool,
}

/// Configuration for history retrieval
#[derive(Debug, Clone, Default)]
pub struct HistoryConfig {
    /// Maximum number of entries to return (None = all)
    pub limit: Option<usize>,
    /// Whether to include all snapshots (overrides limit)
    pub all: bool,
}

/// Service for retrieving table history
pub struct HistoryService;

impl HistoryService {
    /// Get history entries from an Iceberg table
    pub fn get_history(table: &Arc<IcebergTable>, config: &HistoryConfig) -> Result<Vec<HistoryEntry>> {
        let (metadata, _) = table.metadata_with_version();
        let current_snapshot_id = metadata.current_snapshot_id();

        let mut entries = Vec::new();

        // Collect ALL snapshots first
        for snapshot in table.snapshots() {
            let summary = snapshot.summary();
            let mut details = HashMap::new();

            details.insert("operation".to_string(), format!("{:?}", summary.operation));
            for (k, v) in &summary.additional_properties {
                details.insert(k.clone(), v.clone());
            }

            let timestamp = Utc
                .timestamp_millis_opt(snapshot.timestamp_ms())
                .single()
                .unwrap_or_else(Utc::now);

            entries.push(HistoryEntry {
                version: snapshot.snapshot_id(),
                timestamp,
                operation: format!("{:?}", summary.operation),
                details,
                is_current: Some(snapshot.snapshot_id()) == current_snapshot_id,
            });
        }

        // Sort by timestamp descending (newest first)
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        // Apply limit AFTER sorting (unless all is requested)
        if !config.all
            && let Some(limit) = config.limit
        {
            entries.truncate(limit);
        }

        Ok(entries)
    }

    /// Get a summary of changes for a history entry
    pub fn format_details(entry: &HistoryEntry) -> Vec<String> {
        let mut details = Vec::new();

        if let Some(added) = entry.details.get("added-records") {
            details.push(format!("+{} records", added));
        }
        if let Some(deleted) = entry.details.get("deleted-records") {
            details.push(format!("-{} records", deleted));
        }
        if let Some(files) = entry.details.get("added-data-files") {
            details.push(format!("+{} files", files));
        }
        if let Some(files) = entry.details.get("deleted-data-files") {
            details.push(format!("-{} files", files));
        }
        if let Some(total) = entry.details.get("total-records") {
            details.push(format!("total: {} records", total));
        }

        details
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_details_empty() {
        let entry = HistoryEntry {
            version: 1,
            timestamp: Utc::now(),
            operation: "Append".to_string(),
            details: HashMap::new(),
            is_current: true,
        };

        let details = HistoryService::format_details(&entry);
        assert!(details.is_empty());
    }

    #[test]
    fn test_format_details_with_records() {
        let mut details = HashMap::new();
        details.insert("added-records".to_string(), "100".to_string());
        details.insert("total-records".to_string(), "500".to_string());

        let entry = HistoryEntry {
            version: 1,
            timestamp: Utc::now(),
            operation: "Append".to_string(),
            details,
            is_current: true,
        };

        let formatted = HistoryService::format_details(&entry);
        assert!(formatted.iter().any(|s| s.contains("+100 records")));
        assert!(formatted.iter().any(|s| s.contains("total: 500 records")));
    }
}
