//! Core traits and types for physical inspection of table formats
//!
//! This module defines the [`PhysicalInspector`] trait and associated types that enable
//! format-agnostic inspection of data files. The trait provides a unified interface for
//! extracting metadata, schema, layout, and statistics from various table formats.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                   PhysicalInspector trait                   │
//! └─────────────────────────────────────────────────────────────┘
//!                              ▲
//!        ┌─────────────────────┼─────────────────────┐
//!        │                     │                     │
//! ┌──────┴──────┐      ┌───────┴───────┐     ┌───────┴───────┐
//! │ParquetInsp. │      │ IcebergInsp.  │     │  DeltaInsp.   │
//! └─────────────┘      └───────────────┘     └───────────────┘
//! ```
//!
//! # Implementing a New Inspector
//!
//! To add support for a new table format:
//!
//! 1. Implement the [`PhysicalInspector`] trait
//! 2. Implement format-specific metadata extraction in `extract_metadata`
//! 3. Return appropriate [`LayoutInfo`] variant for the format
//! 4. Register in the inspector factory
//!
//! # Example
//!
//! ```ignore
//! use icetable::core::inspection::{PhysicalInspector, PhysicalInspectOptions, VerbosityLevel};
//!
//! // Create inspector for a table
//! let inspector = IcebergInspector::new("/path/to/iceberg/table").await?;
//!
//! // Configure inspection options
//! let options = PhysicalInspectOptions::from_cli_args(
//!     true,   // show_schema
//!     true,   // show_layout
//!     true,   // show_stats
//!     VerbosityLevel::Normal,
//!     false,  // deep_scan
//! );
//!
//! // Extract metadata
//! let metadata = inspector.extract_metadata(&options).await?;
//!
//! // Access format-specific layout info
//! if let Some(LayoutInfo::FileBased(layout)) = metadata.layout {
//!     println!("Table has {} data files", layout.num_files);
//! }
//! ```
//!
//! # Data Types
//!
//! The module provides several data structures to represent inspection results:
//!
//! - [`PhysicalMetadata`] - Complete metadata from inspection
//! - [`SchemaInfo`] / [`ColumnInfo`] - Schema and column definitions
//! - [`LayoutInfo`] - Physical layout (row groups, batches, or files)
//! - [`StatisticsInfo`] / [`ColumnStatistics`] - Row/column statistics
//! - [`OrphanFilesInfo`] - Orphan file detection results

use crate::error::Result;
use async_trait::async_trait;
use std::collections::HashMap;

/// Verbosity level for inspection output
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VerbosityLevel {
    /// Normal output
    Normal = 0,
    /// Verbose output (-v) - all details
    Verbose = 1,
}

/// Options for physical layout inspection
#[derive(Debug, Clone)]
pub struct PhysicalInspectOptions {
    /// Show schema section
    pub show_schema: bool,
    /// Show physical layout section
    pub show_layout: bool,
    /// Show statistics section
    pub show_stats: bool,
    /// Verbosity level
    pub verbosity: VerbosityLevel,
    /// Deep scan: check all snapshots for orphan detection (slower but accurate)
    pub deep_scan: bool,
}

impl PhysicalInspectOptions {
    /// Create options from CLI args
    pub fn from_cli_args(
        schema: bool,
        layout: bool,
        stats: bool,
        verbosity: VerbosityLevel,
        deep_scan: bool,
    ) -> Self {
        let any_flag = schema || layout || stats;

        Self {
            show_schema: if any_flag { schema } else { true },
            show_layout: if any_flag { layout } else { true },
            show_stats: if any_flag { stats } else { true },
            verbosity,
            deep_scan,
        }
    }
}

/// Complete metadata extracted from physical inspection
#[derive(Debug, Clone)]
pub struct PhysicalMetadata {
    /// Format name (e.g., "Apache Parquet")
    pub format_name: String,
    /// File-level information
    pub file_info: FileInfo,
    /// Schema information (optional)
    pub schema: Option<SchemaInfo>,
    /// Physical layout information (optional)
    pub layout: Option<LayoutInfo>,
    /// Statistics information (optional)
    pub statistics: Option<StatisticsInfo>,
    /// Orphan files information (optional)
    pub orphan_files: Option<OrphanFilesInfo>,
}

