//! Structural diff operation - compare table metadata and schema
//!
//! This module implements structural comparison by:
//! 1. Comparing metadata (rows, sizes, compression, versions, custom metadata)
//! 2. Comparing schemas (columns added/removed/modified with nullability)
//! 3. Optionally comparing column statistics (in verbose mode)
//!
//! Unlike row-level diff, this is fast as it only reads metadata without scanning data.

use std::collections::HashMap;
use std::sync::Arc;

use datafusion::arrow::datatypes::DataType;

use crate::core::formats::{ColumnStats, FileMetadata, FormatHandler};
use crate::error::Result;

/// Operation for comparing two tables structurally
pub struct DiffOperation {
    left_handler: Arc<dyn FormatHandler>,
    right_handler: Arc<dyn FormatHandler>,
}

impl DiffOperation {
    /// Create a new diff operation
    pub fn new(
        left_handler: Arc<dyn FormatHandler>,
        right_handler: Arc<dyn FormatHandler>,
    ) -> Self {
        Self {
            left_handler,
            right_handler,
        }
    }

    /// Execute structural diff operation
    pub async fn execute(
        &self,
        options: &DiffOptions,
        left_path: String,
        right_path: String,
    ) -> Result<DiffResult> {
        // Step 1: Always compare metadata
        let left_meta = self.left_handler.read_metadata().await?;
        let right_meta = self.right_handler.read_metadata().await?;
        let metadata_diff = Self::compute_metadata_diff(&left_meta, &right_meta);

        // Step 2: Always compare schemas
        let left_schema = self.left_handler.read_schema().await?;
        let right_schema = self.right_handler.read_schema().await?;
        let schema_diff = Self::compute_schema_diff(&left_schema, &right_schema);

        // Step 3: Optionally compare column statistics (verbose mode)
        let column_stats_diff = if options.verbose {
            let left_stats = self.left_handler.read_statistics().await?;
            let right_stats = self.right_handler.read_statistics().await?;
            Self::compute_stats_diff(&left_stats, &right_stats)
        } else {
            Vec::new()
        };

        Ok(DiffResult {
            left_path,
            right_path,
            metadata_diff,
            schema_diff,
            column_stats_diff,
        })
    }

    /// Compute metadata differences
    fn compute_metadata_diff(left: &FileMetadata, right: &FileMetadata) -> MetadataDiff {
        // Compare num_rows
        let num_rows = match (left.num_rows, right.num_rows) {
            (Some(l), Some(r)) => Some((l, r)),
            _ => None,
        };

        // Compare compressed_size
        let compressed_size = match (left.compressed_size, right.compressed_size) {
            (Some(l), Some(r)) => Some((l, r)),
            _ => None,
        };

        // Compare uncompressed_size
        let uncompressed_size = match (left.uncompressed_size, right.uncompressed_size) {
            (Some(l), Some(r)) => Some((l, r)),
            _ => None,
        };

        // Compare compression
        let compression = match (&left.compression, &right.compression) {
            (Some(l), Some(r)) => Some((l.clone(), r.clone())),
            _ => None,
        };

        // Compare format_version
        let format_version = match (&left.format_version, &right.format_version) {
            (Some(l), Some(r)) => Some((l.clone(), r.clone())),
            _ => None,
        };

        // Compare custom metadata
        let custom_metadata = Self::compute_metadata_changes(&left.metadata, &right.metadata);

        MetadataDiff {
            num_rows,
            compressed_size,
            uncompressed_size,
            compression,
            format_version,
            custom_metadata,
        }
    }

