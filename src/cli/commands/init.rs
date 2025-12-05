//! Init command implementation
//!
//! This command creates new Iceberg tables.

use std::collections::HashMap;
use std::path::Path;

use colored::Colorize;

use crate::cli::parser::InitArgs;
use crate::error::{Error, Result};

/// Handler for init command
pub struct InitCommand;

impl InitCommand {
    /// Execute init command
    pub async fn execute(args: InitArgs) -> Result<()> {
        // Only Iceberg is supported
        if args.format != "iceberg" {
            return Err(Error::UnsupportedFeature {
                feature: format!(
                    "Only Iceberg tables are supported. Use 'icetable init iceberg {}' instead.",
                    args.path
                ),
            });
        }

        // Parse schema if provided
        let schema = if let Some(schema_path) = &args.schema {
            Some(Self::load_schema(schema_path)?)
        } else {
            None
        };

        // Parse properties
        let properties = Self::parse_properties(&args.properties);

        Self::create_iceberg_table(&args, schema, properties).await
    }

    /// Load schema from JSON file
    fn load_schema(path: &Path) -> Result<SchemaDefinition> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            Error::General(format!(
                "Failed to read schema file '{}': {}",
                path.display(),
                e
            ))
        })?;

        let schema: SchemaDefinition = serde_json::from_str(&content).map_err(|e| {
            Error::General(format!(
                "Failed to parse schema file '{}': {}",
                path.display(),
                e
            ))
        })?;

        Ok(schema)
    }

    /// Parse properties from CLI arguments
    fn parse_properties(props: &Option<Vec<String>>) -> HashMap<String, String> {
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

    async fn create_iceberg_table(
        args: &InitArgs,
        schema: Option<SchemaDefinition>,
        _properties: HashMap<String, String>,
    ) -> Result<()> {
        use std::fs;

        let table_path = Path::new(&args.path);

        // Create table directory structure
        fs::create_dir_all(table_path).map_err(|e| {
            Error::General(format!(
                "Failed to create table directory '{}': {}",
                table_path.display(),
                e
            ))
        })?;

        let metadata_dir = table_path.join("metadata");
        fs::create_dir_all(&metadata_dir)
            .map_err(|e| Error::General(format!("Failed to create metadata directory: {}", e)))?;

        let data_dir = table_path.join("data");
        fs::create_dir_all(&data_dir)
            .map_err(|e| Error::General(format!("Failed to create data directory: {}", e)))?;

        // Build Iceberg schema
        let iceberg_schema = if let Some(schema_def) = schema {
            Self::build_iceberg_schema(&schema_def)?
        } else {
            // Default schema
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
                .map_err(|e| Error::General(format!("Failed to build default schema: {}", e)))?
        };

        // Build partition spec (empty by default, or from args)
        let partition_spec = if let Some(partition_cols) = &args.partition_by {
            let mut unbound_fields = Vec::new();
            for (idx, col) in partition_cols.iter().enumerate() {
                // Find the field ID for this column in the schema
                let field_id = iceberg_schema
                    .as_struct()
                    .fields()
                    .iter()
                    .find(|f| f.name == *col)
                    .map(|f| f.id)
                    .ok_or_else(|| {
                        Error::General(format!("Partition column '{}' not found in schema", col))
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
                .map_err(|e| Error::General(format!("Failed to add partition fields: {}", e)))?
                .build()
                .bind(iceberg_schema.clone())
                .map_err(|e| Error::General(format!("Failed to build partition spec: {}", e)))?
        } else {
            iceberg::spec::PartitionSpec::unpartition_spec()
        };

        // Create sort order (empty by default)
        let sort_order = iceberg::spec::SortOrder::unsorted_order();

        // Get canonical path for location
        let canonical_path = table_path
            .canonicalize()
            .unwrap_or(table_path.to_path_buf());
        let location = canonical_path.to_string_lossy().to_string();

        // Build table metadata using TableMetadataBuilder
        let build_result = iceberg::spec::TableMetadataBuilder::new(
            iceberg_schema,
            partition_spec,
            sort_order,
            location.clone(),
            iceberg::spec::FormatVersion::V2,
            HashMap::new(),
        )
        .map_err(|e| Error::General(format!("Failed to create metadata builder: {}", e)))?
        .build()
        .map_err(|e| Error::General(format!("Failed to build table metadata: {}", e)))?;

        let metadata = build_result.metadata;
        let table_uuid = metadata.uuid();

        // Serialize metadata to JSON
        let metadata_json = serde_json::to_string_pretty(&metadata)
            .map_err(|e| Error::General(format!("Failed to serialize metadata: {}", e)))?;

        // Write metadata file
        let metadata_file = metadata_dir.join("v1.metadata.json");
        fs::write(&metadata_file, metadata_json)
            .map_err(|e| Error::General(format!("Failed to write metadata file: {}", e)))?;

        // Write version hint
        let version_hint_file = metadata_dir.join("version-hint.text");
        fs::write(&version_hint_file, "1")
            .map_err(|e| Error::General(format!("Failed to write version hint: {}", e)))?;

        println!(
            "{} Created Apache Iceberg table at {}",
            "✓".green(),
            table_path.display()
        );
        println!("  UUID: {}", table_uuid);
        println!("  Metadata: {}", metadata_file.display());

        Ok(())
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
            .map_err(|e| Error::General(format!("Failed to build Iceberg schema: {}", e)))
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
                return Err(Error::General(format!(
                    "Unsupported Iceberg type: {}. Supported: string, long, integer, float, double, boolean, binary, date, timestamp, timestamptz, time, uuid",
                    type_str
                )));
            }
        };

        Ok(Type::Primitive(prim))
    }
}

/// Schema definition for JSON input
#[derive(Debug, serde::Deserialize)]
pub struct SchemaDefinition {
    /// List of column definitions
    pub columns: Vec<ColumnDefinition>,
}

/// Column definition in schema
#[derive(Debug, serde::Deserialize)]
pub struct ColumnDefinition {
    /// Column name
    pub name: String,
    /// Data type (e.g., "string", "long", "integer", "double")
    #[serde(rename = "type")]
    pub data_type: String,
    /// Whether the column can contain null values (default: true)
    pub nullable: Option<bool>,
}
