//! Metadata bridge for Apache Iceberg tables
//!
//! This module provides a bridge between the public API of the `iceberg` crate
//! and the raw JSON metadata needed to access private fields.
//!
//! # Problem
//!
//! The `iceberg` crate has many private fields in its public structs:
//! - `TableMetadata::table_uuid` is private
//! - `TableMetadata::properties` is private  
//! - `PartitionSpecField` fields are private
//! - etc.
//!
//! # Solution
//!
//! `MetadataBridge` provides:
//! 1. Access to public API methods from `iceberg::spec::TableMetadata`
//! 2. Fallback to raw JSON parsing for private fields
//! 3. Lazy loading of raw JSON (only when needed)
//!
//! # Design Principles
//!
//! 1. **Temporary**: Assume `iceberg` crate will eventually expose these fields
//! 2. **Minimal**: Only bridge what's absolutely necessary
//! 3. **Clean API**: Same method names as `iceberg` crate (when available)
//! 4. **Easy migration**: When `iceberg` exposes a field, switch to public API

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::Result;

/// Bridge between iceberg crate API and raw JSON metadata
pub struct MetadataBridge {
    /// Public metadata from iceberg crate
    iceberg_metadata: Arc<iceberg::spec::TableMetadata>,
    /// Raw JSON metadata (loaded lazily when needed)
    raw_json: Option<serde_json::Value>,
    /// Path to table (for lazy loading raw JSON)
    table_path: Option<String>,
}

impl MetadataBridge {
    /// Create a bridge from an iceberg table with table path
    pub fn from_table_with_path(table: &iceberg::table::Table, table_path: &str) -> Self {
        Self {
            iceberg_metadata: table.metadata().clone().into(),
            raw_json: None,
            table_path: Some(table_path.to_string()),
        }
    }

    /// Create a bridge from TableMetadata (for testing or direct use)
    pub fn from_metadata(metadata: Arc<iceberg::spec::TableMetadata>) -> Self {
        Self {
            iceberg_metadata: metadata,
            raw_json: None,
            table_path: None,
        }
    }

    /// Ensure raw JSON metadata is loaded
    pub async fn ensure_raw_metadata(&mut self) -> Result<()> {
        if self.raw_json.is_none() {
            if let Some(path) = &self.table_path {
                self.raw_json = Some(Self::load_raw_metadata(path).await?);
            }
        }
        Ok(())
    }

    /// Load raw JSON metadata from table path
    async fn load_raw_metadata(table_path: &str) -> Result<serde_json::Value> {
        use crate::core::TableLoader;
        
        // Find latest metadata file
        let metadata_path = TableLoader::find_latest_metadata(table_path).await?;
        
        // Read and parse JSON
        let content = tokio::fs::read_to_string(&metadata_path).await?;
        Ok(serde_json::from_str(&content)?)
    }

    // === Public API Methods (delegated to iceberg_metadata) ===

    /// Get format version
    pub fn format_version(&self) -> i32 {
        self.iceberg_metadata.format_version() as i32
    }

    /// Get current schema ID
    pub fn current_schema_id(&self) -> i32 {
        self.iceberg_metadata.current_schema_id()
    }

    /// Get default sort order ID
    pub fn default_sort_order_id(&self) -> i32 {
        self.iceberg_metadata.default_sort_order_id() as i32
    }

    /// Get current snapshot ID
    pub fn current_snapshot_id(&self) -> Option<i64> {
        self.iceberg_metadata.current_snapshot_id()
    }

    /// Get current snapshot
    pub fn current_snapshot(&self) -> Option<&iceberg::spec::Snapshot> {
        self.iceberg_metadata.current_snapshot().map(|v| &**v)
    }

    /// Get snapshots iterator
    pub fn snapshots(&self) -> impl Iterator<Item = &iceberg::spec::Snapshot> {
        self.iceberg_metadata.snapshots().map(|v| &**v)
    }

    /// Get current schema
    pub fn current_schema(&self) -> &iceberg::spec::Schema {
        &self.iceberg_metadata.current_schema()
    }

    /// Get default partition spec
    pub fn default_partition_spec(&self) -> &iceberg::spec::PartitionSpec {
        &self.iceberg_metadata.default_partition_spec()
    }

    /// Get schemas iterator
    pub fn schemas_iter(&self) -> impl Iterator<Item = &iceberg::spec::Schema> {
        self.iceberg_metadata.schemas_iter().map(|v| &**v)
    }

    /// Get partition specs iterator
    pub fn partition_specs_iter(&self) -> impl Iterator<Item = &iceberg::spec::PartitionSpec> {
        self.iceberg_metadata.partition_specs_iter().map(|v| &**v)
    }

    /// Get sort orders iterator
    pub fn sort_orders_iter(&self) -> impl Iterator<Item = &iceberg::spec::SortOrder> {
        self.iceberg_metadata.sort_orders_iter().map(|v| &**v)
    }

    // === Private Field Accessors (via raw JSON) ===

    /// Get table UUID (private in iceberg crate)
    pub fn table_uuid(&self) -> Result<&str> {
        self.get_raw_string("table-uuid")
            .ok_or_else(|| crate::error::Error::General("Table UUID not found in metadata".to_string()))
    }

    /// Get table location (private in iceberg crate)
    pub fn location(&self) -> Result<&str> {
        self.get_raw_string("location")
            .ok_or_else(|| crate::error::Error::General("Location not found in metadata".to_string()))
    }

