//! Create command implementation
//!
//! Explicit subcommands:
//! - `create namespace <name>` - create a namespace
//! - `create table <name> --schema <file>` - create a table

use colored::Colorize;

use super::common::resolve_catalog;
use crate::cli::parser::{
    CliTableContext, CreateArgs, CreateCommands, NamespaceCreateArgs, TableCreateArgs,
};
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Handler for create command
pub struct CreateCommand;

impl CreateCommand {
    /// Execute create command
    pub async fn execute(args: CreateArgs, ctx: &CliTableContext) -> Result<()> {
        const ESTIMATED_MEMORY: u64 = 32 * 1024 * 1024;
        with_resource_limits(ESTIMATED_MEMORY, Self::execute_inner(args, ctx)).await
    }

    async fn execute_inner(args: CreateArgs, ctx: &CliTableContext) -> Result<()> {
        match args.command {
            CreateCommands::Namespace(ns_args) => Self::create_namespace(ns_args, ctx).await,
            CreateCommands::Table(tbl_args) => Self::create_table(tbl_args, ctx).await,
        }
    }

    async fn create_namespace(args: NamespaceCreateArgs, ctx: &CliTableContext) -> Result<()> {
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

    async fn create_table(args: TableCreateArgs, ctx: &CliTableContext) -> Result<()> {
        // Create a modified context with the table from args
        let mut ctx = ctx.clone();
        ctx.table = Some(args.name.clone());

        let catalog = resolve_catalog(&ctx).await?;

        // Must have namespace
        let namespace = catalog.namespace().ok_or_else(|| Error::MissingArgument {
            argument: "-n/--namespace".to_string(),
            description:
                "Namespace required to create table. Use -n or set context with 'icetable config use'"
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
            std::fs::read_to_string(&args.schema).map_err(|e| Error::Parse {
                message: format!(
                    "Failed to read schema file '{}': {}",
                    args.schema.display(),
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
