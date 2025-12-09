//! Analysis service for table health assessment
//!
//! Provides methods to analyze table health and generate optimization recommendations.
//! This service works with Iceberg tables and calculates metrics for:
//! - Data file compaction needs
//! - Manifest compaction needs
//! - Snapshot expiration recommendations
//! - Orphan file detection

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::TryStreamExt;
use iceberg::spec::TableMetadata;

use super::types::{
    DataCompactionAnalysis, ManifestCompactionAnalysis, OrphanFilesAnalysis,
    PartitionCompactionInfo, SnapshotExpirationAnalysis, TableAnalysis,
};
use crate::core::maintenance::group_files_by_partition;
use crate::core::metadata::{DataFileInfo, IcebergMetadataService};
use crate::core::storage::ObjectStoreExt;
use crate::error::{Error, Result};

/// Default recommended maximum number of manifests
const DEFAULT_RECOMMENDED_MAX_MANIFESTS: usize = 10;

/// Configuration for analysis operations
#[derive(Debug, Clone)]
pub struct AnalysisConfig {
    /// Minimum file size threshold for small file detection
    pub min_file_size: u64,
    /// Whether to skip orphan file analysis (can be slow for large tables)
    pub skip_orphans: bool,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            min_file_size: 16 * 1024 * 1024, // 16 MB
            skip_orphans: false,
        }
    }
}

/// Service for analyzing table health
pub struct AnalyzeService {
    config: AnalysisConfig,
}

impl AnalyzeService {
    /// Create a new analysis service with default configuration
    pub fn new() -> Self {
        Self {
            config: AnalysisConfig::default(),
        }
    }

    /// Create a new analysis service with custom configuration
    pub fn with_config(config: AnalysisConfig) -> Self {
        Self { config }
    }

    /// Perform a complete table health analysis
    ///
    /// This runs all analysis operations and returns a complete health report.
    pub async fn analyze_table(&self, service: &IcebergMetadataService) -> Result<TableAnalysis> {
        let (metadata, _) = service.load_metadata().await?;

        let data_compaction = self.analyze_data_compaction(service).await?;
        let manifest_compaction = self.analyze_manifests(service).await?;
        let snapshot_expiration = self.analyze_snapshots(&metadata);

        let orphan_files = if self.config.skip_orphans {
            None
        } else {
            Some(self.analyze_orphans(service).await?)
        };

        Ok(TableAnalysis {
            table_path: service.table_path().to_string(),
            data_compaction,
            manifest_compaction,
            snapshot_expiration,
            orphan_files,
        })
    }

