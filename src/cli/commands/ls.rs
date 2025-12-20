//! List command implementation
//!
//! Explicit subcommands:
//! - `ls namespaces` - list namespaces in catalog
//! - `ls tables` - list tables in namespace
//! - `ls` (no subcommand) - auto-detect from context

use super::common::{CatalogResolution, resolve_catalog};
use crate::cli::output::{LsFormatter, LsRefInfo, LsSnapshotInfo};
use crate::cli::parser::{CatalogContext, LsArgs, LsCommands};
use crate::core::metadata::{IcebergMetadataService, TableServiceReader};
use crate::error::{Error, Result};

/// Handler for ls command
pub struct LsCommand;

impl LsCommand {
    /// Execute ls command
    pub async fn execute(args: LsArgs, ctx: &CatalogContext) -> Result<()> {
        // Resolve catalog context
        let catalog = resolve_catalog(ctx).await?;

        match args.command {
            Some(LsCommands::Namespaces) => Self::list_namespaces(&catalog, &args.output).await,
            Some(LsCommands::Tables) => Self::list_tables(&catalog, &args.output).await,
            None => {
                // Auto-detect what to list based on context
                Self::auto_detect(&catalog, &args.output).await
            }
        }
    }

    /// List namespaces in catalog
    async fn list_namespaces(catalog: &CatalogResolution, output: &str) -> Result<()> {
        let namespaces = catalog.list_namespaces().await?;

        if output == "json" {
            let json_str = LsFormatter::format_namespaces_json(&catalog.catalog_name, &namespaces)
                .map_err(|e| Error::Serialization {
                    message: e.to_string(),
                })?;
            println!("{}", json_str);
        } else {
            println!(
                "{}",
                LsFormatter::format_namespaces_tree(&catalog.catalog_name, &namespaces)
            );
        }

        Ok(())
    }

    /// List tables in namespace
    async fn list_tables(catalog: &CatalogResolution, output: &str) -> Result<()> {
        let namespace = catalog.namespace().ok_or_else(|| Error::MissingArgument {
            argument: "-n/--namespace".to_string(),
            description:
                "Namespace required to list tables. Use -n or set context with 'icetable config use'"
                    .to_string(),
        })?;

        let tables = catalog.list_tables().await?;

        if output == "json" {
            let json_str =
                LsFormatter::format_tables_json(&catalog.catalog_name, namespace, &tables)
                    .map_err(|e| Error::Serialization {
                        message: e.to_string(),
                    })?;
            println!("{}", json_str);
        } else {
            println!(
                "{}",
                LsFormatter::format_tables_tree(&catalog.catalog_name, namespace, &tables)
            );
        }

        Ok(())
    }

    /// Auto-detect what to list based on context
    async fn auto_detect(catalog: &CatalogResolution, output: &str) -> Result<()> {
        // If we have a table in context, show table info
        if let (Some(table_name), Some(ns)) = (catalog.table(), catalog.namespace()) {
            return Self::show_table_info(catalog, ns, table_name, output).await;
        }

        // If we have a namespace, list tables
        if catalog.namespace().is_some() {
            return Self::list_tables(catalog, output).await;
        }

        // Otherwise, list namespaces
        Self::list_namespaces(catalog, output).await
    }

    /// Show table information (snapshots, branches, tags)
    async fn show_table_info(
        catalog: &CatalogResolution,
        namespace: &str,
        table_name: &str,
        output: &str,
    ) -> Result<()> {
        let table = catalog.load_table(table_name).await?;
        let location = table.metadata().location();

        // Create metadata service from catalog table
        let metadata_service = IcebergMetadataService::from_catalog_table_readonly(&table).await?;
        let snapshots = metadata_service
            .list_snapshots(None)
            .await
            .unwrap_or_default();
        let refs = metadata_service.list_refs().await.unwrap_or_default();

        // Separate branches and tags
        let branches: Vec<_> = refs.iter().filter(|r| r.ref_type == "branch").collect();
        let tags: Vec<_> = refs.iter().filter(|r| r.ref_type == "tag").collect();

        if output == "json" {
            let branch_names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
            let tag_names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();

            let json_str = LsFormatter::format_table_info_json(
                &catalog.catalog_name,
                namespace,
                table_name,
                location,
                snapshots.len(),
                &branch_names,
                &tag_names,
            )
            .map_err(|e| Error::Serialization {
                message: e.to_string(),
            })?;
            println!("{}", json_str);
        } else {
            // Convert to formatter types
            let snapshot_infos: Vec<LsSnapshotInfo> = snapshots
                .iter()
                .map(|s| LsSnapshotInfo {
                    id: s.id,
                    operation: s.operation.clone(),
                })
                .collect();

            let branch_infos: Vec<LsRefInfo> = branches
                .iter()
                .map(|b| LsRefInfo {
                    name: b.name.clone(),
                })
                .collect();

            let tag_infos: Vec<LsRefInfo> = tags
                .iter()
                .map(|t| LsRefInfo {
                    name: t.name.clone(),
                })
                .collect();

            println!(
                "{}",
                LsFormatter::format_table_info_tree(
                    &catalog.catalog_name,
                    namespace,
                    table_name,
                    &snapshot_infos,
                    &branch_infos,
                    &tag_infos,
                )
            );
        }

        Ok(())
    }
}