/// Information about orphan files (files in data/ not tracked in metadata)
#[derive(Debug, Clone)]
pub struct OrphanFilesInfo {
    /// Number of orphan files found
    pub count: usize,
    /// Total size of orphan files in bytes
    pub total_size: u64,
    /// List of orphan file paths (limited to first N)
    pub files: Vec<OrphanFileEntry>,
    /// Whether the list is truncated
    pub truncated: bool,
    /// Whether this was a deep scan (all snapshots) or quick scan (current only)
    pub is_deep_scan: bool,
}

/// Single orphan file entry
#[derive(Debug, Clone)]
pub struct OrphanFileEntry {
    /// File path
    pub path: String,
    /// File size in bytes
    pub size: u64,
}

/// File-level information
#[derive(Debug, Clone)]
pub struct FileInfo {
    /// File path
    pub path: String,
    /// File size in bytes
    pub file_size: u64,
    /// Format version
    pub format_version: String,
    /// Creator/writer information
    pub created_by: Option<String>,
    /// Additional metadata as key-value pairs
    pub metadata: HashMap<String, String>,
}

/// Schema information (format-agnostic)
#[derive(Debug, Clone)]
pub struct SchemaInfo {
    /// Number of columns
    pub num_columns: usize,
    /// Column definitions
    pub columns: Vec<ColumnInfo>,
}

/// Column information
#[derive(Debug, Clone)]
pub struct ColumnInfo {
    /// Column name
    pub name: String,
    /// Column type (format-specific representation)
    pub column_type: String,
    /// Whether column is nullable
    pub nullable: bool,
    /// Column index/position
    pub index: usize,
}

/// Physical layout information (varies by format)
#[derive(Debug, Clone)]
pub enum LayoutInfo {
    /// Row group-based layout (Parquet)
    RowGroupBased(RowGroupLayout),
    /// Batch-based layout (Arrow IPC)
    BatchBased(BatchLayout),
    /// File-based layout (Delta Lake, Iceberg)
    FileBased(FileBasedLayout),
    /// Unstructured layout (CSV, JSON)
    Unstructured(UnstructuredLayout),
}

/// Row group layout for Parquet
#[derive(Debug, Clone)]
pub struct RowGroupLayout {
    /// Number of row groups
    pub num_row_groups: usize,
    /// Row group metadata
    pub row_groups: Vec<RowGroupMetadata>,
}

/// Row group metadata
#[derive(Debug, Clone)]
pub struct RowGroupMetadata {
    /// Row group index
    pub index: usize,
    /// Number of rows
    pub num_rows: i64,
    /// Total compressed size
    pub total_compressed_size: i64,
    /// Total uncompressed size
    pub total_uncompressed_size: i64,
    /// Column chunks
    pub columns: Vec<ColumnChunkMetadata>,
}

/// Column chunk metadata
#[derive(Debug, Clone)]
pub struct ColumnChunkMetadata {
    /// Column name
    pub column_name: String,
    /// Compression codec
    pub compression: String,
    /// Compressed size
    pub compressed_size: i64,
    /// Uncompressed size
    pub uncompressed_size: i64,
    /// Encoding
    pub encoding: String,
}

/// Batch layout for Arrow IPC
#[derive(Debug, Clone)]
pub struct BatchLayout {
    /// Number of record batches
    pub num_batches: usize,
    /// Batch metadata
    pub batches: Vec<BatchMetadata>,
}

/// Batch metadata
#[derive(Debug, Clone)]
pub struct BatchMetadata {
    /// Batch index
    pub index: usize,
    /// Number of rows
    pub num_rows: u64,
    /// Metadata length
    pub metadata_length: u64,
    /// Body length
    pub body_length: u64,
}

/// File-based layout (Delta/Iceberg)
#[derive(Debug, Clone)]
pub struct FileBasedLayout {
    /// Number of data files
    pub num_files: usize,
    /// Total size of all files
    pub total_size: u64,
    /// Partitioning information
    pub partitioning: Option<String>,
    /// Additional layout-specific info
    pub details: HashMap<String, String>,
}