    /// Get table properties (private in iceberg crate)
    pub fn properties(&self) -> HashMap<String, String> {
        self.raw_json.as_ref()
            .and_then(|v| v.get("properties"))
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get a specific property value
    pub fn property(&self, key: &str) -> Option<String> {
        self.raw_json.as_ref()
            .and_then(|v| v.get("properties"))
            .and_then(|v| v.as_object())
            .and_then(|props| props.get(key))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Get created-by property
    pub fn created_by(&self) -> Option<String> {
        self.property("created-by")
    }

    /// Get last updated timestamp in milliseconds
    pub fn last_updated_ms(&self) -> Option<i64> {
        self.raw_json.as_ref()
            .and_then(|v| v.get("last-updated-ms"))
            .and_then(|v| v.as_i64())
    }

    /// Get last column ID
    pub fn last_column_id(&self) -> Option<i32> {
        self.raw_json.as_ref()
            .and_then(|v| v.get("last-column-id"))
            .and_then(|v| v.as_i64())
            .map(|v| v as i32)
    }

    /// Get partition spec fields with details (fields are private in iceberg crate)
    pub fn partition_spec_fields(&self) -> Vec<PartitionFieldInfo> {
        self.raw_json.as_ref()
            .and_then(|v| v.get("partition-specs"))
            .and_then(|v| v.as_array())
            .and_then(|specs| specs.first()) // Get default spec
            .and_then(|spec| spec.get("fields"))
            .and_then(|v| v.as_array())
            .map(|fields| {
                fields.iter()
                    .filter_map(|f| {
                        let source_id = f.get("source-id").and_then(|v| v.as_i64()).map(|v| v as i32)?;
                        let field_id = f.get("field-id").and_then(|v| v.as_i64()).map(|v| v as i32)?;
                        let name = f.get("name").and_then(|v| v.as_str()).map(|s| s.to_string())?;
                        let transform = f.get("transform").and_then(|v| v.as_str()).map(|s| s.to_string())?;
                        
                        Some(PartitionFieldInfo {
                            source_id,
                            field_id,
                            name,
                            transform,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    // === Helper Methods ===

    /// Get a string value from raw JSON
    fn get_raw_string(&self, key: &str) -> Option<&str> {
        self.raw_json.as_ref()
            .and_then(|v| v.get(key))
            .and_then(|v| v.as_str())
    }

    /// Get the raw JSON metadata (for advanced use cases)
    pub fn raw_json(&self) -> Option<&serde_json::Value> {
        self.raw_json.as_ref()
    }

    /// Check if raw JSON is loaded
    pub fn has_raw_json(&self) -> bool {
        self.raw_json.is_some()
    }
}

/// Information about a partition field (since iceberg::spec::PartitionSpecField is private)
#[derive(Debug, Clone)]
pub struct PartitionFieldInfo {
    pub source_id: i32,
    pub field_id: i32,
    pub name: String,
    pub transform: String,
}

impl PartitionFieldInfo {
    /// Format as "name: transform"
    pub fn display(&self) -> String {
        format!("{}: {}", self.name, self.transform)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_metadata() -> Arc<iceberg::spec::TableMetadata> {
        use iceberg::spec::{TableMetadataBuilder, Schema, SchemaV2, StructTypeBuilder, Type};
        
        let schema = Schema::V2(SchemaV2 {
            schema_id: 0,
            identifier_field_ids: vec![],
            fields: StructTypeBuilder::new()
                .with_field(1, "id", Type::Long, false)
                .build()
                .unwrap(),
        });
        
        let partition_spec = iceberg::spec::PartitionSpecBuilder::new()
            .with_spec_id(0)
            .build();
        
        TableMetadataBuilder::new(schema, partition_spec)
            .build()
            .unwrap()
            .into()
    }

    #[test]
    fn test_metadata_bridge_public_api() {
        let metadata = create_test_metadata();
        let bridge = MetadataBridge::from_metadata(metadata);
        
        assert_eq!(bridge.format_version(), 2);
        assert_eq!(bridge.current_schema_id(), 0);
        assert_eq!(bridge.default_sort_order_id(), 0);
    }

    #[test]
    fn test_metadata_bridge_raw_json_access() {
        let metadata = create_test_metadata();
        let raw_json = json!({
            "table-uuid": "test-uuid-123",
            "location": "s3://bucket/table",
            "properties": {
                "created-by": "icetable-test",
                "owner": "data-team"
            },
            "last-updated-ms": 1234567890000,
            "last-column-id": 100,
            "partition-specs": [{
                "spec-id": 0,
                "fields": [{
                    "source-id": 1,
                    "field-id": 1000,
                    "name": "date",
                    "transform": "day"
                }]
            }]
        });
        
        let bridge = MetadataBridge {
            iceberg_metadata: metadata,
            raw_json: Some(raw_json),
            table_path: None,
        };
        
        assert_eq!(bridge.table_uuid().unwrap(), "test-uuid-123");
        assert_eq!(bridge.location().unwrap(), "s3://bucket/table");
        assert_eq!(bridge.created_by(), Some("icetable-test".to_string()));
        assert_eq!(bridge.property("owner"), Some("data-team".to_string()));
        assert_eq!(bridge.last_updated_ms(), Some(1234567890000));
        assert_eq!(bridge.last_column_id(), Some(100));
        
        let partition_fields = bridge.partition_spec_fields();
        assert_eq!(partition_fields.len(), 1);
        assert_eq!(partition_fields[0].name, "date");
        assert_eq!(partition_fields[0].transform, "day");
        assert_eq!(partition_fields[0].display(), "date: day");
    }

    #[test]
    fn test_metadata_bridge_missing_raw_json() {
        let metadata = create_test_metadata();
        let bridge = MetadataBridge::from_metadata(metadata);
        
        // Should return default/empty values when raw JSON not loaded
        assert!(bridge.table_uuid().is_err());
        assert!(bridge.location().is_err());
        assert!(bridge.properties().is_empty());
        assert!(bridge.created_by().is_none());
        assert!(bridge.partition_spec_fields().is_empty());
    }
}