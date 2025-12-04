//! Tag command implementation
//!
//! Manages tags for Iceberg tables.
//! This is a thin wrapper that delegates to core services.

use colored::Colorize;

use crate::cli::parser::{TagArgs, TagCommands};
use crate::core::TableContext;
use crate::error::{Error, Result};

/// Handler for tag command
pub struct TagCommand;

impl TagCommand {
    /// Execute tag command
    pub async fn execute(args: TagArgs) -> Result<()> {
        match args.command {
            TagCommands::List(a) => {
                let ctx = TableContext::from_path(a.path).await?;
                ctx.require_iceberg()?;
                Self::list(&ctx, &a.output).await
            }
            TagCommands::Create(a) => {
                let ctx = TableContext::from_path(a.path).await?;
                ctx.require_iceberg()?;
                Self::create(&ctx, &a.name, a.snapshot_id, a.max_ref_age_ms, &a.output).await
            }
            TagCommands::Delete(a) => {
                let ctx = TableContext::from_path(a.path).await?;
                ctx.require_iceberg()?;
                Self::delete(&ctx, &a.name, a.force, &a.output).await
            }
        }
    }

    async fn list(ctx: &TableContext, output: &str) -> Result<()> {
        println!("{} Iceberg tags at {}", "Listing".green(), ctx.path);
        println!();

        // iceberg-rs doesn't expose refs() directly to list tags
        // Tags require catalog access to enumerate
        if output == "json" {
            let tag_json = serde_json::json!([]);
            println!(
                "{}",
                serde_json::to_string_pretty(&tag_json)
                    .map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!("{:<30} {:<20}", "TAG".cyan(), "SNAPSHOT ID".cyan());
            println!("{}", "-".repeat(50));
            println!("{}", "No tags found".yellow());
        }

        println!();
        println!("{}", "Note: Tag listing requires catalog access".dimmed());

        Ok(())
    }

    async fn create(
        ctx: &TableContext,
        name: &str,
        snapshot_id: Option<i64>,
        max_ref_age_ms: Option<i64>,
        _output: &str,
    ) -> Result<()> {
        println!("{} tag '{}' at {}", "Creating".green(), name, ctx.path);

        let (metadata, _) = ctx.iceberg_metadata().await?;

        // Get snapshot ID to tag
        let snap_id = if let Some(id) = snapshot_id {
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

        // Get snapshot timestamp for display
        let snapshots: Vec<_> = metadata.snapshots().collect();
        let snap = snapshots
            .iter()
            .find(|s| s.snapshot_id() == snap_id)
            .ok_or_else(|| Error::General(format!("Snapshot {} not found", snap_id)))?;

        let timestamp = chrono::DateTime::from_timestamp_millis(snap.timestamp_ms())
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| snap.timestamp_ms().to_string());

        println!();
        println!("Tag: {}", name.cyan());
        println!("Snapshot: {}", snap_id);
        println!("Snapshot timestamp: {}", timestamp);
        if let Some(max_age) = max_ref_age_ms {
            println!("Max ref age: {} ms", max_age);
        }

        println!();
        println!(
            "{}",
            "Tag creation requires iceberg-rs Transaction API".yellow()
        );
        println!(
            "{}",
            "This is a planned feature - use catalog tools for now".dimmed()
        );

        Ok(())
    }

    async fn delete(ctx: &TableContext, name: &str, force: bool, _output: &str) -> Result<()> {
        println!("{} tag '{}' at {}", "Deleting".green(), name, ctx.path);

        println!();
        println!("Tag to delete: {}", name.red());

        if !force {
            println!();
            println!("{}", "Use --force to confirm deletion".yellow());
            return Ok(());
        }

        println!();
        println!(
            "{}",
            "Tag deletion requires iceberg-rs Transaction API".yellow()
        );
        println!(
            "{}",
            "This is a planned feature - use catalog tools for now".dimmed()
        );

        Ok(())
    }
}