/// Unstructured layout (CSV/JSON)
#[derive(Debug, Clone)]
pub struct UnstructuredLayout {
    /// Estimated number of records
    pub estimated_records: Option<i64>,
    /// Delimiter (for CSV)
    pub delimiter: Option<char>,
    /// Has header row (for CSV)
    pub has_header: Option<bool>,
}

/// Statistics information
#[derive(Debug, Clone)]
pub struct StatisticsInfo {
    /// Total number of rows
    pub total_rows: i64,
    /// Compressed size
    pub compressed_size: u64,
    /// Uncompressed size
    pub uncompressed_size: u64,
    /// Per-column statistics
    pub column_stats: Vec<ColumnStatistics>,
}

/// Column statistics
#[derive(Debug, Clone)]
pub struct ColumnStatistics {
    /// Column name
    pub column_name: String,
    /// Number of null values
    pub null_count: Option<i64>,
    /// Minimum value (as string)
    pub min_value: Option<String>,
    /// Maximum value (as string)
    pub max_value: Option<String>,
    /// Distinct count (if available)
    pub distinct_count: Option<i64>,
}

/// Core trait for physical inspection of table formats
///
/// This trait defines the interface that all format inspectors must implement.
/// It enables icetable to work uniformly with different table formats (Parquet,
/// Iceberg, Delta Lake, etc.) while allowing each implementation to handle
/// format-specific details.
///
/// # Required Methods
///
/// - [`extract_metadata`](Self::extract_metadata) - Main inspection method that extracts
///   all metadata according to the provided options
/// - [`format_name`](Self::format_name) - Returns the human-readable format name
/// - [`can_inspect`](Self::can_inspect) - Quick check to determine if this inspector
///   can handle a given path
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` to support concurrent inspection of
/// multiple tables.
///
/// # Example Implementation
///
/// ```ignore
/// #[async_trait]
/// impl PhysicalInspector for MyFormatInspector {
///     async fn extract_metadata(&self, options: &PhysicalInspectOptions) -> Result<PhysicalMetadata> {
///         let file_info = self.read_file_info()?;
///         let schema = if options.show_schema { Some(self.read_schema()?) } else { None };
///         let layout = if options.show_layout { Some(self.read_layout()?) } else { None };
///         let stats = if options.show_stats { Some(self.read_stats().await?) } else { None };
///
///         Ok(PhysicalMetadata {
///             format_name: self.format_name().to_string(),
///             file_info,
///             schema,
///             layout,
///             statistics: stats,
///             orphan_files: None,
///         })
///     }
///
///     fn format_name(&self) -> &str {
///         "My Custom Format"
///     }
///
///     fn can_inspect(&self, path: &str) -> bool {
///         path.ends_with(".myformat")
///     }
/// }
/// ```
#[async_trait]
pub trait PhysicalInspector: Send + Sync {
    /// Extract metadata from physical structure
    ///
    /// This is the main inspection method. Implementations should respect the
    /// options provided and only compute/return the requested sections.
    ///
    /// # Arguments
    ///
    /// * `options` - Controls which sections to extract (schema, layout, stats)
    ///   and the verbosity level
    ///
    /// # Returns
    ///
    /// A [`PhysicalMetadata`] struct containing all requested information about
    /// the table's physical structure.
    async fn extract_metadata(&self, options: &PhysicalInspectOptions) -> Result<PhysicalMetadata>;

    /// Get format name
    ///
    /// Returns a human-readable name for the format (e.g., "Apache Iceberg",
    /// "Apache Parquet", "Delta Lake").
    fn format_name(&self) -> &str;

    /// Quick detection (fast, based on extension/magic bytes)
    ///
    /// This method should be fast and avoid I/O when possible. It's used to
    /// determine which inspector to use for a given path before attempting
    /// full inspection.
    ///
    /// # Arguments
    ///
    /// * `path` - The path or URL to check (e.g., "s3://bucket/table" or "/local/path")
    ///
    /// # Returns
    ///
    /// `true` if this inspector can likely handle the given path
    fn can_inspect(&self, path: &str) -> bool;
}
