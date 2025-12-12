//! Create command implementation
//!
//! Creates namespaces or tables in a catalog.
//! - `create -n <namespace>` creates a namespace
//! - `create -n <namespace> -t <table> --schema <file>` creates a table

use colored::Colorize;

use super::common::{CatalogResolution, no_namespace_error, resolve_catalog_from_context};
use crate::cli::parser::{CreateArgs, TableContext};
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Handler for create command
pub struct CreateCommand;

impl CreateCommand {
    /// Execute create command
    pub async fn execute(args: CreateArgs, ctx: &TableContext) -> Result<()> {
        const ESTIMATED_MEMORY: u64 = 32 * 1024 * 1024;
        with_resource_limits(ESTIMATED_MEMORY, Self::execute_inner(args, ctx)).await
    }

    async fn execute_inner(args: CreateArgs, ctx: &TableContext) -> Result<()> {
        // Resolve catalog context (error propagates with full context)
        let catalog = resolve_catalog_from_context(ctx, args.catalog.as_deref()).await?;

        // Must have namespace to create anything
        let namespace = catalog.namespace().ok_or_else(no_namespace_error)?;

        if let Some(table_name) = catalog.table() {
            // Create table
            Self::create_table(&catalog, namespace, table_name, &args).await
        } else {
            // Create namespace
            Self::create_namespace(&catalog, namespace, &args).await
        }
    }

    async fn create_namespace(
        catalog: &CatalogResolution,
        namespace: &str,
        args: &CreateArgs,
    ) -> Result<()> {
        // Build properties from args
        let properties: std::collections::HashMap<String, String> =
            args.property.iter().cloned().collect();

        catalog.create_namespace(properties).await?;

        println!("{} Created namespace: {}", "✓".green(), namespace.cyan());

        Ok(())
    }

    async fn create_table(
        catalog: &CatalogResolution,
        namespace: &str,
        table_name: &str,
        args: &CreateArgs,
    ) -> Result<()> {
        // Check if namespace exists
        if !catalog.namespace_exists().await? {
            return Err(Error::NamespaceNotFound {
                name: namespace.to_string(),
            });
        }

        // Schema is required for table creation
        let Some(schema_path) = &args.schema else {
            return Err(Error::MissingArgument {
                argument: "--schema".to_string(),
                description: "Schema file is required for table creation".to_string(),
            });
        };

        // Read and parse schema file
        let schema_content = std::fs::read_to_string(schema_path).map_err(|e| Error::Parse {
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
