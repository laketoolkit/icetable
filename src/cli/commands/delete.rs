//! Delete command implementation
//!
//! Explicit subcommands:
//! - `delete namespace <name>` - delete a namespace (must be empty or use --force)
//! - `delete table <name> [<name>...]` - delete one or more tables

use colored::Colorize;

use super::common::resolve_catalog;
use crate::cli::parser::{
    CliTableContext, DeleteArgs, DeleteCommands, NamespaceDeleteArgs, TableDeleteArgs,
};
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
        match args.command {
            DeleteCommands::Namespace(ns_args) => Self::delete_namespace(ns_args, ctx).await,
            DeleteCommands::Table(tbl_args) => Self::delete_tables(tbl_args, ctx).await,
        }
    }

    async fn delete_namespace(args: NamespaceDeleteArgs, ctx: &CliTableContext) -> Result<()> {
        // Create a modified context with the namespace from args
        let mut ctx = ctx.clone();
        ctx.namespace = Some(args.name.clone());

        let catalog = resolve_catalog(&ctx).await?;
        let namespace = catalog.namespace().unwrap(); // Safe: we just set it

        // Check if namespace has tables
        let tables = catalog.list_tables().await?;

        if !tables.is_empty() && !args.force {
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

    async fn delete_tables(args: TableDeleteArgs, ctx: &CliTableContext) -> Result<()> {
        let catalog = resolve_catalog(ctx).await?;

        // Must have namespace
        let namespace = catalog.namespace().ok_or_else(|| Error::MissingArgument {
            argument: "-n/--namespace".to_string(),
            description:
                "Namespace required to delete tables. Use -n or set context with 'icetable config use'"
                    .to_string(),
        })?;

        let purge_msg = if args.purge { " (data purged)" } else { "" };

        let mut errors = Vec::new();

        for table_name in &args.names {
            match catalog.delete_table(table_name, args.purge).await {
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
