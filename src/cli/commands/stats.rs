//! Stats command implementation

use std::path::Path;
use std::sync::Arc;

use crate::cli::output::OutputFormatter;
use crate::cli::parser::StatsArgs;
use crate::config::ResolvePath;
use crate::core::formats::FormatHandlerFactory;
use crate::core::operations::stats::{StatsOperation, StatsOptions};
use crate::core::storage::StorageBackendFactory;
use crate::error::{Error, Result};

/// Handler for stats command
pub struct StatsCommand;

impl StatsCommand {
    /// Execute stats command
    pub async fn execute(args: StatsArgs) -> Result<()> {
        let table_path = args.path.resolve()?;
        let path = Path::new(&table_path);

        // Create storage backend (supports local and cloud)
        let storage = StorageBackendFactory::create_backend(&table_path).await?;

        // Get format handler (use explicit format if provided, otherwise auto-detect)
        let handler = if let Some(format) = &args.format {
            FormatHandlerFactory::create_handler_for_format(format, path, storage).await?
        } else {
            FormatHandlerFactory::create_handler(path, storage).await?
        };

        // Convert to Arc for StatsOperation
        let handler_arc = Arc::from(handler);

        // Create operation
        let operation = StatsOperation::new(handler_arc);

        // Build options from args
        let mut options = StatsOptions::default();
        options.include_histogram = args.histogram;
        options.profile = args.profile;
        options.columns = args.columns;

        if let Some(percentiles) = args.percentiles {
            // Validate percentiles are in [0, 1] range
            for p in &percentiles {
                if !(0.0..=1.0).contains(p) {
                    return Err(Error::General(format!(
                        "Percentile {} must be between 0.0 and 1.0",
                        p
                    )));
                }
            }
            options.percentiles = percentiles;
        }

        // Execute operation
        let result = operation.execute(&options).await?;

        // Format output
        let output = match args.output.as_str() {
            "json" => OutputFormatter::format_json(&result)?,
            "yaml" => OutputFormatter::format_yaml(&result)?,
            "text" | _ => OutputFormatter::format_stats_result(&result, &options),
        };

        println!("{}", output);

        Ok(())
    }
}
