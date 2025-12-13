//! Init service for creating new Iceberg tables
//!
//! Provides functionality to create new Iceberg tables with:
//! - Custom schema from JSON definition
//! - Partition specifications
//! - Table properties

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};

/// Configuration for table initialization
#[derive(Debug, Clone)]
pub struct InitConfig {
    /// Path where the table will be created
    pub path: String,
    /// Optional schema definition
    pub schema: Option<SchemaDefinition>,
    /// Columns to partition by
    pub partition_by: Option<Vec<String>>,
    /// Table properties
    pub properties: HashMap<String, String>,
}

/// Result of table initialization
#[derive(Debug, Clone)]
pub struct InitResult {
    /// UUID of the created table
    pub table_uuid: String,
    /// Path to the metadata file
    pub metadata_path: String,
    /// Location of the table
    pub location: String,
}

/// Schema definition for JSON input
#[derive(Debug, Clone, Deserialize)]
pub struct SchemaDefinition {
    /// List of column definitions
    pub columns: Vec<ColumnDefinition>,
}

/// Column definition in schema
#[derive(Debug, Clone, Deserialize)]
pub struct ColumnDefinition {
    /// Column name
    pub name: String,
    /// Data type (e.g., "string", "long", "integer", "double")
    #[serde(rename = "type")]
    pub data_type: String,
    /// Whether the column can contain null values (default: true)
    pub nullable: Option<bool>,
}

/// Service for initializing new Iceberg tables
pub struct InitService;

impl InitService {
    /// Create a new Iceberg table
    pub async fn create_table(config: InitConfig) -> Result<InitResult> {
        use std::fs;

        let table_path = Path::new(&config.path);

        // Create table directory structure
        fs::create_dir_all(table_path).map_err(|e| Error::Storage {
            message: format!(
                "Failed to create table directory '{}': {}",
                table_path.display(),
                e
            ),
        })?;

        let metadata_dir = table_path.join("metadata");
        fs::create_dir_all(&metadata_dir).map_err(|e| Error::Storage {
            message: format!("Failed to create metadata directory: {}", e),
        })?;

        let data_dir = table_path.join("data");
        fs::create_dir_all(&data_dir).map_err(|e| Error::Storage {
            message: format!("Failed to create data directory: {}", e),
        })?;

        // Build Iceberg schema
        let iceberg_schema = if let Some(schema_def) = &config.schema {
            Self::build_iceberg_schema(schema_def)?
        } else {
            Self::build_default_schema()?
        };

        // Build partition spec
        let partition_spec = Self::build_partition_spec(&iceberg_schema, &config.partition_by)?;

        // Create sort order (empty by default)
        let sort_order = iceberg::spec::SortOrder::unsorted_order();

        // Get canonical path for location
        let canonical_path = table_path
            .canonicalize()
            .unwrap_or(table_path.to_path_buf());
        let location = canonical_path.to_string_lossy().to_string();

        // Build table metadata
        let build_result = iceberg::spec::TableMetadataBuilder::new(
            iceberg_schema,
            partition_spec,
            sort_order,
            location.clone(),
            iceberg::spec::FormatVersion::V2,
            config.properties,
        )
        .map_err(|e| Error::Metadata {
            message: format!("Failed to create metadata builder: {}", e),
        })?
        .build()
        .map_err(|e| Error::Metadata {
            message: format!("Failed to build table metadata: {}", e),
        })?;

        let metadata = build_result.metadata;
        let table_uuid = metadata.uuid().to_string();

        // Serialize metadata to JSON
        let metadata_json =
            serde_json::to_string_pretty(&metadata).map_err(|e| Error::Serialization {
                message: format!("Failed to serialize metadata: {}", e),
            })?;

        // Write metadata file with standard Iceberg naming
        use crate::utils::core::{metadata_location_filename, new_metadata_location};
        let initial_location = new_metadata_location(&location);
        let metadata_filename = metadata_location_filename(&initial_location);
        let metadata_file = metadata_dir.join(&metadata_filename);
        fs::write(&metadata_file, metadata_json).map_err(|e| Error::Storage {
            message: format!("Failed to write metadata file: {}", e),
        })?;

        Ok(InitResult {
            table_uuid,
            metadata_path: metadata_file.to_string_lossy().to_string(),
            location,
        })
    }

    /// Load schema definition from a JSON file
    pub fn load_schema_from_file(path: &Path) -> Result<SchemaDefinition> {
        let content = std::fs::read_to_string(path).map_err(|e| Error::Storage {
            message: format!("Failed to read schema file '{}': {}", path.display(), e),
        })?;

        serde_json::from_str(&content).map_err(|e| Error::InvalidFormat {
            message: format!("Invalid schema file '{}': {}", path.display(), e),
        })
    }