    /// Compute changes in custom metadata
    fn compute_metadata_changes(
        left: &HashMap<String, String>,
        right: &HashMap<String, String>,
    ) -> MetadataChanges {
        let mut added = HashMap::new();
        let mut removed = HashMap::new();
        let mut modified = HashMap::new();

        // Find added and modified
        for (key, right_value) in right {
            match left.get(key) {
                Some(left_value) if left_value != right_value => {
                    modified.insert(key.clone(), (left_value.clone(), right_value.clone()));
                }
                None => {
                    added.insert(key.clone(), right_value.clone());
                }
                _ => {} // Unchanged
            }
        }

        // Find removed
        for (key, value) in left {
            if !right.contains_key(key) {
                removed.insert(key.clone(), value.clone());
            }
        }

        MetadataChanges {
            added,
            removed,
            modified,
        }
    }

    /// Compute schema differences
    fn compute_schema_diff(
        left: &datafusion::arrow::datatypes::Schema,
        right: &datafusion::arrow::datatypes::Schema,
    ) -> SchemaDiff {
        let mut columns_added = Vec::new();
        let mut columns_removed = Vec::new();
        let mut columns_modified = Vec::new();
        let mut columns_unchanged = Vec::new();

        // Build maps for easier lookup
        let left_fields: HashMap<_, _> = left
            .fields()
            .iter()
            .map(|f| (f.name().clone(), (f.data_type().clone(), f.is_nullable())))
            .collect();

        let right_fields: HashMap<_, _> = right
            .fields()
            .iter()
            .map(|f| (f.name().clone(), (f.data_type().clone(), f.is_nullable())))
            .collect();

        // Find added columns (in right but not in left)
        for (name, (data_type, nullable)) in &right_fields {
            if !left_fields.contains_key(name) {
                columns_added.push(ColumnInfo {
                    name: name.clone(),
                    data_type: format_data_type(data_type),
                    nullable: *nullable,
                });
            }
        }

        // Find removed columns (in left but not in right)
        for (name, (data_type, nullable)) in &left_fields {
            if !right_fields.contains_key(name) {
                columns_removed.push(ColumnInfo {
                    name: name.clone(),
                    data_type: format_data_type(data_type),
                    nullable: *nullable,
                });
            }
        }

        // Find modified columns (same name, different type or nullability)
        for (name, (left_type, left_nullable)) in &left_fields {
            if let Some((right_type, right_nullable)) = right_fields.get(name) {
                let type_change = if left_type != right_type {
                    Some((format_data_type(left_type), format_data_type(right_type)))
                } else {
                    None
                };

                let nullability_change = if left_nullable != right_nullable {
                    Some((*left_nullable, *right_nullable))
                } else {
                    None
                };

                if type_change.is_some() || nullability_change.is_some() {
                    columns_modified.push(ColumnChange {
                        name: name.clone(),
                        type_change,
                        nullability_change,
                    });
                } else {
                    columns_unchanged.push(name.clone());
                }
            }
        }

        SchemaDiff {
            columns_added,
            columns_removed,
            columns_modified,
            columns_unchanged,
        }
    }

    /// Compute column statistics differences
    fn compute_stats_diff(left: &[ColumnStats], right: &[ColumnStats]) -> Vec<ColumnStatsDiff> {
        let mut diffs = Vec::new();

        // Build a map of right stats for easy lookup
        let right_map: HashMap<_, _> = right.iter().map(|s| (s.name.as_str(), s)).collect();

        // Compare stats for each column in left
        for left_stat in left {
            if let Some(right_stat) = right_map.get(left_stat.name.as_str()) {
                let null_count = match (left_stat.null_count, right_stat.null_count) {
                    (Some(l), Some(r)) => Some((l, r)),
                    _ => None,
                };

                let distinct_count_approx =
                    match (left_stat.distinct_count, right_stat.distinct_count) {
                        (Some(l), Some(r)) => Some((l, r)),
                        _ => None,
                    };

                let min_value = match (&left_stat.min_value, &right_stat.min_value) {
                    (Some(l), Some(r)) => Some((l.clone(), r.clone())),
                    _ => None,
                };

                let max_value = match (&left_stat.max_value, &right_stat.max_value) {
                    (Some(l), Some(r)) => Some((l.clone(), r.clone())),
                    _ => None,
                };

                let mean = match (left_stat.mean, right_stat.mean) {
                    (Some(l), Some(r)) => Some((l, r)),
                    _ => None,
                };

                diffs.push(ColumnStatsDiff {
                    name: left_stat.name.clone(),
                    null_count,
                    distinct_count_approx,
                    min_value,
                    max_value,
                    mean,
                });
            }
        }

        diffs
    }
}

