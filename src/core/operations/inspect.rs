//! Inspect operation - view contents and metadata of tables
//!
//! This module provides two inspection modes:
//! - `InspectOperation`: For inspecting raw files (Parquet, CSV, etc.) via FormatHandler
//! - `IcebergTableInspector`: For inspecting Iceberg tables via native iceberg-rs API

use std::sync::Arc;

use arrow::datatypes::Schema;
use arrow::record_batch::RecordBatch;
use serde::Serialize;

use crate::core::formats::{ColumnStats, FileMetadata, FormatHandler, ReadOptions};
use crate::error::Result;

// ═══════════════════════════════════════════════════════════════════════════════
// FILE INSPECTION (Parquet, CSV, etc.)
// ═══════════════════════════════════════════════════════════════════════════════

/// Options for inspect operation
#[derive(Debug, Clone)]
pub struct InspectOptions {
    /// Show only schema (no data) - deprecated, use show_data instead
    pub schema_only: bool,

    /// Show schema
    pub show_schema: bool,

    /// Show metadata
    pub show_metadata: bool,

    /// Show statistics
    pub show_stats: bool,

    /// Show data preview
    pub show_data: bool,

    /// Number of rows to sample
    pub num_rows: usize,

    /// Columns to include
    pub columns: Option<Vec<String>>,

    /// Use random sampling
    pub sample: bool,
}

impl Default for InspectOptions {
    fn default() -> Self {
        Self {
            schema_only: false,
            show_schema: true,
            show_metadata: true,
            show_stats: false,
            show_data: true,
            num_rows: 10,
            columns: None,
            sample: false,
        }
    }
}

/// Operation for inspecting file contents (Parquet, CSV, etc.)
pub struct InspectOperation {
    handler: Box<dyn FormatHandler>,
}

impl InspectOperation {
    /// Create a new inspect operation
    pub fn new(handler: Box<dyn FormatHandler>) -> Self {
        Self { handler }
    }

    /// Execute the inspect operation
    pub async fn execute(&self, options: &InspectOptions) -> Result<InspectResult> {
        // Always read schema
        let schema = self.handler.read_schema().await?;

        // Read metadata if requested
        let metadata = if options.show_metadata {
            Some(self.handler.read_metadata().await?)
        } else {
            None
        };

        // Read statistics if requested
        let statistics = if options.show_stats {
            Some(self.handler.read_statistics().await?)
        } else {
            None
        };

        // Read sample data if requested
        let sample_data = if options.show_data && !options.schema_only {
            let mut read_opts_builder = ReadOptions::builder()
                .limit(options.num_rows)
                .sample(options.sample)
                .batch_size(1024);

            if let Some(cols) = options.columns.clone() {
                read_opts_builder = read_opts_builder.columns(cols);
            }

            let read_opts = read_opts_builder.build();

            Some(self.handler.read_batch(&read_opts).await?)
        } else {
            None
        };

        Ok(InspectResult {
            format_name: self.handler.format_name().to_string(),
            schema,
            metadata,
            statistics,
            sample_data,
        })
    }
}

/// Result of an inspect operation (for files)
#[derive(Debug, Serialize)]
pub struct InspectResult {
    /// Format name (e.g., "Apache Parquet")
    pub format_name: String,

    /// Table schema
    #[serde(skip)]
    pub schema: Arc<Schema>,

    /// File metadata (if requested)
    #[serde(skip)]
    pub metadata: Option<FileMetadata>,

    /// Column statistics (if requested)
    #[serde(skip)]
    pub statistics: Option<Vec<ColumnStats>>,

