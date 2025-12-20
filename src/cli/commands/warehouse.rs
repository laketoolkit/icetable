//! Warehouse command implementation
//!
//! Thin wrapper that delegates to core services.

use std::collections::HashMap;
use std::path::Path;

use colored::Colorize;

use super::common::{get_catalog_provider, resolve_management_from_context};
use crate::cli::output::{AdminFormatter, WarehouseInfo};
use crate::cli::parser::{
    CatalogContext, WarehouseArgs, WarehouseCommands, WarehouseCreateArgs, WarehouseDeleteArgs,
    WarehouseLsArgs,
};
use crate::config::{DeleteProgress, WarehouseService};
use crate::core::catalog::CreateWarehouseRequest;
use crate::error::{Error, Result};

/// Handler for warehouse command
pub struct WarehouseCommand;

impl WarehouseCommand {
    /// Execute warehouse command
    pub async fn execute(args: WarehouseArgs, ctx: &CatalogContext) -> Result<()> {
        match args.command {
            WarehouseCommands::Ls(args) => Self::ls(args, ctx).await,
            WarehouseCommands::Create(args) => Self::create(args, ctx).await,
            WarehouseCommands::Delete(args) => Self::delete(args, ctx).await,
        }
    }

    /// List warehouses
    async fn ls(args: WarehouseLsArgs, ctx: &CatalogContext) -> Result<()> {
        let resolution = resolve_management_from_context(ctx).await?;

        if !resolution.supports_management() {
            return Err(Error::UnsupportedFeature {
                feature: format!(
                    "Warehouse management not supported for {} catalogs",
                    resolution.catalog_type()
                ),
            });
        }

        let warehouses = resolution.client.list_warehouses().await?;

        // Convert to formatter types
        let formatter_warehouses: Vec<WarehouseInfo> = warehouses
            .iter()
            .map(|wh| WarehouseInfo {
                name: wh.name.clone(),
                warehouse_type: wh.warehouse_type.to_string(),
                storage_type: wh.storage_type.to_string(),
                location: wh.default_base_location.clone(),
            })
            .collect();

        if args.output == "json" {
            let json_str = AdminFormatter::format_warehouse_list_json(
                resolution.catalog_name(),
                &formatter_warehouses,
            )
            .map_err(|e| Error::Serialization {
                message: e.to_string(),
            })?;
            println!("{}", json_str);
        } else {
            println!(
                "{}",
                AdminFormatter::format_warehouse_list_table(
                    resolution.catalog_name(),
                    &formatter_warehouses
                )
            );
        }

        Ok(())
    }

    /// Create a warehouse
    async fn create(args: WarehouseCreateArgs, ctx: &CatalogContext) -> Result<()> {
        // Handle --examples flag
        if args.examples {
            let provider = get_catalog_provider(ctx)?;
            println!(
                "{}",
                AdminFormatter::format_warehouse_examples(&provider.to_string().to_lowercase())
            );
            return Ok(());
        }

        // At this point, name and location are guaranteed by clap's required_unless_present
        let name = args.name.as_ref().ok_or_else(|| Error::Configuration {
            message: "warehouse name is required (use --name or --examples)".to_string(),
        })?;
        let location = args.location.as_ref().ok_or_else(|| Error::Configuration {
            message: "warehouse location is required (use --location or --examples)".to_string(),
        })?;

        let resolution = resolve_management_from_context(ctx).await?;

        if !resolution.supports_management() {
            return Err(Error::UnsupportedFeature {
                feature: format!(
                    "Warehouse management not supported for {} catalogs",
                    resolution.catalog_type()
                ),
            });
        }

        // Parse storage config from --config and --config-set (CLI arg parsing)
        let storage_config = Self::parse_storage_config(&args.config, &args.config_set)?;

        let request =
            CreateWarehouseRequest::new(name, location).with_storage_config_map(storage_config);

        let warehouse = resolution.client.create_warehouse(request).await?;

        println!(
            "{}",
            AdminFormatter::format_warehouse_create_success(
                &warehouse.name,
                resolution.catalog_name(),
                &warehouse.default_base_location
            )
        );

        Ok(())
    }

    /// Delete a warehouse
    async fn delete(args: WarehouseDeleteArgs, ctx: &CatalogContext) -> Result<()> {
        let resolution = resolve_management_from_context(ctx).await?;

        if !resolution.supports_management() {
            return Err(Error::UnsupportedFeature {
                feature: format!(
                    "Warehouse management not supported for {} catalogs",
                    resolution.catalog_type()
                ),
            });
        }

        // If --force, delegate to service for cascade delete
        if args.force {
            let progress =
                WarehouseService::force_delete_contents(resolution.catalog_name(), &args.name)
                    .await?;

            // Format the progress output
            Self::print_delete_progress(&progress);
        }

        resolution.client.delete_warehouse(&args.name).await?;

        println!(
            "{}",
            AdminFormatter::format_warehouse_delete_success(&args.name, resolution.catalog_name())
        );

        Ok(())
    }

    /// Format and print delete progress
    fn print_delete_progress(progress: &DeleteProgress) {
        // Print deleted tables
        for (ns, table) in &progress.tables_deleted {
            println!(
                "  {} {}.{} ... {}",
                "Deleting".dimmed(),
                ns,
                table,
                "ok".green()
            );
        }

        // Print failed tables
        for (ns, table, err) in &progress.tables_failed {
            println!(
                "  {} {}.{} ... {} ({})",
                "Deleting".dimmed(),
                ns,
                table,
                "failed".red(),
                err
            );
        }

        // Print deleted namespaces
        for ns in &progress.namespaces_deleted {
            println!(
                "  {} {} ... {}",
                "Deleting namespace".dimmed(),
                ns,
                "ok".green()
            );
        }

        // Print failed namespaces
        for (ns, err) in &progress.namespaces_failed {
            println!(
                "  {} {} ... {} ({})",
                "Deleting namespace".dimmed(),
                ns,
                "failed".red(),
                err
            );
        }
    }

    /// Parse storage configuration from CLI args (--config and --config-set)
    fn parse_storage_config(
        config: &Option<String>,
        config_set: &[(String, String)],
    ) -> Result<HashMap<String, serde_json::Value>> {
        let mut result = HashMap::new();

        if let Some(config_str) = config {
            let config_str = config_str.trim();

            if config_str.starts_with('{') {
                // Inline JSON
                result = serde_json::from_str(config_str).map_err(|e| Error::Parse {
                    message: format!("Invalid JSON in --config: {}", e),
                    source: Some(Box::new(e)),
                })?;
            } else {
                // File path
                let content = std::fs::read_to_string(Path::new(config_str)).map_err(|e| {
                    Error::Parse {
                        message: format!("Failed to read config file '{}': {}", config_str, e),
                        source: Some(Box::new(e)),
                    }
                })?;

                result = serde_json::from_str(&content).map_err(|e| Error::Parse {
                    message: format!("Invalid JSON in config file '{}': {}", config_str, e),
                    source: Some(Box::new(e)),
                })?;
            }
        }

        // Apply --config-set overrides
        for (key, value) in config_set {
            result.insert(key.clone(), serde_json::Value::String(value.clone()));
        }

        Ok(result)
    }
}
