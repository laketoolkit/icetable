//! Branch command implementation
//!
//! Manages branches for Iceberg tables.
//! This is a thin wrapper that delegates to core services.

use colored::Colorize;

use crate::cli::parser::{BranchArgs, BranchCommands};
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
        }
    }

    async fn list(ctx: &TableContext, output: &str) -> Result<()> {
        println!("{} Iceberg branches at {}", "Listing".green(), ctx.path);
        println!();

        let (metadata, _) = ctx.iceberg_metadata().await?;
        let current_snapshot_id = metadata.current_snapshot_id();

        if output == "json" {
            let branch_json = serde_json::json!([{
                "name": "main",
                "snapshot_id": current_snapshot_id,
                "is_current": true,
            }]);
            println!(
                "{}",
                serde_json::to_string_pretty(&branch_json)
                    .map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!(
                "{:<20} {:<20} {}",
                "BRANCH".cyan(),
                "SNAPSHOT ID".cyan(),
                "".cyan()
            );
            println!("{}", "-".repeat(50));

            if let Some(snap_id) = current_snapshot_id {
                println!("{:<20} {:<20} {}", "main", snap_id, " (current)".green());
            } else {
                println!("{:<20} {:<20}", "main", "-");
            }
        }

        println!();
        println!(
            "{}",
            "Note: Additional branches require catalog access".dimmed()
        );

        Ok(())
    }

    async fn create(
        ctx: &TableContext,
        name: &str,
        from_snapshot: Option<i64>,
        max_ref_age_ms: Option<i64>,
        min_snapshots_to_keep: Option<i32>,
        max_snapshot_age_ms: Option<i64>,
        _output: &str,
    ) -> Result<()> {
        println!("{} branch '{}' at {}", "Creating".green(), name, ctx.path);

        let (metadata, _) = ctx.iceberg_metadata().await?;

        // Get snapshot ID to branch from
        let snapshot_id = if let Some(id) = from_snapshot {
            let snapshots: Vec<_> = metadata.snapshots().collect();
            if !snapshots.iter().any(|s| s.snapshot_id() == id) {
                return Err(Error::General(format!("Snapshot {} not found", id)));
            }
            id
        } else {
            metadata
                .current_snapshot_id()
                .ok_or_else(|| Error::General("No current snapshot".to_string()))?
        };

        println!();
        println!("Branch: {}", name.cyan());
        println!("From snapshot: {}", snapshot_id);
        if let Some(max_age) = max_ref_age_ms {
            println!("Max ref age: {} ms", max_age);
        }
        if let Some(min_snaps) = min_snapshots_to_keep {
            println!("Min snapshots to keep: {}", min_snaps);
        }
        if let Some(max_snap_age) = max_snapshot_age_ms {
            println!("Max snapshot age: {} ms", max_snap_age);
        }

        println!();
        println!(
            "{}",
            "Branch creation requires iceberg-rs Transaction API".yellow()
        );
        println!(
            "{}",
            "This is a planned feature - use catalog tools for now".dimmed()
        );

        Ok(())
    }

    async fn delete(ctx: &TableContext, name: &str, dry_run: bool, _output: &str) -> Result<()> {
        println!(
            "{} branch '{}' at {}",
            if dry_run { "Analyzing" } else { "Deleting" }.green(),
            name,
            ctx.path
        );

        if name == "main" {
            return Err(Error::General("Cannot delete 'main' branch".to_string()));
        }

        println!();
        println!("Branch to delete: {}", name.red());

        if dry_run {
            println!();
            println!("{}", "DRY RUN - No changes made".yellow().bold());
            return Ok(());
        }

        println!();
        println!(
            "{}",
            "Branch deletion requires iceberg-rs Transaction API".yellow()
        );
        println!(
            "{}",
            "This is a planned feature - use catalog tools for now".dimmed()
        );

        Ok(())
    }

    async fn fast_forward(ctx: &TableContext, name: &str, to: &str, _output: &str) -> Result<()> {
        println!(
            "{} branch '{}' to {} at {}",
            "Fast-forwarding".green(),
            name,
            to,
            ctx.path
        );

        let (metadata, _) = ctx.iceberg_metadata().await?;

        // Parse target - could be snapshot ID or branch name
        let target_snapshot_id: i64 = if let Ok(id) = to.parse() {
            let snapshots: Vec<_> = metadata.snapshots().collect();
            if !snapshots.iter().any(|s| s.snapshot_id() == id) {
                return Err(Error::General(format!("Snapshot {} not found", id)));
            }
            id
        } else if let Some(snap) = metadata.snapshot_for_ref(to) {
            snap.snapshot_id()
        } else {
            return Err(Error::General(format!("Reference '{}' not found", to)));
        };

        println!();
        println!("Branch: {}", name.cyan());
        println!("Target snapshot: {}", target_snapshot_id);

        println!();
        println!(
            "{}",
            "Fast-forward requires iceberg-rs Transaction API".yellow()
        );
        println!(
            "{}",
            "This is a planned feature - use catalog tools for now".dimmed()
        );

        Ok(())
    }
}