    /// Analyze data files for compaction opportunities
    ///
    /// Identifies partitions with small files that would benefit from compaction.
    pub async fn analyze_data_compaction(
        &self,
        service: &IcebergMetadataService,
    ) -> Result<DataCompactionAnalysis> {
        let table = service.table();
        let metadata = table.metadata();

        if metadata.current_snapshot().is_none() {
            return Ok(DataCompactionAnalysis {
                total_files: 0,
                small_files: 0,
                groups_needing_compaction: 0,
                total_size: 0,
                small_files_size: 0,
                min_size_threshold: self.config.min_file_size,
                partitions: Vec::new(),
            });
        }

        // Use native scan API - automatically filters deleted files
        let scan = table.scan().build()
            .map_err(|e| Error::General(format!("Failed to build scan: {}", e)))?;

        let tasks: Vec<_> = scan.plan_files().await
            .map_err(|e| Error::General(format!("Failed to plan files: {}", e)))?
            .try_collect().await
            .map_err(|e| Error::General(format!("Failed to collect tasks: {}", e)))?;

        let files: Vec<DataFileInfo> = tasks.iter().map(|task| {
            let path = task.data_file_path().to_string();
            DataFileInfo {
                path: path.clone(),
                size: task.length,
                record_count: task.record_count.unwrap_or(0),
                partition: extract_partition_from_path(&path),
            }
        }).collect();

        // Calculate statistics
        let total_files = files.len();
        let total_size: u64 = files.iter().map(|f| f.size).sum();

        let small_files: Vec<_> = files
            .iter()
            .filter(|f| f.size < self.config.min_file_size)
            .collect();
        let small_files_count = small_files.len();
        let small_files_size: u64 = small_files.iter().map(|f| f.size).sum();

        let groups = group_files_by_partition(files);

        // Collect partition-level details
        let mut partitions: Vec<PartitionCompactionInfo> = groups
            .iter()
            .filter(|(_, g)| g.needs_compaction(self.config.min_file_size))
            .map(|(key, g)| {
                let small_count = g
                    .files
                    .iter()
                    .filter(|f| f.size < self.config.min_file_size)
                    .count();
                let total_size: u64 = g.files.iter().map(|f| f.size).sum();
                let total_records: u64 = g.files.iter().map(|f| f.record_count).sum();

                // Priority score based on:
                // 1. Reduction ratio (files -> 1): more files = more benefit
                // 2. Number of records: more records = more query impact
                // Score = files * log2(records + 1) to balance both factors
                let reduction_ratio = g.files.len() as u64;
                let records_factor = ((total_records + 1) as f64).log2() as u64;
                let priority_score = reduction_ratio * records_factor.max(1);

                PartitionCompactionInfo {
                    partition: if key.is_empty() {
                        "(unpartitioned)".to_string()
                    } else {
                        key.clone()
                    },
                    files: g.files.len(),
                    small_files: small_count,
                    size_bytes: total_size,
                    records: total_records,
                    priority: String::new(), // Will be set after sorting
                    priority_score,
                }
            })
            .collect();

        // Sort by priority_score descending (highest priority first)
        partitions.sort_by(|a, b| b.priority_score.cmp(&a.priority_score));

        // Assign priority labels based on percentiles
        let total = partitions.len();
        for (i, p) in partitions.iter_mut().enumerate() {
            let percentile = (i as f64) / (total.max(1) as f64);
            p.priority = if percentile < 0.1 {
                "high".to_string() // Top 10%
            } else if percentile < 0.4 {
                "medium".to_string() // Next 30%
            } else {
                "low".to_string() // Bottom 60%
            };
        }

        let groups_needing_compaction = partitions.len();

        Ok(DataCompactionAnalysis {
            total_files,
            small_files: small_files_count,
            groups_needing_compaction,
            total_size,
            small_files_size,
            min_size_threshold: self.config.min_file_size,
            partitions,
        })
    }

    /// Analyze manifests for compaction recommendations
    ///
    /// Note: StaticTable doesn't expose inspect() API, so we use load_manifest_list()
    pub async fn analyze_manifests(
        &self,
        service: &IcebergMetadataService,
    ) -> Result<ManifestCompactionAnalysis> {
        let table = service.table();
        let metadata = table.metadata();

        let snapshot = match metadata.current_snapshot() {
            Some(s) => s,
            None => {
                return Ok(ManifestCompactionAnalysis {
                    total_manifests: 0,
                    recommended_max: DEFAULT_RECOMMENDED_MAX_MANIFESTS,
                });
            }
        };

        // Load manifest list using native API
        let manifest_list = snapshot
            .load_manifest_list(service.file_io(), metadata.as_ref())
            .await
            .map_err(|e| Error::General(format!("Failed to load manifest list: {}", e)))?;

        let total_manifests = manifest_list.entries().len();

        Ok(ManifestCompactionAnalysis {
            total_manifests,
            recommended_max: DEFAULT_RECOMMENDED_MAX_MANIFESTS,
        })
    }

    /// Analyze snapshots for expiration recommendations
    pub fn analyze_snapshots(&self, metadata: &Arc<TableMetadata>) -> SnapshotExpirationAnalysis {
        let now = chrono::Utc::now().timestamp_millis();
        let day_ms: i64 = 24 * 60 * 60 * 1000;

        let snapshots: Vec<_> = metadata.snapshots().collect();
        let total_snapshots = snapshots.len();

        let mut oldest_age_days: i64 = 0;
        let mut older_than_7d = 0;
        let mut older_than_30d = 0;

        for snapshot in &snapshots {
            let age_ms = now - snapshot.timestamp_ms();
            let age_days = age_ms / day_ms;

            if age_days > oldest_age_days {
                oldest_age_days = age_days;
            }

            if age_days >= 7 {
                older_than_7d += 1;
            }
            if age_days >= 30 {
                older_than_30d += 1;
            }
        }

        SnapshotExpirationAnalysis {
            total_snapshots,
            snapshots_older_than_7d: older_than_7d,
            snapshots_older_than_30d: older_than_30d,
            oldest_snapshot_age_days: oldest_age_days,
        }
    }

