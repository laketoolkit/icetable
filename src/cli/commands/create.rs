//! Create command implementation
//!
//! Explicit subcommands:
//! - `create namespace <name>` - create a namespace
//! - `create table <name|path>` - create a table (catalog or path-based)
//!
//! Table type is auto-detected from the name:
//! - Path-based (no catalog): /, s3://, gs://, az://, file://, abfss://
//! - Catalog-based: namespace.table format

use colored::Colorize;

use super::common::resolve_catalog;
use crate::cli::parser::{CatalogContext, CreateArgs, CreateCommands, NamespaceCreateArgs, TableCreateArgs};
use crate::core::operations::{InitConfig, InitService};
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Handler for create command
pub struct CreateCommand;

impl CreateCommand {
    /// Execute create command
    pub async fn execute(args: CreateArgs, ctx: &CatalogContext) -> Result<()> {
        use super::constants::MEMORY_LIGHT_OPS;
        with_resource_limits(MEMORY_LIGHT_OPS, Self::execute_inner(args, ctx)).await
    }

    async fn execute_inner(args: CreateArgs, ctx: &CatalogContext) -> Result<()> {
        match args.command {
            CreateCommands::Namespace(ns_args) => Self::create_namespace(ns_args, ctx).await,
            CreateCommands::Table(tbl_args) => Self::create_table(tbl_args, ctx).await,
        }
    }

    async fn create_namespace(args: NamespaceCreateArgs, ctx: &CatalogContext) -> Result<()> {
        // Create a modified context with the namespace from args
        let mut ctx = ctx.clone();
        ctx.namespace = Some(args.name.clone());

        let catalog = resolve_catalog(&ctx).await?;
        let namespace = catalog.namespace().unwrap(); // Safe: we just set it

        // Build properties from args
        let properties: std::collections::HashMap<String, String> =
            args.property.iter().cloned().collect();

        catalog.create_namespace(properties).await?;

        println!("{} Created namespace: {}", "✓".green(), namespace.cyan());

        Ok(())
    }

    async fn create_table(args: TableCreateArgs, ctx: &CatalogContext) -> Result<()> {
        // Detect table type from name
        if args.is_path_based() {
            Self::create_table_path_based(args).await
        } else {
            Self::create_table_catalog(args, ctx).await
        }
    }

    /// Create a path-based table (no catalog registration)
    async fn create_table_path_based(args: TableCreateArgs) -> Result<()> {
        // Load schema if provided
        let schema = if let Some(ref schema_path) = args.schema {
            Some(InitService::load_schema_from_file(schema_path)?)
        } else {
            None
        };

        // Parse properties
        let properties: std::collections::HashMap<String, String> =
            args.property.iter().cloned().collect();

        // Build config and delegate to service
        let config = InitConfig {
            path: args.name.clone(),
            schema,
            partition_by: args.partition_by.clone(),
            properties,
        };

        let result = InitService::create_table(config).await?;

        // Output result
        println!(
            "{} Created Iceberg table at {}",
            "✓".green(),
            args.name
        );
        println!("  UUID:     {}", result.table_uuid);
        println!("  Metadata: {}", result.metadata_path);

        Ok(())
    }

    /// Create a catalog-based table
    async fn create_table_catalog(args: TableCreateArgs, ctx: &CatalogContext) -> Result<()> {
        // Schema is required for catalog tables
        let schema_path = args.schema.ok_or_else(|| Error::MissingArgument {
            argument: "--schema".to_string(),
            description: "Schema file required for catalog tables. Use --schema <file.json>".to_string(),
        })?;

        // Create a modified context with the table from args
        let mut ctx = ctx.clone();
        ctx.table = Some(args.name.clone());

        let catalog = resolve_catalog(&ctx).await?;

        // Must have namespace
        let namespace = catalog.namespace().ok_or_else(|| Error::MissingArgument {
            argument: "-n/--namespace".to_string(),
            description:
                "Namespace required to create table. Use -n or set context with 'icetable admin config use'"
                    .to_string(),
        })?;

        let table_name = catalog.table().unwrap(); // Safe: we just set it

        // Check if namespace exists
        if !catalog.namespace_exists().await? {
            return Err(Error::NamespaceNotFound {
                name: namespace.to_string(),
            });
        }

        // Read and parse schema file
        let schema_content =
            std::fs::read_to_string(&schema_path).map_err(|e| Error::Parse {
                message: format!(
                    "Failed to read schema file '{}': {}",
                    schema_path.display(),
                    e
                ),
                source: None,
            })?;

        let iceberg_schema: iceberg::spec::Schema =
            serde_json::from_str(&schema_content).map_err(|e| Error::Parse {
                message: format!("Failed to parse schema JSON: {}", e),
                source: None,
            })?;

        // Build properties from args
        let properties: std::collections::HashMap<String, String> =
            args.property.iter().cloned().collect();

        catalog
            .create_table(
                table_name,
                iceberg_schema,
                args.location.as_deref(),
                properties,
            )
            .await?;

        println!(
            "{} Created table: {}.{}",
            "✓".green(),
            namespace.cyan(),
            table_name.cyan()
        );

        Ok(())
    }
}
