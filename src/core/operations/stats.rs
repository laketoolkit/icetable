//! Stats service for retrieving table statistics
//!
//! Provides functionality to retrieve pre-computed statistics from
//! snapshot summary metadata. This is instantaneous as it doesn't
//! require scanning manifests or data files.

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};

use crate::core::analysis::get_partition_stats;
use crate::core::formats::FormatHandlerFactory;
use crate::core::maintenance::PartitionFilter;
use crate::core::storage::create_object_store;
use crate::error::Result;

/// Configuration for stats retrieval
#[derive(Debug, Clone, Default)]
pub struct StatsConfig {
    /// Optional partition filter for detailed stats
    pub partition: Option<String>,
}

/// General table statistics
#[derive(Debug, Clone)]
pub struct TableStats {
    /// Table name (extracted from path)
    pub table_name: String,
    /// Format name (iceberg, delta, parquet)
    pub format: String,
    /// Total number of records (if available)
    pub total_records: Option<i64>,
    /// Compressed size in bytes (if available)
    pub compressed_size: Option<u64>,
    /// Format version (if available)
    pub format_version: Option<String>,
    /// Last modified timestamp (if available)
    pub last_modified: Option<DateTime<Utc>>,
    /// Additional properties from metadata
    pub properties: HashMap<String, String>,
}

/// Partition-specific statistics
#[derive(Debug, Clone)]
pub struct PartitionStats {
    /// Table name
    pub table_name: String,
    /// Format name
    pub format: String,
    /// Partition filter used
    pub partition_filter: String,
    /// Number of files in partition
    pub file_count: usize,
    /// Total size of files
    pub total_size: u64,
    /// Average file size
    pub avg_file_size: u64,
    /// Number of small files (<128MB)
    pub small_files: usize,
    /// Percentage of small files
    pub small_files_percent: f64,
    /// Recommended target size for optimization
    pub recommended_target_size: u64,
}

/// Result of stats operation
#[derive(Debug)]
pub enum StatsResult {
    /// General table statistics
    Table(TableStats),
    /// Partition-specific statistics
    Partition(PartitionStats),
}

/// Service for retrieving table statistics
pub struct StatsService;

impl StatsService {
    /// Get statistics for a table
    pub async fn get_stats(table_path: &str, config: &StatsConfig) -> Result<StatsResult> {
        let path = Path::new(table_path);

        // Create storage backend
        let storage = create_object_store(table_path).await?;

        // Get format handler (auto-detect)
        let handler = FormatHandlerFactory::create_handler(path, storage).await?;

        let format_name = handler.format_name().to_string();
        let table_name = Self::extract_table_name(table_path);

        // If partition filter specified, get partition stats
        if let Some(partition_filter_str) = &config.partition {
            let partition_filter = PartitionFilter::parse(partition_filter_str).map_err(|e| {
                crate::error::Error::Parse {
                    message: format!("Invalid partition filter: {}", e),
                    source: None,
                }
            })?;

            let partition_stats = get_partition_stats(table_path, &partition_filter).await?;

            return Ok(StatsResult::Partition(PartitionStats {
                table_name,
                format: format_name,
                partition_filter: partition_filter_str.clone(),
                file_count: partition_stats.file_count,
                total_size: partition_stats.total_size,
                avg_file_size: partition_stats.avg_file_size,
                small_files: partition_stats.small_files,
                small_files_percent: partition_stats.small_files_percent,
                recommended_target_size: partition_stats.recommended_target_size,
            }));
        }

        // General table stats from metadata
        let metadata = handler.read_metadata().await?;

        Ok(StatsResult::Table(TableStats {
            table_name,
            format: format_name,
            total_records: metadata.num_rows,
            compressed_size: metadata.compressed_size,
            format_version: metadata.format_version,
            last_modified: metadata.created_at,
            properties: metadata.metadata,
        }))
    }

    /// Extract table name from path
    fn extract_table_name(path: &str) -> String {
        Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| path.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_table_name() {
        assert_eq!(
            StatsService::extract_table_name("/path/to/my_table"),
            "my_table"
        );
        assert_eq!(
            StatsService::extract_table_name("s3://bucket/tables/orders"),
            "orders"
        );
        assert_eq!(StatsService::extract_table_name("my_table"), "my_table");
    }

    #[test]
    fn test_stats_config_default() {
        let config = StatsConfig::default();
        assert!(config.partition.is_none());
    }
}