    /// Parse properties from key=value strings
    pub fn parse_properties(props: &Option<Vec<String>>) -> HashMap<String, String> {
        let mut map = HashMap::new();
        if let Some(properties) = props {
            for prop in properties {
                if let Some((key, value)) = prop.split_once('=') {
                    map.insert(key.trim().to_string(), value.trim().to_string());
                }
            }
        }
        map
    }

    fn build_default_schema() -> Result<iceberg::spec::Schema> {
        iceberg::spec::Schema::builder()
            .with_fields(vec![
                iceberg::spec::NestedField::required(
                    1,
                    "id",
                    iceberg::spec::Type::Primitive(iceberg::spec::PrimitiveType::Long),
                )
                .into(),
            ])
            .build()
            .map_err(|e| Error::Metadata {
                message: format!("Failed to build default schema: {}", e),
            })
    }

    fn build_iceberg_schema(schema_def: &SchemaDefinition) -> Result<iceberg::spec::Schema> {
        let mut fields = Vec::new();

        for (idx, col) in schema_def.columns.iter().enumerate() {
            let field_id = (idx + 1) as i32;
            let iceberg_type = Self::parse_iceberg_type(&col.data_type)?;
            let nullable = col.nullable.unwrap_or(true);

            let field = if nullable {
                iceberg::spec::NestedField::optional(field_id, &col.name, iceberg_type)
            } else {
                iceberg::spec::NestedField::required(field_id, &col.name, iceberg_type)
            };

            fields.push(field.into());
        }

        iceberg::spec::Schema::builder()
            .with_fields(fields)
            .build()
            .map_err(|e| Error::Metadata {
                message: format!("Failed to build Iceberg schema: {}", e),
            })
    }

    fn build_partition_spec(
        schema: &iceberg::spec::Schema,
        partition_by: &Option<Vec<String>>,
    ) -> Result<iceberg::spec::PartitionSpec> {
        let Some(partition_cols) = partition_by else {
            return Ok(iceberg::spec::PartitionSpec::unpartition_spec());
        };

        let mut unbound_fields = Vec::new();
        for (idx, col) in partition_cols.iter().enumerate() {
            let field_id = schema
                .as_struct()
                .fields()
                .iter()
                .find(|f| f.name == *col)
                .map(|f| f.id)
                .ok_or_else(|| Error::ColumnNotFound {
                    column: col.clone(),
                })?;

            unbound_fields.push(
                iceberg::spec::UnboundPartitionField::builder()
                    .source_id(field_id)
                    .field_id(1000 + idx as i32)
                    .name(col.clone())
                    .transform(iceberg::spec::Transform::Identity)
                    .build(),
            );
        }

        iceberg::spec::UnboundPartitionSpec::builder()
            .with_spec_id(0)
            .add_partition_fields(unbound_fields)
            .map_err(|e| Error::Metadata {
                message: format!("Failed to add partition fields: {}", e),
            })?
            .build()
            .bind(schema.clone())
            .map_err(|e| Error::Metadata {
                message: format!("Failed to build partition spec: {}", e),
            })
    }

    fn parse_iceberg_type(type_str: &str) -> Result<iceberg::spec::Type> {
        use iceberg::spec::{PrimitiveType, Type};

        let prim = match type_str.to_lowercase().as_str() {
            "string" | "utf8" | "varchar" | "text" => PrimitiveType::String,
            "long" | "int64" | "bigint" => PrimitiveType::Long,
            "integer" | "int32" | "int" => PrimitiveType::Int,
            "float" | "float32" => PrimitiveType::Float,
            "double" | "float64" => PrimitiveType::Double,
            "boolean" | "bool" => PrimitiveType::Boolean,
            "binary" | "bytes" => PrimitiveType::Binary,
            "date" | "date32" => PrimitiveType::Date,
            "timestamp" | "datetime" => PrimitiveType::Timestamp,
            "timestamptz" | "timestamp_tz" => PrimitiveType::Timestamptz,
            "time" => PrimitiveType::Time,
            "uuid" => PrimitiveType::Uuid,
            _ => {
                return Err(Error::InvalidFormat {
                    message: format!(
                        "Invalid Iceberg type: '{}'. Supported: string, long, integer, float, double, boolean, binary, date, timestamp, timestamptz, time, uuid",
                        type_str
                    ),
                });
            }
        };

        Ok(Type::Primitive(prim))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_properties() {
        let props = Some(vec!["key1=value1".to_string(), "key2=value2".to_string()]);
        let map = InitService::parse_properties(&props);
        assert_eq!(map.get("key1"), Some(&"value1".to_string()));
        assert_eq!(map.get("key2"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_parse_properties_empty() {
        let map = InitService::parse_properties(&None);
        assert!(map.is_empty());
    }
}
