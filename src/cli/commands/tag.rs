//! Tag command implementation
//!
//! Manages tags for Iceberg tables.

use colored::Colorize;

use super::common::{resolve_table, TableResolution};
use crate::cli::parser::{TagArgs, TagCommands};
use crate::core::catalog::TableCommitter;
use crate::core::maintenance::{RefConfig, RefService};
use crate::core::{CatalogConfig, TableContext};
use crate::error::{Error, Result};

/// Handler for tag command
pub struct TagCommand;

impl TagCommand {
    /// Execute tag command
    pub async fn execute(args: TagArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        match args.command {
            TagCommands::List(a) => {
                let resolution = resolve_table(&a.path, catalog_config.as_ref()).await?;
                let ctx = TableContext::from_path(Some(resolution.location().to_string())).await?;
                ctx.require_iceberg()?;
                Self::list(&ctx, &a.output).await
            }
            TagCommands::Create(a) => {
                let resolution = resolve_table(&a.path, catalog_config.as_ref()).await?;
                let ctx = TableContext::from_path(Some(resolution.location().to_string())).await?;
                ctx.require_iceberg()?;
                let committer = Self::create_committer(catalog_config.as_ref(), &resolution);
                Self::create(&ctx, &a.name, a.snapshot_id, a.max_ref_age_ms, &a.output, committer).await
            }
            TagCommands::Delete(a) => {
                let resolution = resolve_table(&a.path, catalog_config.as_ref()).await?;
                let ctx = TableContext::from_path(Some(resolution.location().to_string())).await?;
                ctx.require_iceberg()?;
                let committer = Self::create_committer(catalog_config.as_ref(), &resolution);
                Self::delete(&ctx, &a.name, a.dry_run, &a.output, committer).await
            }
            TagCommands::Rename(a) => {
                let resolution = resolve_table(&a.path, catalog_config.as_ref()).await?;
                let ctx = TableContext::from_path(Some(resolution.location().to_string())).await?;
                ctx.require_iceberg()?;
                let committer = Self::create_committer(catalog_config.as_ref(), &resolution);
                Self::rename(&ctx, &a.old_name, &a.new_name, &a.output, committer).await
            }
        }
    }

    /// Create a TableCommitter if catalog is configured
    fn create_committer(
        catalog_config: Option<&CatalogConfig>,
        resolution: &TableResolution,
    ) -> Option<TableCommitter> {
        match (catalog_config, resolution) {
            (Some(config), TableResolution::CatalogTable { namespace, name, .. }) => {
                Some(TableCommitter::with_catalog(
                    config.clone(),
                    namespace.clone(),
                    name.clone(),
                ))
            }
            _ => None,
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
            println!(
                "{}",
                serde_json::to_string_pretty(&tag_json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!("{} Iceberg tags at {}", "Listing".green(), ctx.path);
            println!();
            println!(
                "{:<20} {:<20}",
                "TAG".cyan(),
                "SNAPSHOT ID".cyan()
            );
            println!("{}", "-".repeat(40));

            if tags.is_empty() {
                println!("{}", "No tags found".dimmed());
            } else {
                for tag in &tags {
                    println!("{:<20} {:<20}", tag.name, tag.snapshot_id);
                }
            }
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
        let ref_service = match committer {
            Some(c) => RefService::with_committer(c),
            None => RefService::new(),
        };

        let result = ref_service
            .create_tag(&service, &ctx.path, name, snapshot_id, max_ref_age_ms)
            .await?;

        if output == "json" {
            let json = serde_json::json!({
                "name": result.name,
                "snapshot_id": result.snapshot_id,
                "new_version": result.new_version,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!(
                "{} Created tag '{}' at snapshot {}",
                "Success:".green(),
                result.name.cyan(),
                result.snapshot_id
            );
            if let Some(v) = result.new_version {
                println!("New metadata version: v{}", v);
            }
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
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else if result.dry_run {
            println!("{}", "DRY RUN - No changes made".yellow().bold());
            println!();
            println!("Would delete the following:");
            println!(
                "  Tag: {} (snapshot {})",
                result.name.cyan(),
                result.snapshot_id
            );
            println!();
            println!(
                "{}",
                "Run without --dry-run to apply this change.".dimmed()
            );
        } else {
            println!(
                "{} Deleted tag '{}'",
                "Success:".green(),
                result.name.red()
            );
            if let Some(v) = result.new_version {
                println!("New metadata version: v{}", v);
            }
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
        let ref_service = match committer {
            Some(c) => RefService::with_committer(c),
            None => RefService::new(),
        };

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
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!(
                "{} Renamed tag '{}' to '{}'",
                "Success:".green(),
                old_name.yellow(),
                result.name.cyan()
            );
            if let Some(v) = result.new_version {
                println!("New metadata version: v{}", v);
            }
        }

        Ok(())
    }
}
