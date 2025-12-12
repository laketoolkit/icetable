//! Delete command implementation
//!
//! Deletes namespaces or tables from a catalog.
//! - `delete -n <namespace>` deletes a namespace (must be empty or use --force)
//! - `delete table1 table2 ...` deletes one or more tables

use colored::Colorize;

use crate::cli::parser::DeleteArgs;
use crate::config::Config;
use crate::core::catalog::RestCatalogClient;
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Handler for delete command
pub struct DeleteCommand;

impl DeleteCommand {
    /// Execute delete command
    pub async fn execute(args: DeleteArgs) -> Result<()> {
        const ESTIMATED_MEMORY: u64 = 32 * 1024 * 1024;
        with_resource_limits(ESTIMATED_MEMORY, Self::execute_inner(args)).await
    }

    async fn execute_inner(args: DeleteArgs) -> Result<()> {
        let config = Config::load()?;

        // Get catalog name from args or context
        let catalog_name = args
            .catalog
            .clone()
            .or_else(|| config.get_current_catalog().map(String::from));

        let Some(catalog_name) = catalog_name else {
            println!("{} No catalog specified", "!".yellow());
            println!(
                "  Use {} or specify {}",
                "icetable config use <catalog>".dimmed(),
                "-c <catalog>".dimmed()
            );
            return Ok(());
        };

        // Get catalog config
        let Some(catalog_config) = config.catalogs.get(&catalog_name) else {
            println!(
                "{} Catalog not found: {}",
                "!".yellow(),
                catalog_name.cyan()
            );
            return Ok(());
        };

        // Get namespace from args, context, or catalog default
        let namespace = args
            .namespace
            .clone()
            .or_else(|| config.get_current_namespace())
            .or_else(|| catalog_config.default_namespace.clone());

        // Must have namespace
        let Some(namespace) = namespace else {
            println!("{} No namespace specified", "!".yellow());
            println!(
                "  Use {} or specify {}",
                "icetable config use <catalog> -n <namespace>".dimmed(),
                "-n <namespace>".dimmed()
            );
            return Ok(());
        };

        // Create REST client
        let client = RestCatalogClient::new(catalog_config).await?;

        // If tables provided, delete them
        if !args.tables.is_empty() {
            Self::delete_tables(&client, &namespace, &args.tables, args.purge).await
        } else {
            // No tables - delete namespace
            Self::delete_namespace(&client, &namespace, args.force).await
        }
    }

    async fn delete_namespace(
        client: &RestCatalogClient,
        namespace: &str,
        force: bool,
    ) -> Result<()> {
        let ns_parts: Vec<String> = namespace.split('.').map(String::from).collect();

        // Check if namespace has tables
        let tables = client.list_tables(&ns_parts).await?;

        if !tables.is_empty() && !force {
            return Err(Error::General(format!(
                "Namespace '{}' contains {} table(s). Use --force to delete anyway.",
                namespace,
                tables.len()
            )));
        }

        client.delete_namespace(&ns_parts).await?;

        println!(
            "{} Deleted namespace: {}",
            "✓".green(),
            namespace.cyan()
        );

        Ok(())
    }

    async fn delete_tables(
        client: &RestCatalogClient,
        namespace: &str,
        tables: &[String],
        purge: bool,
    ) -> Result<()> {
        let ns_parts: Vec<String> = namespace.split('.').map(String::from).collect();
        let purge_msg = if purge { " (data purged)" } else { "" };

        let mut errors = Vec::new();

        for table_name in tables {
            match client.delete_table(&ns_parts, table_name, purge).await {
                Ok(()) => {
                    println!(
                        "{} Deleted: {}.{}{}",
                        "✓".green(),
                        namespace.cyan(),
                        table_name.cyan(),
                        purge_msg.dimmed()
                    );
                }
                Err(e) => {
                    println!(
                        "{} Failed: {}.{} - {}",
                        "✗".red(),
                        namespace,
                        table_name,
                        e
                    );
                    errors.push(format!("{}: {}", table_name, e));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(Error::General(format!(
                "Failed to delete {} table(s)",
                errors.len()
            )))
        }
    }
}
