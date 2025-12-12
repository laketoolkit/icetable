//! Delete command implementation
//!
//! Deletes namespaces or tables from a catalog.
//! - `delete -n <namespace>` deletes a namespace (must be empty or use --force)
//! - `delete table1 table2 ...` deletes one or more tables

use colored::Colorize;

use super::common::{CatalogResolution, no_namespace_error, resolve_catalog};
use crate::cli::parser::{CliTableContext, DeleteArgs};
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Handler for delete command
pub struct DeleteCommand;

impl DeleteCommand {
    /// Execute delete command
    pub async fn execute(args: DeleteArgs, ctx: &CliTableContext) -> Result<()> {
        const ESTIMATED_MEMORY: u64 = 32 * 1024 * 1024;
        with_resource_limits(ESTIMATED_MEMORY, Self::execute_inner(args, ctx)).await
    }

    async fn execute_inner(args: DeleteArgs, ctx: &CliTableContext) -> Result<()> {
        // Resolve catalog context (error propagates with full context)
        let catalog = resolve_catalog(ctx, args.catalog.as_deref()).await?;

        // Must have namespace
        let namespace = catalog.namespace().ok_or_else(no_namespace_error)?;

        // If tables provided, delete them
        if !args.tables.is_empty() {
            Self::delete_tables(&catalog, namespace, &args.tables, args.purge).await
        } else {
            // No tables - delete namespace
            Self::delete_namespace(&catalog, namespace, args.force).await
        }
    }

    async fn delete_namespace(
        catalog: &CatalogResolution,
        namespace: &str,
        force: bool,
    ) -> Result<()> {
        // Check if namespace has tables
        let tables = catalog.list_tables().await?;

        if !tables.is_empty() && !force {
            return Err(Error::CatalogOperation {
                message: format!(
                    "Namespace '{}' contains {} table(s). Use --force to delete anyway.",
                    namespace,
                    tables.len()
                ),
            });
        }

        catalog.delete_namespace().await?;

        println!("{} Deleted namespace: {}", "✓".green(), namespace.cyan());

        Ok(())
    }

    async fn delete_tables(
        catalog: &CatalogResolution,
        namespace: &str,
        tables: &[String],
        purge: bool,
    ) -> Result<()> {
        let purge_msg = if purge { " (data purged)" } else { "" };

        let mut errors = Vec::new();

        for table_name in tables {
            match catalog.delete_table(table_name, purge).await {
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
                    println!("{} Failed: {}.{} - {}", "✗".red(), namespace, table_name, e);
                    errors.push(format!("{}: {}", table_name, e));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(Error::CatalogOperation {
                message: format!("Failed to delete {} table(s)", errors.len()),
            })
        }
    }
}
