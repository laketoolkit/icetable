//! Inspect command implementation

mod arrow;
mod common;
mod csv;
mod delta;
mod iceberg;
mod json;
mod parquet;

use std::path::Path;

use crate::cli::output::OutputFormatter;
use crate::cli::parser::InspectArgs;
use crate::core::formats::FormatHandlerRegistry;
use common::VerbosityLevel;
use crate::core::operations::inspect::{InspectOperation, InspectOptions};
use crate::core::storage::StorageBackendFactory;
use crate::error::Result;

use common::PhysicalInspectOptions;

/// Handler for inspect command
pub struct InspectCommand;

impl InspectCommand {
    /// Execute inspect command
    pub async fn execute(args: InspectArgs) -> Result<()> {
        // Check if any physical layout flags are set
        let physical_mode = args.layout || (!args.schema && !args.metadata && !args.stats && !args.preview);

        // If physical mode, use new physical layout inspection
        if physical_mode {
            return Self::execute_physical_inspect(args).await;
        }

        // Otherwise, use legacy inspect (for backwards compatibility)
        Self::execute_legacy_inspect(args).await
    }

    /// Execute physical layout inspection (new mode)
    async fn execute_physical_inspect(args: InspectArgs) -> Result<()> {
        // 1. Create storage backend based on path
        let storage = StorageBackendFactory::create_backend(&args.path).await?;

        // 2. Build physical inspect options
        let verbosity = if args.verbose {
            VerbosityLevel::Verbose
        } else {
            VerbosityLevel::Normal
        };
        let options = PhysicalInspectOptions::from_cli_args(
            args.schema,
            args.layout,
            args.stats,
            verbosity,
        );

        // 3. Inspect physical layout
        let path = Path::new(&args.path);
        let result = self::inspect_physical_layout(path, storage, &options).await?;

        // 4. Display result
        println!("{}", result);

        Ok(())
    }

    /// Execute legacy inspect (old mode, for backwards compatibility)
    async fn execute_legacy_inspect(args: InspectArgs) -> Result<()> {
        // 1. Create storage backend based on path
        let storage = StorageBackendFactory::create_backend(&args.path).await?;

        // 2. Create format handler
        let path = Path::new(&args.path);
        let handler = FormatHandlerRegistry::global()
            .create_handler(path, storage)
            .await?;

        // 3. Build inspect options
        // Logic: Default shows file info, metadata, schema, and stats
        // --preview adds data preview to the default view
        // Individual flags (-s, -m, --stats) show only what's requested
        let any_flag_set = args.schema || args.metadata || args.stats;

        let options = InspectOptions {
            schema_only: args.schema && !args.metadata && !args.stats && !args.preview,
            show_schema: if any_flag_set { args.schema } else { true },
            show_metadata: if any_flag_set { args.metadata } else { true },
            show_stats: if any_flag_set { args.stats } else { true },
            show_data: args.preview,
            num_rows: args.rows,
            columns: args.columns,
            sample: args.sample,
        };

        // 4. Execute inspect operation
        let operation = InspectOperation::new(handler);
        let result = operation.execute(&options).await?;

        // 5. Format and display output based on output format
        match args.output.as_str() {
            "json" => {
                // For JSON output, we need to create a serializable structure
                let json_output = serde_json::json!({
                    "format": result.format_name,
                    "schema": {
                        "fields": result.schema.fields().iter().map(|f| {
                            serde_json::json!({
                                "name": f.name(),
                                "type": format!("{:?}", f.data_type()),
                                "nullable": f.is_nullable(),
                            })
                        }).collect::<Vec<_>>(),
                    },
                    "metadata": result.metadata.as_ref().map(|m| {
                        serde_json::json!({
                            "num_rows": m.num_rows,
                            "compressed_size": m.compressed_size,
                            "uncompressed_size": m.uncompressed_size,
                            "compression": m.compression,
                            "format_version": m.format_version,
                        })
                    }),
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_output)
                        .map_err(|e| crate::error::Error::General(e.to_string()))?
                );
            }
            "yaml" => {
                // Similar to JSON but YAML format
                let yaml_output = serde_json::json!({
                    "format": result.format_name,
                    "schema": {
                        "fields": result.schema.fields().iter().map(|f| {
                            serde_json::json!({
                                "name": f.name(),
                                "type": format!("{:?}", f.data_type()),
                                "nullable": f.is_nullable(),
                            })
                        }).collect::<Vec<_>>(),
                    },
                });
                println!(
                    "{}",
                    serde_yaml::to_string(&yaml_output)
                        .map_err(|e| crate::error::Error::General(e.to_string()))?
                );
            }
            _ => {
                // Default table format
                let output = OutputFormatter::format_inspect_result(&result, &options);
                println!("{}", output);
            }
        }

        Ok(())
    }
}

/// Inspect physical layout of a file
async fn inspect_physical_layout(
    path: &Path,
    storage: std::sync::Arc<dyn crate::core::storage::StorageBackend>,
    options: &PhysicalInspectOptions,
) -> Result<String> {
    // Detect format and dispatch to appropriate inspector
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    let result = match extension.as_str() {
        "parquet" => {
            let result = parquet::inspect_parquet_layout(path, storage, options).await?;
            result.render("Apache Parquet File")
        }
        "arrow" | "ipc" => {
            let result = arrow::inspect_arrow_layout(path, storage, options).await?;
            result.render("Apache Arrow IPC File")
        }
        "csv" => {
            let result = csv::inspect_csv_layout(path, storage, options).await?;
            result.render("CSV File")
        }
        "json" | "jsonl" | "ndjson" => {
            let result = json::inspect_json_layout(path, storage, options).await?;
            result.render("JSON File")
        }
        _ => {
            // Try Delta Lake (directory-based) - check for _delta_log
            let path_str = path.to_str().unwrap_or("");
            let delta_log_path = format!("{}/_delta_log", path_str);

            if storage.exists(&delta_log_path).await.unwrap_or(false) {
                let result = delta::inspect_delta_layout(path, storage, options).await?;
                return Ok(result.render("Delta Lake Table"));
            }

            // Check for Iceberg metadata
            let metadata_path = format!("{}/metadata", path_str);
            if storage.exists(&metadata_path).await.unwrap_or(false) {
                let result = iceberg::inspect_iceberg_layout(path, storage, options).await?;
                return Ok(result.render("Apache Iceberg Table"));
            }

            return Err(crate::error::Error::InvalidFormat {
                message: format!(
                    "Unsupported file format for physical inspection: {}",
                    extension
                ),
            });
        }
    };

    Ok(result)
}
