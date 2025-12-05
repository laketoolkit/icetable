//! Inspect command implementation

pub mod common;

use std::path::Path;

use crate::cli::output::InspectionFormatter;
use crate::cli::parser::InspectArgs;
use crate::config::ResolvePath;
use crate::core::formats::{FormatHandlerRegistry, TimeTravelOptions};
use crate::core::operations::inspect::{InspectOperation, InspectOptions};
use crate::core::storage::StorageBackendFactory;
use crate::error::Result;
use common::VerbosityLevel;

use common::PhysicalInspectOptions;

/// Handler for inspect command
pub struct InspectCommand;

impl InspectCommand {
    /// Execute inspect command
    pub async fn execute(args: InspectArgs) -> Result<()> {
        let path = args.path.resolve()?;

        // Check if any physical layout flags are set
        let physical_mode =
            args.layout || (!args.schema && !args.metadata && !args.stats && !args.preview);

        // If physical mode, use new physical layout inspection
        if physical_mode {
            return Self::execute_physical_inspect(&path, args).await;
        }

        // Otherwise, use legacy inspect (for backwards compatibility)
        Self::execute_legacy_inspect(&path, args).await
    }

    /// Execute physical layout inspection (new mode)
    async fn execute_physical_inspect(path_str: &str, args: InspectArgs) -> Result<()> {
        // 1. Create storage backend based on path
        let storage = StorageBackendFactory::create_backend(path_str).await?;

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
            args.deep,
        );

        // 3. Inspect physical layout (progress shown inside service)
        let path = Path::new(path_str);
        let result = self::inspect_physical_layout(path, storage, &options).await?;

        // 4. Display result
        println!("{}", result);

        Ok(())
    }

    /// Execute legacy inspect (old mode, for backwards compatibility)
    async fn execute_legacy_inspect(path_str: &str, args: InspectArgs) -> Result<()> {
        // 1. Create storage backend based on path
        let storage = StorageBackendFactory::create_backend(path_str).await?;

        // 2. Build time-travel options from CLI args
        let time_travel = TimeTravelOptions {
            version: args.version,
            as_of: args.as_of.clone(),
        };

        // 3. Create format handler with time-travel support
        let path = Path::new(path_str);
        let handler = FormatHandlerRegistry::global()
            .create_handler_with_options(path, storage, time_travel)
            .await?;

        // 4. Build inspect options
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

        // 5. Execute inspect operation
        let operation = InspectOperation::new(handler);
        let result = operation.execute(&options).await?;

        // 6. Format and display output based on output format
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
                let output = InspectionFormatter::format_inspect_result(&result, &options);
                println!("{}", output);
            }
        }

        Ok(())
    }
}

/// Inspect physical layout of a file
async fn inspect_physical_layout(
    path: &Path,
    _storage: std::sync::Arc<dyn crate::core::storage::StorageBackend>,
    options: &PhysicalInspectOptions,
) -> Result<String> {
    // Use the new PhysicalInspectionService with dynamic inspector registry
    use crate::core::inspection::{PhysicalInspectionService, view_to_inspect_result};

    // Convert CLI options to core options (same structure, different types)
    let core_options = crate::core::inspection::PhysicalInspectOptions {
        show_schema: options.show_schema,
        show_layout: options.show_layout,
        show_stats: options.show_stats,
        verbosity: match options.verbosity {
            VerbosityLevel::Normal => crate::core::inspection::VerbosityLevel::Normal,
            VerbosityLevel::Verbose => crate::core::inspection::VerbosityLevel::Verbose,
        },
        deep_scan: options.deep_scan,
    };

    let service = PhysicalInspectionService::new();
    let view = service.inspect(path, core_options).await?;

    // Convert InspectionView to PhysicalInspectResult and render
    let result = view_to_inspect_result(&view);
    Ok(result.render(&view.format_name))
}
