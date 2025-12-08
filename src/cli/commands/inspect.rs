//! Inspect command implementation

pub mod common;

use std::path::Path;

use crate::cli::output::InspectionFormatter;
use crate::cli::parser::InspectArgs;
use crate::config::{ResolveTableRef, ResolvedTable};
use crate::core::formats::{FormatHandlerRegistry, TimeTravelOptions};
use crate::core::operations::inspect::{InspectOperation, InspectOptions};
use crate::core::storage::create_object_store;
use crate::core::{CatalogConfig, TableRef};
use crate::error::Result;
use crate::utils::{track_memory_usage, with_cancellation, with_timeout};

use common::{PhysicalInspectOptions, VerbosityLevel};

/// Handler for inspect command
pub struct InspectCommand;

impl InspectCommand {
    /// Execute inspect command
    pub async fn execute(args: InspectArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        // Apply timeout and cancellation from global resource limits
        with_timeout(async {
            with_cancellation(async {
                // Estimate memory usage: depends on --deep flag and rows
                let estimated_memory = if args.deep {
                    512 * 1024 * 1024 // 512MB for deep inspection
                } else {
                    128 * 1024 * 1024 // 128MB for regular inspection
                };
                track_memory_usage(estimated_memory)?;

                Self::inspect_inner(args, catalog_config).await
            })
            .await
        })
        .await
    }

    async fn inspect_inner(args: InspectArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        // Priority 1: If --catalog-uri is provided, use it directly
        if let Some(ref cli_catalog) = catalog_config {
            let table_input = args.path.as_ref().ok_or_else(|| {
                crate::error::Error::General(
                    "Table identifier required when using --catalog-uri (e.g., namespace.table)"
                        .to_string(),
                )
            })?;

            let table_ref = TableRef::parse(table_input, Some(cli_catalog));

            if let TableRef::Catalog { namespace, name } = &table_ref {
                return Self::execute_catalog_inspect(namespace, name, cli_catalog, args).await;
            }
        }

        // Priority 2: Try to resolve from config (may return path or catalog reference)
        let resolved = args.path.resolve_ref()?;

        match resolved {
            ResolvedTable::Path(path_str) => {
                // Direct path mode
                return Self::execute_path_inspect(&path_str, args).await;
            }
            ResolvedTable::Catalog {
                catalog_config,
                table_name,
                ..
            } => {
                // Parse table_name which may be "namespace.table" or "ns1.ns2.table"
                let parts: Vec<&str> = table_name.split('.').collect();
                let (namespace, name) = if parts.len() >= 2 {
                    // Safe: we checked len >= 2, so last() always exists
                    let name = parts.last().expect("checked len >= 2").to_string();
                    let namespace: Vec<String> = parts[..parts.len() - 1]
                        .iter()
                        .map(|s| s.to_string())
                        .collect();
                    (namespace, name)
                } else {
                    // Single name, use default namespace
                    (vec!["default".to_string()], table_name.clone())
                };

                return Self::execute_catalog_inspect(&namespace, &name, &catalog_config, args)
                    .await;
            }
        }
    }

    /// Execute inspect for a direct path
    async fn execute_path_inspect(path_str: &str, args: InspectArgs) -> Result<()> {
        let path = path_str;

        // Check if any physical layout flags are set
        let physical_mode =
            args.layout || (!args.schema && !args.metadata && !args.stats && !args.preview);

        // If physical mode, use new physical layout inspection
        if physical_mode {
            return Self::execute_physical_inspect(path, args).await;
        }

        // Otherwise, use legacy inspect (for backwards compatibility)
        Self::execute_legacy_inspect(path, args).await
    }

    /// Execute physical layout inspection (new mode)
    async fn execute_physical_inspect(path_str: &str, args: InspectArgs) -> Result<()> {
        // 1. Build physical inspect options
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

        // 2. Inspect physical layout (progress shown inside service)
        let result = self::inspect_physical_layout(path_str, &options).await?;

        // 3. Display result
        println!("{}", result);

        Ok(())
    }

    /// Execute legacy inspect (old mode, for backwards compatibility)
    async fn execute_legacy_inspect(path_str: &str, args: InspectArgs) -> Result<()> {
        // 1. Create storage backend based on path
        let storage = create_object_store(path_str).await?;

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

    /// Execute catalog-based inspect (REST catalog mode)
    async fn execute_catalog_inspect(
        namespace: &[String],
        name: &str,
        catalog_config: &CatalogConfig,
        args: InspectArgs,
    ) -> Result<()> {
        use crate::core::CatalogClient;
        use colored::Colorize;

        // Create catalog client
        let catalog = CatalogClient::new(Some(catalog_config.clone())).await?;

        // Load table from catalog
        let table = catalog
            .load_table(&crate::core::TableRef::Catalog {
                namespace: namespace.to_vec(),
                name: name.to_string(),
            })
            .await?;

        // Get table metadata
        let metadata = table.metadata();

        // Print basic info
        println!("{}", "Iceberg Table (via Catalog)".cyan().bold());
        println!();
        println!("{}: {}.{}", "Table".bold(), namespace.join("."), name);
        println!("{}: {}", "Format Version".bold(), metadata.format_version());
        println!("{}: {}", "Location".bold(), metadata.location());

        if let Some(current_snapshot_id) = metadata.current_snapshot_id() {
            println!("{}: {}", "Current Snapshot".bold(), current_snapshot_id);
        }

        println!("{}: {}", "Snapshots".bold(), metadata.snapshots().len());

        // Show schema if requested
        if args.schema || (!args.metadata && !args.stats && !args.preview) {
            println!();
            println!("{}", "Schema:".cyan().bold());
            let schema = metadata.current_schema();
            for field in schema.as_struct().fields() {
                let nullable = if field.required { "" } else { " (nullable)" };
                println!("  {} : {}{}", field.name.bold(), field.field_type, nullable);
            }
        }

        // Show partition spec
        let spec = metadata.default_partition_spec();
        if !spec.fields().is_empty() {
            println!();
            println!("{}", "Partition Spec:".cyan().bold());
            for field in spec.fields() {
                println!("  {} ({})", field.name, field.transform);
            }
        }

        Ok(())
    }
}

/// Inspect physical layout of a table
async fn inspect_physical_layout(path: &str, options: &PhysicalInspectOptions) -> Result<String> {
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
