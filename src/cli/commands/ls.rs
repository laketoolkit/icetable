//! List command implementation
//!
//! Context-aware listing:
//! - If context is a table → show table info (snapshots, branches, tags)
//! - If context is a namespace → show tables
//! - If context is a catalog → show namespaces

use colored::Colorize;

use super::common::print_json;
use crate::cli::parser::LsArgs;
use crate::config::Config;
use crate::core::catalog::RestCatalogClient;
use crate::core::metadata::{IcebergMetadataService, MetadataService};
use crate::error::Result;

/// Tree drawing characters
const TREE_BRANCH: &str = "├── ";
const TREE_LAST: &str = "└── ";
const TREE_INDENT: &str = "│   ";

/// Handler for ls command
pub struct LsCommand;

impl LsCommand {
    /// Execute ls command
    pub async fn execute(args: LsArgs) -> Result<()> {
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

        // Check if context points to a table (and no explicit args override)
        let table_from_context = if args.catalog.is_none() && args.namespace.is_none() {
            config.get_current_table().map(String::from)
        } else {
            None
        };

        // Get namespace from args or context
        let namespace = args.namespace.or_else(|| config.get_current_namespace());

        // Create REST client
        let client = RestCatalogClient::new(catalog_config).await?;

        // If we have a table in context, show table info
        if let Some(table_name) = table_from_context {
            if let Some(ref ns) = namespace {
                return Self::show_table_info(&client, &catalog_name, ns, &table_name, &args.output).await;
            }
        }

        // Otherwise, list tables or namespaces
        if let Some(ref ns) = namespace {
            // List tables in namespace
            let ns_parts: Vec<String> = ns.split('.').map(String::from).collect();
            let tables = client.list_tables(&ns_parts).await?;

            if args.output == "json" {
                let json = serde_json::json!({
                    "catalog": catalog_name,
                    "namespace": ns,
                    "tables": tables,
                });
                print_json(&json)?;
            } else {
                Self::print_tables_tree(&catalog_name, ns, &tables);
            }
        } else {
            // List namespaces
            let namespaces = client.list_namespaces(None).await?;

            if args.output == "json" {
                let json = serde_json::json!({
                    "catalog": catalog_name,
                    "namespaces": namespaces.iter().map(|ns| ns.join(".")).collect::<Vec<_>>(),
                });
                print_json(&json)?;
            } else {
                Self::print_namespaces_tree(&catalog_name, &namespaces);
            }
        }

        Ok(())
    }

    /// Show table information (snapshots, branches, tags)
    async fn show_table_info(
        client: &RestCatalogClient,
        catalog: &str,
        namespace: &str,
        table_name: &str,
        output: &str,
    ) -> Result<()> {
        let ns_parts: Vec<String> = namespace.split('.').map(String::from).collect();
        let table = client.load_table(&ns_parts, table_name).await?;
        let location = table.metadata().location();

        // Load metadata service for detailed info
        let metadata = IcebergMetadataService::new_async(location.to_string()).await?;
        let snapshots = metadata.list_snapshots(None).await.unwrap_or_default();
        let refs = metadata.list_refs().await.unwrap_or_default();

        // Separate branches and tags
        let branches: Vec<_> = refs.iter().filter(|r| r.ref_type == "branch").collect();
        let tags: Vec<_> = refs.iter().filter(|r| r.ref_type == "tag").collect();

        if output == "json" {
            let json = serde_json::json!({
                "catalog": catalog,
                "namespace": namespace,
                "table": table_name,
                "location": location,
                "snapshots": snapshots.len(),
                "branches": branches.iter().map(|b| &b.name).collect::<Vec<_>>(),
                "tags": tags.iter().map(|t| &t.name).collect::<Vec<_>>(),
            });
            print_json(&json)?;
        } else {
            Self::print_table_info_tree(catalog, namespace, table_name, &snapshots, &branches, &tags);
        }

        Ok(())
    }