    /// Sample data (if requested)
    #[serde(skip)]
    pub sample_data: Option<RecordBatch>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ICEBERG TABLE INSPECTION (using native iceberg-rs API)
// ═══════════════════════════════════════════════════════════════════════════════

/// Options for Iceberg table inspection
#[derive(Debug, Clone, Default)]
pub struct IcebergInspectOptions {
    /// Verbose mode - show additional details like snapshot history
    pub verbose: bool,
}

impl IcebergInspectOptions {
    /// Create options from CLI args
    pub fn from_cli(verbose: bool) -> Self {
        Self { verbose }
    }
}

/// Information about a schema field
#[derive(Debug, Clone, Serialize)]
pub struct FieldInfo {
    /// Field ID (used for schema evolution)
    pub field_id: i32,
    /// Field name
    pub name: String,
    /// Field type as string
    pub field_type: String,
    /// Whether the field is required (non-nullable)
    pub required: bool,
    /// Documentation string if present
    pub doc: Option<String>,
    /// Whether this field is an identifier field
    pub is_identifier: bool,
}

/// Information about a partition field
#[derive(Debug, Clone, Serialize)]
pub struct PartitionFieldInfo {
    /// Partition field name
    pub name: String,
    /// Transform applied (Identity, Year, Month, Day, Hour, Bucket, Truncate, etc.)
    pub transform: String,
    /// Source field ID in the schema
    pub source_id: i32,
    /// Partition field ID
    pub field_id: i32,
}

/// Information about a sort field
#[derive(Debug, Clone, Serialize)]
pub struct SortFieldInfo {
    /// Source field ID
    pub source_id: i32,
    /// Sort direction (Ascending/Descending)
    pub direction: String,
    /// Null order (NullsFirst/NullsLast)
    pub null_order: String,
}

/// Snapshot information
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotInfo {
    /// Snapshot ID
    pub snapshot_id: i64,
    /// Sequence number (for conflict resolution)
    pub sequence_number: i64,
    /// Timestamp in milliseconds
    pub timestamp_ms: i64,
    /// Operation type (append, overwrite, delete, etc.)
    pub operation: String,
    /// Parent snapshot ID if any
    pub parent_snapshot_id: Option<i64>,
    /// Manifest list location
    pub manifest_list: String,
    /// Summary statistics
    pub summary: SnapshotSummary,
}

/// Snapshot summary statistics
#[derive(Debug, Clone, Default, Serialize)]
pub struct SnapshotSummary {
    /// Total records in the table after this snapshot
    pub total_records: Option<i64>,
    /// Total data files
    pub total_data_files: Option<i64>,
    /// Total delete files (position + equality)
    pub total_delete_files: Option<i64>,
    /// Total file size in bytes
    pub total_files_size: Option<i64>,
    /// Records added in this snapshot
    pub added_records: Option<i64>,
    /// Data files added
    pub added_data_files: Option<i64>,
    /// Delete files added
    pub added_delete_files: Option<i64>,
    /// Records deleted
    pub deleted_records: Option<i64>,
    /// Data files removed
    pub deleted_data_files: Option<i64>,
}

/// Current table state (from latest snapshot)
#[derive(Debug, Clone, Default, Serialize)]
pub struct CurrentState {
    /// Total records in the table
    pub total_records: Option<i64>,
    /// Number of data files
    pub total_data_files: Option<i64>,
    /// Number of delete files (indicates technical debt)
    pub total_delete_files: Option<i64>,
    /// Total size in bytes
    pub total_files_size: Option<i64>,
}

/// Table reference (branch or tag)
#[derive(Debug, Clone, Serialize)]
pub struct RefInfo {
    /// Reference name (e.g., "main", "audit-2024")
    pub name: String,
    /// Type: "branch" or "tag"
    pub ref_type: String,
    /// Snapshot ID this ref points to
    pub snapshot_id: i64,
}

/// Metadata log entry
#[derive(Debug, Clone, Serialize)]
pub struct MetadataLogEntry {
    /// Timestamp when this metadata was created
    pub timestamp_ms: i64,
    /// Location of the metadata file
    pub metadata_file: String,
}

/// Result of Iceberg table inspection
#[derive(Debug, Clone, Serialize)]
pub struct IcebergInspectResult {
    // ═══════════════════════════════════════════════════════════════════════════
    // TABLE INFORMATION
    // ═══════════════════════════════════════════════════════════════════════════
    /// Iceberg format version (1 or 2)
    pub format_version: i32,
    /// Table location URI
    pub location: String,
    /// Table UUID
    pub table_uuid: String,
    /// Current snapshot ID
    pub current_snapshot_id: Option<i64>,
    /// Total number of snapshots
    pub snapshot_count: usize,
    /// Last updated timestamp in milliseconds
    pub last_updated_ms: i64,
    /// Last sequence number (for conflict resolution)
    pub last_sequence_number: i64,

    // ═══════════════════════════════════════════════════════════════════════════
    // CURRENT STATE
    // ═══════════════════════════════════════════════════════════════════════════
    /// Current table state (records, files, size)
    pub current_state: CurrentState,

    // ═══════════════════════════════════════════════════════════════════════════
    // SCHEMA
    // ═══════════════════════════════════════════════════════════════════════════
    /// Current schema ID
    pub schema_id: i32,
    /// Schema fields
    pub fields: Vec<FieldInfo>,
    /// Identifier field IDs (primary key equivalent)
    pub identifier_field_ids: Vec<i32>,

    // ═══════════════════════════════════════════════════════════════════════════
    // PARTITION SPEC
    // ═══════════════════════════════════════════════════════════════════════════
    /// Default partition spec ID
    pub partition_spec_id: i32,
    /// Partition fields
    pub partition_fields: Vec<PartitionFieldInfo>,

