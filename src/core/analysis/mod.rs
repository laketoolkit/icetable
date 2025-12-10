//! Analysis module for table health assessment
//!
//! Provides services and types for analyzing table health and generating
//! optimization recommendations for Iceberg tables.

mod service;
mod types;

pub use service::{get_partition_stats, AnalysisConfig, AnalyzeService};
pub use types::{
    DataCompactionAnalysis, ManifestCompactionAnalysis, OrphanFilesAnalysis,
    PartitionCompactionInfo, PartitionStats, SnapshotExpirationAnalysis, TableAnalysis,
};
