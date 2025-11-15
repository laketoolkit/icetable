//! Inspect command implementation

use std::path::Path;

use crate::cli::output::OutputFormatter;
use crate::cli::parser::InspectArgs;
use crate::core::formats::FormatHandlerRegistry;
use crate::core::operations::inspect::{InspectOperation, InspectOptions};
use crate::core::storage::StorageBackendFactory;
use crate::error::Result;

/// Handler for inspect command
pub struct InspectCommand;

impl InspectCommand {
    /// Execute inspect command
    pub async fn execute(args: InspectArgs) -> Result<()> {
        // 1. Create storage backend based on path
        let storage = StorageBackendFactory::create_backend(&args.path).await?;

        // 2. Create format handler
        let path = Path::new(&args.path);
        let handler = FormatHandlerRegistry::global()
            .create_handler(path, storage)
            .await?;

        // 3. Build inspect options
        // Logic: If NO specific flags are set, show everything
        // If ANY flag is set, show only what's requested (combinable)
        let any_flag_set = args.schema || args.metadata || args.stats || args.data;

        let options = InspectOptions {
            schema_only: args.schema && !args.metadata && !args.stats && !args.data,
            show_schema: if any_flag_set { args.schema } else { true },
            show_metadata: if any_flag_set { args.metadata } else { true },
            show_stats: if any_flag_set { args.stats } else { false },
            show_data: if any_flag_set { args.data } else { true },
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