/// Format DataType to readable string
fn format_data_type(dtype: &DataType) -> String {
    match dtype {
        DataType::Int8 => "int8".to_string(),
        DataType::Int16 => "int16".to_string(),
        DataType::Int32 => "int32".to_string(),
        DataType::Int64 => "int64".to_string(),
        DataType::UInt8 => "uint8".to_string(),
        DataType::UInt16 => "uint16".to_string(),
        DataType::UInt32 => "uint32".to_string(),
        DataType::UInt64 => "uint64".to_string(),
        DataType::Float16 => "float16".to_string(),
        DataType::Float32 => "float32".to_string(),
        DataType::Float64 => "float64".to_string(),
        DataType::Utf8 => "string".to_string(),
        DataType::LargeUtf8 => "large_string".to_string(),
        DataType::Binary => "binary".to_string(),
        DataType::LargeBinary => "large_binary".to_string(),
        DataType::Boolean => "bool".to_string(),
        DataType::Date32 => "date32".to_string(),
        DataType::Date64 => "date64".to_string(),
        DataType::Timestamp(unit, tz) => {
            let tz_str = tz.as_ref().map(|t| format!(" ({})", t)).unwrap_or_default();
            format!("timestamp({:?}){}", unit, tz_str)
        }
        DataType::List(field) => format!("list<{}>", format_data_type(field.data_type())),
        DataType::LargeList(field) => {
            format!("large_list<{}>", format_data_type(field.data_type()))
        }
        DataType::Struct(fields) => {
            let field_strs: Vec<_> = fields
                .iter()
                .map(|f| format!("{}: {}", f.name(), format_data_type(f.data_type())))
                .collect();
            format!("struct<{}>", field_strs.join(", "))
        }
        DataType::Decimal128(p, s) => format!("decimal({}, {})", p, s),
        _ => format!("{:?}", dtype),
    }
}

/// Options for diff operation
#[derive(Debug, Clone)]
pub struct DiffOptions {
    /// Show detailed column statistics
    pub verbose: bool,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self { verbose: false }
    }
}

/// Metadata differences between two tables
#[derive(Debug, Clone)]
pub struct MetadataDiff {
    /// Number of rows: (left_rows, right_rows)
    pub num_rows: Option<(i64, i64)>,

    /// Compressed size in bytes: (left_size, right_size)
    pub compressed_size: Option<(u64, u64)>,

    /// Uncompressed size in bytes: (left_size, right_size)
    pub uncompressed_size: Option<(u64, u64)>,

    /// Compression codec: (left_compression, right_compression)
    pub compression: Option<(String, String)>,

    /// Format version: (left_version, right_version)
    pub format_version: Option<(String, String)>,

    /// Custom metadata changes
    pub custom_metadata: MetadataChanges,
}

/// Changes in custom metadata
#[derive(Debug, Clone)]
pub struct MetadataChanges {
    /// Metadata keys added in right
    pub added: HashMap<String, String>,

    /// Metadata keys removed from left
    pub removed: HashMap<String, String>,

    /// Metadata keys modified: (old_value, new_value)
    pub modified: HashMap<String, (String, String)>,
}

impl MetadataChanges {
    /// Check if there are any changes
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
    }
}

/// Schema differences between two tables
#[derive(Debug, Clone)]
pub struct SchemaDiff {
    /// Columns present in right but not in left
    pub columns_added: Vec<ColumnInfo>,

    /// Columns present in left but not in right
    pub columns_removed: Vec<ColumnInfo>,

