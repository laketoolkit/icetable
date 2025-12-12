//! Diff command implementation
//!
//! Compares snapshots, branches, or tags within a table.
//! Thin wrapper that delegates to DiffService in core.

use colored::Colorize;

use super::common::{print_json, resolve_table_from_context};
use crate::cli::output::format_timestamp_ms;
use crate::cli::parser::{CliTableContext, DiffArgs};
use crate::core::extract_filename;
use crate::core::operations::{DiffConfig, DiffService, SnapshotDiffResult};
use crate::error::Result;
use crate::utils::with_resource_limits;

/// Handler for diff command
pub struct DiffCommand;

impl DiffCommand {
    /// Execute diff command
    pub async fn execute(args: DiffArgs, ctx: &CliTableContext) -> Result<()> {
        const ESTIMATED_MEMORY: u64 = 128 * 1024 * 1024; // 128MB for diff operations
        with_resource_limits(ESTIMATED_MEMORY, Self::execute_inner(args, ctx)).await
    }

    async fn execute_inner(args: DiffArgs, ctx: &CliTableContext) -> Result<()> {
        // 1. Resolve table (supports catalog resolution) and create metadata service
        let resolution = resolve_table_from_context(ctx).await?;
        let service = resolution.to_readonly_service().await?;

        // 2. Build config and delegate to service
        let config = DiffConfig {
            reference: args.reference.clone(),
            base: args.base.clone(),
        };

        let result = DiffService::compare_snapshots(&service, &config).await?;

        // 3. Output
        Self::output(&result, &args)
    }

    /// Output diff result in the requested format
    fn output(result: &SnapshotDiffResult, args: &DiffArgs) -> Result<()> {
        if args.output == "json" {
            Self::output_json(result)
        } else {
            Self::output_table(result, args)
        }
    }

    /// Output diff as formatted table
    fn output_table(result: &SnapshotDiffResult, args: &DiffArgs) -> Result<()> {
        if result.is_identical {
            println!("{}", "References point to the same snapshot".yellow());
            return Ok(());
        }

        let ref_label = args.reference.as_deref().unwrap_or("current");
        let base_label = args.base.as_deref().unwrap_or("parent");

        println!(
            "{} {} (base: {})",
            "Comparing".green(),
            ref_label,
            base_label
        );
        println!();
        println!(
            "{:<20} {:<20} {:<20} {}",
            "REF".cyan(),
            "SNAPSHOT".cyan(),
            "TIMESTAMP".cyan(),
            "MANIFESTS".cyan()
        );
        println!("{}", "-".repeat(75));
        println!(
            "{:<20} {:<20} {:<20} {}",
            base_label,
            result.base.snapshot_id,
            format_timestamp_ms(result.base.timestamp_ms),
            result.base.manifest_count
        );
        println!(
            "{:<20} {:<20} {:<20} {}",
            ref_label,
            result.reference.snapshot_id,
            format_timestamp_ms(result.reference.timestamp_ms),
            result.reference.manifest_count
        );
        println!();

        if result.manifests_added.is_empty() && result.manifests_removed.is_empty() {
            println!("{}", "No manifest changes".yellow());
        } else {
            println!("Changes:");
            for path in &result.manifests_added {
                let filename = extract_filename(path);
                println!("  {} {}", "+".green(), filename);
            }
            for path in &result.manifests_removed {
                let filename = extract_filename(path);
                println!("  {} {}", "-".red(), filename);
            }
            println!();
            println!(
                "Summary: {} added, {} removed",
                result.manifests_added.len().to_string().green(),
                result.manifests_removed.len().to_string().red()
            );
        }

        Ok(())
    }

    /// Output diff as JSON
    fn output_json(result: &SnapshotDiffResult) -> Result<()> {
        if result.is_identical {
            let json = serde_json::json!({
                "reference": result.reference.snapshot_id,
                "base": result.base.snapshot_id,
                "identical": true,
            });
            print_json(&json)?;
            return Ok(());
        }

        let json = serde_json::json!({
            "base": {
                "ref": result.base.label,
                "snapshot_id": result.base.snapshot_id,
                "timestamp": format_timestamp_ms(result.base.timestamp_ms),
                "manifest_count": result.base.manifest_count,
            },
            "reference": {
                "ref": result.reference.label,
                "snapshot_id": result.reference.snapshot_id,
                "timestamp": format_timestamp_ms(result.reference.timestamp_ms),
                "manifest_count": result.reference.manifest_count,
            },
            "diff": {
                "manifests_added": result.manifests_added.len(),
                "manifests_removed": result.manifests_removed.len(),
                "added_paths": result.manifests_added,
                "removed_paths": result.manifests_removed,
            }
        });
        print_json(&json)?;

        Ok(())
    }
}
