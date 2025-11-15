//! Convert command implementation

use std::path::Path;

use crate::cli::output::{SeverityIcon, StatusIcon};
use crate::cli::parser::ConvertArgs;
use crate::core::formats::{FormatHandlerRegistry, WriteOptions};
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
        // 1. Check if output file exists and handle overwrite
        if !args.overwrite && std::path::Path::new(&args.file).exists() {
            return Err(Error::General(format!(
                "Output file '{}' already exists. Use --overwrite to replace it.",
                args.file
            )));
        }

        // 2. Check for partitioning (not yet supported)
        if args.partition_by.is_some() {
            return Err(Error::General(
                "Partitioning is not yet implemented. Coming soon!".to_string(),
            ));
        }

        // 3. Build transform configuration from CLI args
        let transform_config = Self::build_transform_config(&args)?;

        // 4. Create storage backends
        let source_storage = StorageBackendFactory::create_backend(&args.input).await?;
        let target_storage = StorageBackendFactory::create_backend(&args.file).await?;

        // 5. Create format handlers using registry
        let source_path = Path::new(&args.input);
        let source_handler = FormatHandlerRegistry::global()
            .create_handler(source_path, source_storage)
            .await?;

        let target_path = Path::new(&args.file);
        let target_handler = FormatHandlerRegistry::global()
            .create_handler(target_path, target_storage)
            .await?;

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

        // 6. Display results
        println!();
        println!("Source format:   {}", result.source_format);
        println!("Target format:   {}", result.target_format);
        println!("Rows converted:  {}", result.rows_converted);
        println!("Source size:     {} bytes", result.source_size);
        println!("Target size:     {} bytes", result.target_size);

        if let Some(ratio) = result.compression_ratio {
            println!("Compression:     {:.2}%", ratio);
        }

        // 7. Optionally validate output
        if args.validate {
            println!();
            println!("Validating output file...");

            use crate::core::operations::validate::ValidateOperation;

            let validate_storage = StorageBackendFactory::create_backend(&args.file).await?;
            let validate_handler = FormatHandlerRegistry::global()
                .create_handler(target_path, validate_storage)
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

        // Add column selection
        if let Some(columns) = &args.columns {
            config = config.with_columns(columns.clone());
        }

        // Add filter expression
        if let Some(filter) = &args.where_clause {
            config = config.with_filter(filter.clone());
        }

        // Parse and add column renames
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

        // Parse and add column casts
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

                // Parse Arrow data type
                let data_type = parse_data_type(type_str)?;

                config = config.with_cast(column, data_type);
            }
        }

        Ok(config)
    }
}
