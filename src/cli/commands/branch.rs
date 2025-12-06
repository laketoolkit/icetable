//! Branch command implementation
//!
//! Manages branches for Iceberg tables.

use colored::Colorize;

use crate::cli::parser::{BranchArgs, BranchCommands};
use crate::core::maintenance::{RefConfig, RefService};
use crate::core::TableContext;
use crate::error::{Error, Result};

/// Handler for branch command
pub struct BranchCommand;

impl BranchCommand {
    /// Execute branch command
    pub async fn execute(args: BranchArgs) -> Result<()> {
        match args.command {
            BranchCommands::List(a) => {
                let ctx = TableContext::from_path(a.path).await?;
                ctx.require_iceberg()?;
                Self::list(&ctx, &a.output).await
            }
            BranchCommands::Create(a) => {
                let ctx = TableContext::from_path(a.path).await?;
                ctx.require_iceberg()?;
                Self::create(
                    &ctx,
                    &a.name,
                    a.from_snapshot,
                    a.max_ref_age_ms,
                    a.min_snapshots_to_keep,
                    a.max_snapshot_age_ms,
                    &a.output,
                )
                .await
            }
            BranchCommands::Delete(a) => {
                let ctx = TableContext::from_path(a.path).await?;
                ctx.require_iceberg()?;
                Self::delete(&ctx, &a.name, a.dry_run, &a.output).await
            }
            BranchCommands::FastForward(a) => {
                let ctx = TableContext::from_path(a.path).await?;
                ctx.require_iceberg()?;
                Self::fast_forward(&ctx, &a.name, &a.to, &a.output).await
            }
            BranchCommands::Rename(a) => {
                let ctx = TableContext::from_path(a.path).await?;
                ctx.require_iceberg()?;
                Self::rename(&ctx, &a.old_name, &a.new_name, &a.output).await
            }
        }
    }

    async fn list(ctx: &TableContext, output: &str) -> Result<()> {
        let service = ctx.iceberg_service().await?;
        let refs = service.list_refs().await?;
        let (metadata, _) = ctx.iceberg_metadata().await?;
        let current_snapshot_id = metadata.current_snapshot_id();

        // Filter to branches only
        let branches: Vec<_> = refs.iter().filter(|r| r.ref_type == "branch").collect();

        if output == "json" {
            let branch_json: Vec<serde_json::Value> = branches
                .iter()
                .map(|b| {
                    serde_json::json!({
                        "name": b.name,
                        "snapshot_id": b.snapshot_id,
                        "is_current": Some(b.snapshot_id) == current_snapshot_id && b.name == "main",
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&branch_json)
                    .map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!("{} Iceberg branches at {}", "Listing".green(), ctx.path);
            println!();
            println!(
                "{:<20} {:<20} {}",
                "BRANCH".cyan(),
                "SNAPSHOT ID".cyan(),
                "".cyan()
            );
            println!("{}", "-".repeat(50));

            if branches.is_empty() {
                println!("{}", "No branches found".dimmed());
            } else {
                for branch in &branches {
                    let is_main_current =
                        branch.name == "main" && Some(branch.snapshot_id) == current_snapshot_id;
                    let marker = if is_main_current {
                        "(current)".green().to_string()
                    } else {
                        "".to_string()
                    };
                    println!("{:<20} {:<20} {}", branch.name, branch.snapshot_id, marker);
                }
            }
        }

        Ok(())
    }

    async fn create(
        ctx: &TableContext,
        name: &str,
        from_snapshot: Option<i64>,
        max_ref_age_ms: Option<i64>,
        min_snapshots_to_keep: Option<i32>,
        max_snapshot_age_ms: Option<i64>,
        output: &str,
    ) -> Result<()> {
        let service = ctx.iceberg_service().await?;
        let ref_service = RefService::new();

        let result = ref_service
            .create_branch(
                &service,
                &ctx.path,
                name,
                from_snapshot,
                min_snapshots_to_keep,
                max_snapshot_age_ms,
                max_ref_age_ms,
            )
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
                "{} Created branch '{}' at snapshot {}",
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

    async fn delete(ctx: &TableContext, name: &str, dry_run: bool, output: &str) -> Result<()> {
        let service = ctx.iceberg_service().await?;
        let config = RefConfig { dry_run };
        let ref_service = RefService::with_config(config);

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
                "  Branch: {} (snapshot {})",
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
                "{} Deleted branch '{}'",
                "Success:".green(),
                result.name.red()
            );
            if let Some(v) = result.new_version {
                println!("New metadata version: v{}", v);
            }
        }

        Ok(())
    }

    async fn fast_forward(ctx: &TableContext, name: &str, to: &str, output: &str) -> Result<()> {
        let service = ctx.iceberg_service().await?;
        let ref_service = RefService::new();

        let result = ref_service
            .fast_forward_branch(&service, &ctx.path, name, to)
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
                "{} Fast-forwarded branch '{}' to snapshot {}",
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

    async fn rename(ctx: &TableContext, old_name: &str, new_name: &str, output: &str) -> Result<()> {
        let service = ctx.iceberg_service().await?;
        let ref_service = RefService::new();

        let result = ref_service
            .rename_branch(&service, &ctx.path, old_name, new_name)
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
                "{} Renamed branch '{}' to '{}'",
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