    // ═══════════════════════════════════════════════════════════════════════════
    // SORT ORDER
    // ═══════════════════════════════════════════════════════════════════════════
    /// Default sort order ID
    pub sort_order_id: i64,
    /// Sort fields
    pub sort_fields: Vec<SortFieldInfo>,

    // ═══════════════════════════════════════════════════════════════════════════
    // PROPERTIES
    // ═══════════════════════════════════════════════════════════════════════════
    /// Table properties
    pub properties: std::collections::HashMap<String, String>,

    // ═══════════════════════════════════════════════════════════════════════════
    // REFS (branches and tags)
    // ═══════════════════════════════════════════════════════════════════════════
    /// Table references (branches and tags)
    pub refs: Vec<RefInfo>,

    // ═══════════════════════════════════════════════════════════════════════════
    // METADATA
    // ═══════════════════════════════════════════════════════════════════════════
    /// Number of schema versions
    pub schemas_count: usize,
    /// Number of partition spec versions
    pub partition_specs_count: usize,
    /// Number of sort order versions
    pub sort_orders_count: usize,
    /// Metadata log entries (for verbose mode)
    pub metadata_log: Vec<MetadataLogEntry>,

    // ═══════════════════════════════════════════════════════════════════════════
    // SNAPSHOTS (for verbose mode)
    // ═══════════════════════════════════════════════════════════════════════════
    /// Snapshot history
    pub snapshots: Vec<SnapshotInfo>,
}

/// Inspector for Iceberg tables using native iceberg-rs API
pub struct IcebergTableInspector;

impl IcebergTableInspector {
    /// Inspect an Iceberg table and return structured result
    pub fn inspect(
        table: &iceberg::table::Table,
        options: &IcebergInspectOptions,
    ) -> Result<IcebergInspectResult> {
        use crate::core::TableExt;
        use std::collections::HashSet;

        let (metadata, version) = table.metadata_with_version();

        // ═══════════════════════════════════════════════════════════════════════
        // TABLE INFORMATION
        // ═══════════════════════════════════════════════════════════════════════
        let format_version = version as i32;
        let location = metadata.location().to_string();
        let table_uuid = metadata.uuid().to_string();
        let current_snapshot_id = metadata.current_snapshot_id();
        let snapshot_count = metadata.snapshots().count();
        let last_updated_ms = metadata.last_updated_ms();
        let last_sequence_number = metadata.last_sequence_number();

        // ═══════════════════════════════════════════════════════════════════════
        // CURRENT STATE (from current snapshot)
        // ═══════════════════════════════════════════════════════════════════════
        let current_state = current_snapshot_id
            .and_then(|id| metadata.snapshot_by_id(id))
            .map(|snapshot| {
                let props = &snapshot.summary().additional_properties;
                CurrentState {
                    total_records: props.get("total-records").and_then(|v| v.parse().ok()),
                    total_data_files: props.get("total-data-files").and_then(|v| v.parse().ok()),
                    total_delete_files: props
                        .get("total-delete-files")
                        .and_then(|v| v.parse().ok()),
                    total_files_size: props.get("total-files-size").and_then(|v| v.parse().ok()),
                }
            })
            .unwrap_or_default();

        // ═══════════════════════════════════════════════════════════════════════
        // SCHEMA
        // ═══════════════════════════════════════════════════════════════════════
        let schema = metadata.current_schema();
        let schema_id = schema.schema_id();

        // Get identifier field IDs
        let identifier_field_ids: Vec<i32> = schema.identifier_field_ids().collect();
        let identifier_set: HashSet<i32> = identifier_field_ids.iter().copied().collect();

        let fields: Vec<FieldInfo> = schema
            .as_struct()
            .fields()
            .iter()
            .map(|f| FieldInfo {
                field_id: f.id,
                name: f.name.clone(),
                field_type: format_iceberg_type(&f.field_type),
                required: f.required,
                doc: f.doc.clone(),
                is_identifier: identifier_set.contains(&f.id),
            })
            .collect();

        // ═══════════════════════════════════════════════════════════════════════
        // PARTITION SPEC
        // ═══════════════════════════════════════════════════════════════════════
        let partition_spec = metadata.default_partition_spec();
        let partition_spec_id = partition_spec.spec_id();
        let partition_fields: Vec<PartitionFieldInfo> = partition_spec
            .fields()
            .iter()
            .map(|f| PartitionFieldInfo {
                name: f.name.clone(),
                transform: format!("{:?}", f.transform),
                source_id: f.source_id,
                field_id: f.field_id,
            })
            .collect();

        // ═══════════════════════════════════════════════════════════════════════
        // SORT ORDER
        // ═══════════════════════════════════════════════════════════════════════
        let sort_order_id = metadata.default_sort_order_id();
        let sort_fields: Vec<SortFieldInfo> = metadata
            .sort_orders_iter()
            .find(|so| so.order_id == sort_order_id)
            .map(|so| {
                so.fields
                    .iter()
                    .map(|sf| SortFieldInfo {
                        source_id: sf.source_id,
                        direction: format!("{:?}", sf.direction),
                        null_order: format!("{:?}", sf.null_order),
                    })
                    .collect()
            })
            .unwrap_or_default();

        // ═══════════════════════════════════════════════════════════════════════
        // PROPERTIES
        // ═══════════════════════════════════════════════════════════════════════
        let properties: std::collections::HashMap<String, String> = metadata
            .properties()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        // ═══════════════════════════════════════════════════════════════════════
        // REFS (branches and tags) - extracted from metadata
        // Note: We iterate known ref names since refs field is pub(crate)
        // ═══════════════════════════════════════════════════════════════════════
        let mut refs = Vec::new();

        // Always check for main branch
        if let Some(snapshot) = metadata.snapshot_for_ref("main") {
            refs.push(RefInfo {
                name: "main".to_string(),
                ref_type: "branch".to_string(),
                snapshot_id: snapshot.snapshot_id(),
            });
        }

        // ═══════════════════════════════════════════════════════════════════════
        // METADATA COUNTS
        // ═══════════════════════════════════════════════════════════════════════
        let schemas_count = metadata.schemas_iter().count();
        let partition_specs_count = metadata.partition_specs_iter().count();
        let sort_orders_count = metadata.sort_orders_iter().count();

        // ═══════════════════════════════════════════════════════════════════════
        // METADATA LOG (for verbose mode)
        // ═══════════════════════════════════════════════════════════════════════
        let metadata_log: Vec<MetadataLogEntry> = if options.verbose {
            metadata
                .metadata_log()
                .iter()
                .map(|entry| MetadataLogEntry {
                    timestamp_ms: entry.timestamp_ms,
                    metadata_file: entry.metadata_file.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };

        // ═══════════════════════════════════════════════════════════════════════
        // SNAPSHOTS (for verbose mode)
        // ═══════════════════════════════════════════════════════════════════════
        let snapshots: Vec<SnapshotInfo> = if options.verbose {
            metadata
                .snapshots()
                .map(|s| {
                    let summary = s.summary();
                    let props = &summary.additional_properties;

                    SnapshotInfo {
                        snapshot_id: s.snapshot_id(),
                        sequence_number: s.sequence_number(),
                        timestamp_ms: s.timestamp_ms(),
                        operation: format!("{:?}", summary.operation),
                        parent_snapshot_id: s.parent_snapshot_id(),
                        manifest_list: s.manifest_list().to_string(),
                        summary: SnapshotSummary {
                            total_records: props.get("total-records").and_then(|v| v.parse().ok()),
                            total_data_files: props
                                .get("total-data-files")
                                .and_then(|v| v.parse().ok()),
                            total_delete_files: props
                                .get("total-delete-files")
                                .and_then(|v| v.parse().ok()),
                            total_files_size: props
                                .get("total-files-size")
                                .and_then(|v| v.parse().ok()),
                            added_records: props.get("added-records").and_then(|v| v.parse().ok()),
                            added_data_files: props
                                .get("added-data-files")
                                .and_then(|v| v.parse().ok()),
                            added_delete_files: props
                                .get("added-delete-files")
                                .and_then(|v| v.parse().ok()),
                            deleted_records: props
                                .get("deleted-records")
                                .and_then(|v| v.parse().ok()),
                            deleted_data_files: props
                                .get("deleted-data-files")
                                .and_then(|v| v.parse().ok()),
                        },
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        Ok(IcebergInspectResult {
            format_version,
            location,
            table_uuid,
            current_snapshot_id,
            snapshot_count,
            last_updated_ms,
            last_sequence_number,
            current_state,
            schema_id,
            fields,
            identifier_field_ids,
            partition_spec_id,
            partition_fields,
            sort_order_id,
            sort_fields,
            properties,
            refs,
            schemas_count,
            partition_specs_count,
            sort_orders_count,
            metadata_log,
            snapshots,
        })
    }
}

/// Format an Iceberg type for display
fn format_iceberg_type(field_type: &iceberg::spec::Type) -> String {
    match field_type {
        iceberg::spec::Type::Primitive(p) => format!("{:?}", p),
        iceberg::spec::Type::Struct(s) => format!("struct<{} fields>", s.fields().len()),
        iceberg::spec::Type::List(l) => {
            format!(
                "list<{}>",
                format_iceberg_type(l.element_field.field_type.as_ref())
            )
        }
        iceberg::spec::Type::Map(m) => format!(
            "map<{}, {}>",
            format_iceberg_type(m.key_field.field_type.as_ref()),
            format_iceberg_type(m.value_field.field_type.as_ref())
        ),
    }
}
