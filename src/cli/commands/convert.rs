//! Convert command implementation

use std::path::Path;

use crate::cli::output::{SeverityIcon, StatusIcon};
use crate::cli::parser::ConvertArgs;
use crate::core::formats::{FormatHandlerFactory, WriteOptions};
use crate::core::operations::convert::ConvertOperation;
use crate::core::storage::StorageBackendFactory;
use crate::error::{Error, Result};
use crate::utils::progress::ProgressTracker;

/// Handler for convert command
pub struct ConvertCommand;

impl ConvertCommand {
    /// Execute convert command
    pub async fn execute(args: ConvertArgs) -> Result<()> {
        // 1. Check if output file exists and handle overwrite
        if !args.overwrite && std::path::Path::new(&args.output).exists() {
            return Err(Error::General(format!(
                "Output file '{}' already exists. Use --overwrite to replace it.",
                args.output
            )));
        }

        // 2. Create storage backends
        let source_storage = StorageBackendFactory::create_backend(&args.input).await?;
        let target_storage = StorageBackendFactory::create_backend(&args.output).await?;

        // 3. Create format handlers
        let source_path = Path::new(&args.input);
        let source_handler =
            FormatHandlerFactory::create_handler(source_path, source_storage).await?;

        let target_path = Path::new(&args.output);
        let target_handler =
            FormatHandlerFactory::create_handler(target_path, target_storage).await?;

        // 4. Build write options
        let write_options = WriteOptions {
            compression: args.compression.clone(),
            row_group_size: args.row_group_size,
            enable_dictionary: true,
            enable_statistics: true,
            overwrite: args.overwrite,
        };

        // 5. Execute conversion
        let progress = ProgressTracker::spinner(&format!("Converting {} to {}...", args.input, args.output));

        let operation = ConvertOperation::new(source_handler.into(), target_handler.into());
        let result = operation.execute(&write_options).await?;

        progress.finish_with_message(&format!("{} Conversion completed", StatusIcon::Success.as_str()));

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

            let validate_storage = StorageBackendFactory::create_backend(&args.output).await?;
            let validate_handler =
                FormatHandlerFactory::create_handler(target_path, validate_storage).await?;

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
}
