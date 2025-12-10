//! Tag command implementation
//!
//! Manages tags for Iceberg tables.

use colored::Colorize;
use comfy_table::{Cell, CellAlignment};

use super::common::{create_committer, create_ref_service, create_table, print_json, print_ref_delete_dry_run, print_version_if_present, resolve_iceberg_context};
use crate::cli::parser::{TagArgs, TagCommands};
use crate::core::maintenance::{RefConfig, RefService};
use crate::core::{CatalogConfig, TableCommitter, TableContext};
use crate::error::Result;
use crate::utils::with_resource_limits;

/// Handler for tag command
pub struct TagCommand;

impl TagCommand {
    /// Execute tag command
    pub async fn execute(args: TagArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        const ESTIMATED_MEMORY: u64 = 32 * 1024 * 1024; // 32MB for tag ops
        with_resource_limits(ESTIMATED_MEMORY, Self::execute_inner(args, catalog_config)).await
    }

    async fn execute_inner(args: TagArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        match args.command {
            TagCommands::List(a) => {
                let iceberg = resolve_iceberg_context(&a.path, catalog_config.as_ref()).await?;
                Self::list(&iceberg.ctx, &a.output).await
            }
            TagCommands::Create(a) => {
                let iceberg = resolve_iceberg_context(&a.path, catalog_config.as_ref()).await?;
                let committer = create_committer(catalog_config.as_ref(), &iceberg.resolution);
                Self::create(
                    &iceberg.ctx,
                    &a.name,
                    a.snapshot_id,
                    a.max_ref_age_ms,
                    &a.output,
                    committer,
                )
                .await
            }
            TagCommands::Delete(a) => {
                let iceberg = resolve_iceberg_context(&a.path, catalog_config.as_ref()).await?;
                let committer = create_committer(catalog_config.as_ref(), &iceberg.resolution);
                Self::delete(&iceberg.ctx, &a.name, a.dry_run, &a.output, committer).await
            }
            TagCommands::Rename(a) => {
                let iceberg = resolve_iceberg_context(&a.path, catalog_config.as_ref()).await?;
                let committer = create_committer(catalog_config.as_ref(), &iceberg.resolution);
                Self::rename(&iceberg.ctx, &a.old_name, &a.new_name, &a.output, committer).await
            }
        }
    }

    async fn list(ctx: &TableContext, output: &str) -> Result<()> {
        let service = ctx.iceberg_service().await?;
        let refs = service.list_refs().await?;

        // Filter to tags only
        let tags: Vec<_> = refs.iter().filter(|r| r.ref_type == "tag").collect();

        if output == "json" {
            let tag_json: Vec<serde_json::Value> = tags
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "snapshot_id": t.snapshot_id,
                    })
                })
                .collect();
            print_json(&tag_json)?;
        } else if tags.is_empty() {
            println!("{}", "No tags found".dimmed());
        } else {
            let mut table = create_table();

            table.set_header(vec![
                Cell::new("Tag".cyan().to_string()).set_alignment(CellAlignment::Left),
                Cell::new("Snapshot ID".cyan().to_string()).set_alignment(CellAlignment::Right),
            ]);

            for tag in &tags {
                table.add_row(vec![
                    Cell::new(&tag.name).set_alignment(CellAlignment::Left),
                    Cell::new(tag.snapshot_id.to_string()).set_alignment(CellAlignment::Right),
                ]);
            }

            println!("{}", table);
        }

        Ok(())
    }

    async fn create(
        ctx: &TableContext,
        name: &str,
        snapshot_id: Option<i64>,
        max_ref_age_ms: Option<i64>,
        output: &str,
        committer: Option<TableCommitter>,
    ) -> Result<()> {
        let service = ctx.iceberg_service().await?;
        let ref_service = create_ref_service(committer);

        let result = ref_service
            .create_tag(&service, &ctx.path, name, snapshot_id, max_ref_age_ms)
            .await?;

        if output == "json" {
            let json = serde_json::json!({
                "name": result.name,
                "snapshot_id": result.snapshot_id,
                "new_version": result.new_version,
            });
            print_json(&json)?;
        } else {
            println!(
                "{} Created tag '{}' at snapshot {}",
                "Success:".green(),
                result.name.cyan(),
                result.snapshot_id
            );
            print_version_if_present(result.new_version);
        }

        Ok(())
    }

    async fn delete(
        ctx: &TableContext,
        name: &str,
        dry_run: bool,
        output: &str,
        committer: Option<TableCommitter>,
    ) -> Result<()> {
        let service = ctx.iceberg_service().await?;
        let config = RefConfig { dry_run };
        let ref_service = RefService::with_config_and_committer(config, committer);

        let result = ref_service.delete_ref(&service, &ctx.path, name).await?;

        if output == "json" {
            let json = serde_json::json!({
                "name": result.name,
                "snapshot_id": result.snapshot_id,
                "new_version": result.new_version,
                "dry_run": result.dry_run,
            });
            print_json(&json)?;
        } else if result.dry_run {
            print_ref_delete_dry_run("Tag", &result.name, result.snapshot_id);
        } else {
            println!("{} Deleted tag '{}'", "Success:".green(), result.name.red());
            print_version_if_present(result.new_version);
        }

        Ok(())
    }

    async fn rename(
        ctx: &TableContext,
        old_name: &str,
        new_name: &str,
        output: &str,
        committer: Option<TableCommitter>,
    ) -> Result<()> {
        let service = ctx.iceberg_service().await?;
        let ref_service = create_ref_service(committer);

        let result = ref_service
            .rename_tag(&service, &ctx.path, old_name, new_name)
            .await?;

        if output == "json" {
            let json = serde_json::json!({
                "old_name": old_name,
                "new_name": result.name,
                "snapshot_id": result.snapshot_id,
                "new_version": result.new_version,
            });
            print_json(&json)?;
        } else {
            println!(
                "{} Renamed tag '{}' to '{}'",
                "Success:".green(),
                old_name.yellow(),
                result.name.cyan()
            );
            print_version_if_present(result.new_version);
        }

        Ok(())
    }
}