    /// Columns with type or nullability changes
    pub columns_modified: Vec<ColumnChange>,

    /// Columns that are identical in both schemas
    pub columns_unchanged: Vec<String>,
}

impl SchemaDiff {
    /// Check if schemas are identical
    pub fn is_identical(&self) -> bool {
        self.columns_added.is_empty()
            && self.columns_removed.is_empty()
            && self.columns_modified.is_empty()
    }
}

/// Information about a column
#[derive(Debug, Clone)]
pub struct ColumnInfo {
    /// Column name
    pub name: String,

    /// Data type as string
    pub data_type: String,

    /// Whether the column is nullable
    pub nullable: bool,
}

/// Information about a column that changed
#[derive(Debug, Clone)]
pub struct ColumnChange {
    /// Column name
    pub name: String,

    /// Type change: (old_type, new_type)
    pub type_change: Option<(String, String)>,

    /// Nullability change: (old_nullable, new_nullable)
    pub nullability_change: Option<(bool, bool)>,
}

/// Column statistics differences (for verbose mode)
#[derive(Debug, Clone)]
pub struct ColumnStatsDiff {
    /// Column name
    pub name: String,

    /// Null count: (left, right)
    pub null_count: Option<(i64, i64)>,

    /// Approximate distinct count: (left, right)
    pub distinct_count_approx: Option<(i64, i64)>,

    /// Minimum value: (left, right)
    pub min_value: Option<(String, String)>,

    /// Maximum value: (left, right)
    pub max_value: Option<(String, String)>,

    /// Mean value: (left, right)
    pub mean: Option<(f64, f64)>,
}

/// Complete result of a structural diff operation
#[derive(Debug)]
pub struct DiffResult {
    /// Path to left table
    pub left_path: String,

    /// Path to right table
    pub right_path: String,

    /// Metadata differences (always present)
    pub metadata_diff: MetadataDiff,

    /// Schema differences (always present)
    pub schema_diff: SchemaDiff,

    /// Column statistics differences (only if verbose = true)
    pub column_stats_diff: Vec<ColumnStatsDiff>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::arrow::datatypes::{Field, Schema as ArrowSchema};
    use std::sync::Arc;

    #[test]
    fn test_format_data_type() {
        assert_eq!(format_data_type(&DataType::Int32), "int32");
        assert_eq!(format_data_type(&DataType::Utf8), "string");
        assert_eq!(format_data_type(&DataType::Boolean), "bool");
    }