    /// Print table info in tree format
    fn print_table_info_tree(
        catalog: &str,
        namespace: &str,
        table: &str,
        snapshots: &[crate::core::metadata::SnapshotInfo],
        branches: &[&crate::core::metadata::RefInfo],
        tags: &[&crate::core::metadata::RefInfo],
    ) {
        println!(
            "{}.{}.{}",
            catalog.cyan(),
            namespace.cyan(),
            table.cyan().bold()
        );

        // Snapshots
        let is_last_section = branches.is_empty() && tags.is_empty();
        let prefix = if is_last_section { TREE_LAST } else { TREE_BRANCH };
        println!(
            "{}{} {}",
            prefix,
            "snapshots".yellow(),
            format!("({})", snapshots.len()).dimmed()
        );

        // Show last 3 snapshots
        let recent: Vec<_> = snapshots.iter().take(3).collect();
        let indent = if is_last_section { "    " } else { TREE_INDENT };
        for (i, snap) in recent.iter().enumerate() {
            let snap_prefix = if i == recent.len() - 1 { TREE_LAST } else { TREE_BRANCH };
            println!(
                "{}{}#{} {}",
                indent,
                snap_prefix,
                snap.id.to_string().dimmed(),
                snap.operation.dimmed()
            );
        }
        if snapshots.len() > 3 {
            println!("{}{}... and {} more", indent, TREE_LAST, snapshots.len() - 3);
        }

        // Branches
        if !branches.is_empty() || !tags.is_empty() {
            let is_last_section = tags.is_empty();
            let prefix = if is_last_section { TREE_LAST } else { TREE_BRANCH };
            println!(
                "{}{} {}",
                prefix,
                "branches".yellow(),
                format!("({})", branches.len()).dimmed()
            );

            let indent = if is_last_section { "    " } else { TREE_INDENT };
            for (i, branch) in branches.iter().enumerate() {
                let is_last = i == branches.len() - 1;
                let branch_prefix = if is_last { TREE_LAST } else { TREE_BRANCH };
                let current_marker = if branch.name == "main" { " *" } else { "" };
                println!("{}{}{}{}", indent, branch_prefix, branch.name, current_marker.green());
            }
        }

        // Tags
        if !tags.is_empty() {
            println!(
                "{}{} {}",
                TREE_LAST,
                "tags".yellow(),
                format!("({})", tags.len()).dimmed()
            );

            for (i, tag) in tags.iter().enumerate() {
                let is_last = i == tags.len() - 1;
                let tag_prefix = if is_last { TREE_LAST } else { TREE_BRANCH };
                println!("    {}{}", tag_prefix, tag.name);
            }
        }
    }

    /// Print namespaces in tree format
    fn print_namespaces_tree(catalog: &str, namespaces: &[Vec<String>]) {
        // Header: catalog (count)
        println!(
            "{} {}",
            catalog.cyan(),
            format!("({})", namespaces.len()).dimmed()
        );

        if namespaces.is_empty() {
            println!("{}{}", TREE_LAST, "(empty)".dimmed());
            return;
        }

        let len = namespaces.len();
        for (i, ns) in namespaces.iter().enumerate() {
            let is_last = i == len - 1;
            let prefix = if is_last { TREE_LAST } else { TREE_BRANCH };
            println!("{}{}", prefix, ns.join("."));
        }
    }

    /// Print tables in tree format
    fn print_tables_tree(catalog: &str, namespace: &str, tables: &[String]) {
        // Header: catalog.namespace (count)
        println!(
            "{}.{} {}",
            catalog.cyan(),
            namespace.cyan(),
            format!("({})", tables.len()).dimmed()
        );

        if tables.is_empty() {
            println!("{}{}", TREE_LAST, "(empty)".dimmed());
            return;
        }

        let len = tables.len();
        for (i, table) in tables.iter().enumerate() {
            let is_last = i == len - 1;
            let prefix = if is_last { TREE_LAST } else { TREE_BRANCH };
            println!("{}{}", prefix, table);
        }
    }
}
