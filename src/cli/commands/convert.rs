//! Convert command implementation
//!
//! Converts files to Iceberg tables.

use std::path::Path;

use crate::cli::output::{SeverityIcon, StatusIcon};
use crate::cli::parser::ConvertArgs;
use crate::core::formats::{FormatHandler, FormatHandlerRegistry, WriteOptions};
use crate::core::operations::convert::ConvertOperation;
use crate::core::operations::transform::TransformConfig;
use crate::core::storage::StorageBackendFactory;
use crate::error::{Error, Result};
use crate::utils::{ProgressTracker, parse_data_type};

/// Handler for convert command
pub struct ConvertCommand;

impl ConvertCommand {
    /// Execute convert command
    pub async fn execute(args: ConvertArgs) -> Result<()> {
        // 1. Check for partitioning (not yet supported)
        if args.partition_by.is_some() {
            return Err(Error::General(
                "Partitioning is not yet implemented. Coming soon!".to_string(),
            ));
        }

        // 2. Only Iceberg target is supported
        if let Some(ref target_format) = args.target_format {
            if target_format.to_lowercase() != "iceberg" {
                return Err(Error::UnsupportedFeature {
                    feature: "Only Iceberg is supported as target format. Use 'icebergctl import' for Delta sources.".to_string(),
                });
            }
        }

        // 3. Build transform configuration from CLI args
        let transform_config = Self::build_transform_config(&args)?;

        // 4. Create source storage and handler
        let source_storage = StorageBackendFactory::create_backend(&args.input).await?;
        let source_path = Path::new(&args.input);
        let source_handler = FormatHandlerRegistry::global()
            .create_handler(source_path, source_storage)
            .await?;

        // 5. Handle target
        let target_handler: Box<dyn FormatHandler> = if args.target_format.is_some() {
            // Table-to-table conversion to Iceberg
            Self::create_or_open_iceberg_table(&args, &source_handler).await?
        } else {
            // File format conversion
            if !args.overwrite && std::path::Path::new(&args.file).exists() {
                return Err(Error::General(format!(
                    "Output file '{}' already exists. Use --overwrite to replace it.",
                    args.file
                )));
            }
            let target_storage = StorageBackendFactory::create_backend(&args.file).await?;
            let target_path = Path::new(&args.file);
            FormatHandlerRegistry::global()
                .create_handler(target_path, target_storage)
                .await?
        };

        // 6. Build write options
        let mut write_options_builder = WriteOptions::builder()
            .enable_dictionary(true)
            .enable_statistics(true)
            .overwrite(args.overwrite);

        if let Some(compression) = args.compression.clone() {
            write_options_builder = write_options_builder.compression(compression);
        }

        if let Some(row_group_size) = args.row_group_size {
            write_options_builder = write_options_builder.row_group_size(row_group_size);
        }

        let write_options = write_options_builder.build();

        // 7. Execute conversion
        let progress =
            ProgressTracker::spinner(&format!("Converting {} to {}...", args.input, args.file));

        let operation = if transform_config.has_transforms() {
            ConvertOperation::with_transforms(
                source_handler.into(),
                target_handler.into(),
                transform_config,
            )
        } else {
            ConvertOperation::new(source_handler.into(), target_handler.into())
        };

        let result = operation.execute(&write_options).await?;

        progress.finish_with_message(&format!(
            "{} Conversion completed",
            StatusIcon::Success.as_str()
        ));

        // 8. Display results
        println!();
        println!("Source format:   {}", result.source_format);
        println!("Target format:   {}", result.target_format);
        println!("Rows converted:  {}", result.rows_converted);
        println!("Source size:     {} bytes", result.source_size);
        println!("Target size:     {} bytes", result.target_size);

        if let Some(ratio) = result.compression_ratio {
            println!("Compression:     {:.2}%", ratio);
        }

        // 9. Optionally validate output
        if args.validate {
            println!();
            println!("Validating output file...");

            use crate::core::operations::validate::ValidateOperation;

            let validate_storage = StorageBackendFactory::create_backend(&args.file).await?;
            let validate_path = Path::new(&args.file);
            let validate_handler = FormatHandlerRegistry::global()
                .create_handler(validate_path, validate_storage)
                .await?;

            let validate_op = ValidateOperation::new(validate_handler.into());
            let validate_result = validate_op.execute(false).await?;

            if validate_result.is_valid {
                println!("{} Output file is valid", StatusIcon::Success);
            } else {
                println!("{} Output file validation failed:", StatusIcon::Error);
                for error in &validate_result.errors {
                    println!("  {}  {}", SeverityIcon::Error, error);
                }
                return Err(Error::General(
                    "Conversion produced invalid output".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Build TransformConfig from CLI arguments
    fn build_transform_config(args: &ConvertArgs) -> Result<TransformConfig> {
        let mut config = TransformConfig::new();

        if let Some(columns) = &args.columns {
            config = config.with_columns(columns.clone());
        }

        if let Some(filter) = &args.where_clause {
            config = config.with_filter(filter.clone());
        }

        if let Some(renames) = &args.rename {
            for rename_spec in renames {
                let parts: Vec<&str> = rename_spec.split(':').collect();
                if parts.len() != 2 {
                    return Err(Error::General(format!(
                        "Invalid rename specification '{}'. Expected format: 'old_name:new_name'",
                        rename_spec
                    )));
                }
                config = config.with_rename(parts[0].to_string(), parts[1].to_string());
            }
        }

        if let Some(casts) = &args.cast {
            for cast_spec in casts {
                let parts: Vec<&str> = cast_spec.split(':').collect();
                if parts.len() != 2 {
                    return Err(Error::General(format!(
                        "Invalid cast specification '{}'. Expected format: 'column:type'",
                        cast_spec
                    )));
                }

                let column = parts[0].to_string();
                let type_str = parts[1];
                let data_type = parse_data_type(type_str)?;

                config = config.with_cast(column, data_type);
            }
        }

        Ok(config)
    }

    /// Create or open an Iceberg table for table-to-table conversion
    async fn create_or_open_iceberg_table(
        args: &ConvertArgs,
        source_handler: &Box<dyn FormatHandler>,
    ) -> Result<Box<dyn FormatHandler>> {
        use crate::cli::commands::init::InitCommand;
        use crate::cli::parser::InitArgs;

        let target_path = Path::new(&args.file);

        // Check if table already exists
        let table_exists = target_path.join("metadata").exists();

        if table_exists {
            if !args.overwrite {
                return Err(Error::General(format!(
                    "Target table '{}' already exists. Use --overwrite to replace it.",
                    args.file
                )));
            }
            std::fs::remove_dir_all(target_path)
                .map_err(|e| Error::General(format!("Failed to remove existing table: {}", e)))?;
        }

        // Get schema from source
        let source_schema = source_handler.read_schema().await?;

        // Create schema JSON for init command
        let schema_def = Self::arrow_schema_to_init_schema(&source_schema)?;
        let schema_json = serde_json::to_string(&schema_def)
            .map_err(|e| Error::General(format!("Failed to serialize schema: {}", e)))?;

        // Write temporary schema file
        let schema_file =
            std::env::temp_dir().join(format!("icebergctl_schema_{}.json", std::process::id()));
        std::fs::write(&schema_file, &schema_json)
            .map_err(|e| Error::General(format!("Failed to write schema file: {}", e)))?;

        // Create init args
        let init_args = InitArgs {
            format: "iceberg".to_string(),
            path: args.file.clone(),
            schema: Some(schema_file.clone()),
            name: None,
            description: Some(format!(
                "Converted from {} table",
                source_handler.format_name()
            )),
            partition_by: None,
            properties: None,
        };

        // Create the table
        InitCommand::execute(init_args).await?;

        // Clean up schema file
        let _ = std::fs::remove_file(&schema_file);

        // Now open the created table
        let target_storage = StorageBackendFactory::create_backend(&args.file).await?;
        FormatHandlerRegistry::global()
            .create_handler(target_path, target_storage)
            .await
    }

    /// Convert Arrow schema to init schema definition
    fn arrow_schema_to_init_schema(
        schema: &arrow::datatypes::Schema,
    ) -> Result<InitSchemaDefinition> {
        let columns: Vec<InitColumnDefinition> = schema
            .fields()
            .iter()
            .map(|field| {
                let type_str = Self::arrow_type_to_string(field.data_type());
                InitColumnDefinition {
                    name: field.name().clone(),
                    data_type: type_str,
                    nullable: Some(field.is_nullable()),
                }
            })
            .collect();

        Ok(InitSchemaDefinition { columns })
    }

    /// Convert Arrow DataType to string for init schema
    fn arrow_type_to_string(dt: &arrow::datatypes::DataType) -> String {
        use arrow::datatypes::DataType;

        match dt {
            DataType::Boolean => "boolean".to_string(),
            DataType::Int8 => "byte".to_string(),
            DataType::Int16 => "short".to_string(),
            DataType::Int32 => "integer".to_string(),
            DataType::Int64 => "long".to_string(),
            DataType::Float32 => "float".to_string(),
            DataType::Float64 => "double".to_string(),
            DataType::Utf8 | DataType::LargeUtf8 => "string".to_string(),
            DataType::Binary | DataType::LargeBinary => "binary".to_string(),
            DataType::Date32 | DataType::Date64 => "date".to_string(),
            DataType::Timestamp(_, _) => "timestamp".to_string(),
            DataType::Time32(_) | DataType::Time64(_) => "time".to_string(),
            DataType::Decimal128(_, _) | DataType::Decimal256(_, _) => "double".to_string(),
            _ => "string".to_string(),
        }
    }
}

/// Schema definition for init command
#[derive(serde::Serialize)]
struct InitSchemaDefinition {
    columns: Vec<InitColumnDefinition>,
}

/// Column definition for init schema
#[derive(serde::Serialize)]
struct InitColumnDefinition {
    name: String,
    #[serde(rename = "type")]
    data_type: String,
    nullable: Option<bool>,
}