    #[test]
    fn test_schema_diff_identical() {
        let schema = Arc::new(ArrowSchema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, true),
        ]));

        let diff = DiffOperation::compute_schema_diff(&schema, &schema);

        assert!(diff.is_identical());
        assert!(diff.columns_added.is_empty());
        assert!(diff.columns_removed.is_empty());
        assert!(diff.columns_modified.is_empty());
        assert_eq!(diff.columns_unchanged.len(), 2);
    }

    #[test]
    fn test_schema_diff_added_column() {
        let left = Arc::new(ArrowSchema::new(vec![Field::new(
            "id",
            DataType::Int32,
            false,
        )]));

        let right = Arc::new(ArrowSchema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, true),
        ]));

        let diff = DiffOperation::compute_schema_diff(&left, &right);

        assert!(!diff.is_identical());
        assert_eq!(diff.columns_added.len(), 1);
        assert_eq!(diff.columns_added[0].name, "name");
        assert_eq!(diff.columns_added[0].data_type, "string");
        assert!(diff.columns_added[0].nullable);
        assert!(diff.columns_removed.is_empty());
        assert!(diff.columns_modified.is_empty());
    }

    #[test]
    fn test_schema_diff_removed_column() {
        let left = Arc::new(ArrowSchema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, true),
        ]));

        let right = Arc::new(ArrowSchema::new(vec![Field::new(
            "id",
            DataType::Int32,
            false,
        )]));

        let diff = DiffOperation::compute_schema_diff(&left, &right);

        assert!(!diff.is_identical());
        assert!(diff.columns_added.is_empty());
        assert_eq!(diff.columns_removed.len(), 1);
        assert_eq!(diff.columns_removed[0].name, "name");
        assert!(diff.columns_modified.is_empty());
    }

    #[test]
    fn test_schema_diff_type_change() {
        let left = Arc::new(ArrowSchema::new(vec![Field::new(
            "id",
            DataType::Int32,
            false,
        )]));

        let right = Arc::new(ArrowSchema::new(vec![Field::new(
            "id",
            DataType::Int64,
            false,
        )]));

        let diff = DiffOperation::compute_schema_diff(&left, &right);

        assert!(!diff.is_identical());
        assert!(diff.columns_added.is_empty());
        assert!(diff.columns_removed.is_empty());
        assert_eq!(diff.columns_modified.len(), 1);
        assert_eq!(diff.columns_modified[0].name, "id");
        assert_eq!(
            diff.columns_modified[0].type_change,
            Some(("int32".to_string(), "int64".to_string()))
        );
        assert_eq!(diff.columns_modified[0].nullability_change, None);
    }

    #[test]
    fn test_schema_diff_nullability_change() {
        let left = Arc::new(ArrowSchema::new(vec![Field::new(
            "name",
            DataType::Utf8,
            false,
        )]));

        let right = Arc::new(ArrowSchema::new(vec![Field::new(
            "name",
            DataType::Utf8,
            true,
        )]));

        let diff = DiffOperation::compute_schema_diff(&left, &right);

        assert!(!diff.is_identical());
        assert!(diff.columns_added.is_empty());
        assert!(diff.columns_removed.is_empty());
        assert_eq!(diff.columns_modified.len(), 1);
        assert_eq!(diff.columns_modified[0].name, "name");
        assert_eq!(diff.columns_modified[0].type_change, None);
        assert_eq!(
            diff.columns_modified[0].nullability_change,
            Some((false, true))
        );
    }

    #[test]
    fn test_metadata_changes_empty() {
        let left = HashMap::new();
        let right = HashMap::new();

        let changes = DiffOperation::compute_metadata_changes(&left, &right);

        assert!(changes.is_empty());
    }

    #[test]
    fn test_metadata_changes_added() {
        let left = HashMap::new();
        let mut right = HashMap::new();
        right.insert("key1".to_string(), "value1".to_string());

        let changes = DiffOperation::compute_metadata_changes(&left, &right);

        assert!(!changes.is_empty());
        assert_eq!(changes.added.len(), 1);
        assert_eq!(changes.added.get("key1"), Some(&"value1".to_string()));
        assert!(changes.removed.is_empty());
        assert!(changes.modified.is_empty());
    }

    #[test]
    fn test_metadata_changes_removed() {
        let mut left = HashMap::new();
        left.insert("key1".to_string(), "value1".to_string());
        let right = HashMap::new();

        let changes = DiffOperation::compute_metadata_changes(&left, &right);

        assert!(!changes.is_empty());
        assert!(changes.added.is_empty());
        assert_eq!(changes.removed.len(), 1);
        assert_eq!(changes.removed.get("key1"), Some(&"value1".to_string()));
        assert!(changes.modified.is_empty());
    }

    #[test]
    fn test_metadata_changes_modified() {
        let mut left = HashMap::new();
        left.insert("key1".to_string(), "value1".to_string());
        let mut right = HashMap::new();
        right.insert("key1".to_string(), "value2".to_string());

        let changes = DiffOperation::compute_metadata_changes(&left, &right);

        assert!(!changes.is_empty());
        assert!(changes.added.is_empty());
        assert!(changes.removed.is_empty());
        assert_eq!(changes.modified.len(), 1);
        assert_eq!(
            changes.modified.get("key1"),
            Some(&("value1".to_string(), "value2".to_string()))
        );
    }
}