    /// Analyze for orphan files
    ///
    /// Scans all snapshots to find files that exist on storage but are not
    /// referenced by any snapshot, and files that are referenced but missing.
    pub async fn analyze_orphans(
        &self,
        service: &IcebergMetadataService,
    ) -> Result<OrphanFilesAnalysis> {
        let table = service.table();
        let metadata = table.metadata();
        let snapshots: Vec<_> = metadata.snapshots().collect();

        // Helper to normalize paths - extract just the relative path after table location
        let normalize_path = |path: &str| -> String {
            // If path contains /data/, extract from /data/ onwards
            if let Some(idx) = path.find("/data/") {
                return path[idx + 1..].to_string(); // "data/..."
            }
            // Fallback: just the filename
            path.rsplit('/').next().unwrap_or(path).to_string()
        };

        // Collect all referenced files from all snapshots using native scan API
        let mut referenced: HashSet<String> = HashSet::new();

        for snapshot in &snapshots {
            let scan = match table.scan()
                .snapshot_id(snapshot.snapshot_id())
                .build()
            {
                Ok(s) => s,
                Err(_) => continue,
            };

            let tasks: Vec<_> = match scan.plan_files().await {
                Ok(stream) => match stream.try_collect().await {
                    Ok(t) => t,
                    Err(_) => continue,
                },
                Err(_) => continue,
            };

            for task in tasks {
                let path = task.data_file_path().to_string();
                referenced.insert(normalize_path(&path));
            }
        }

        // List files on storage
        let table_path = service.table_path();
        let data_prefix = format!("{}/data/", table_path.trim_end_matches('/'));
        let storage = service.storage();

        let all_objects = storage.list_prefix(&data_prefix).await?;

        let on_storage: Vec<DataFileInfo> = all_objects
            .iter()
            .filter(|obj| obj.location.to_string().ends_with(".parquet"))
            .map(|obj| {
                let path = obj.location.to_string();
                DataFileInfo {
                    path: normalize_path(&path),
                    size: obj.size,
                    record_count: 0,
                    partition: HashMap::new(),
                }
            })
            .collect();

        // Calculate orphans and missing
        let mut orphan_count = 0;
        let mut orphan_size = 0u64;
        let mut missing_count = 0;

        for file in &on_storage {
            if !referenced.contains(&file.path) {
                orphan_count += 1;
                orphan_size += file.size;
            }
        }

        let storage_paths: HashSet<_> = on_storage.iter().map(|f| &f.path).collect();
        for path in &referenced {
            if !storage_paths.contains(path) {
                missing_count += 1;
            }
        }

        Ok(OrphanFilesAnalysis {
            orphan_count,
            orphan_size,
            missing_count,
        })
    }
}

impl Default for AnalyzeService {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract partition key=value pairs from a file path
///
/// e.g., "s3://bucket/data/day=2024-01-01/currency=USD/file.parquet"
///       -> {"day": "2024-01-01", "currency": "USD"}
fn extract_partition_from_path(path: &str) -> HashMap<String, String> {
    let mut partition = HashMap::new();

    for segment in path.split('/') {
        if let Some(idx) = segment.find('=') {
            let key = &segment[..idx];
            let value = &segment[idx + 1..];
            // Skip if it looks like a file, not a partition
            if !value.contains('.') {
                partition.insert(key.to_string(), value.to_string());
            }
        }
    }

    partition
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_partition_from_path() {
        let path = "s3://bucket/data/day=2024-01-01/currency=USD/file.parquet";
        let partition = extract_partition_from_path(path);

        assert_eq!(partition.get("day"), Some(&"2024-01-01".to_string()));
        assert_eq!(partition.get("currency"), Some(&"USD".to_string()));
        assert_eq!(partition.len(), 2);
    }

    #[test]
    fn test_extract_partition_no_partitions() {
        let path = "s3://bucket/data/file.parquet";
        let partition = extract_partition_from_path(path);

        assert!(partition.is_empty());
    }

    #[test]
    fn test_default_config() {
        let config = AnalysisConfig::default();
        assert_eq!(config.min_file_size, 16 * 1024 * 1024);
        assert!(!config.skip_orphans);
    }
}
