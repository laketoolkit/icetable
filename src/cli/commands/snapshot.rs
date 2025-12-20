//! Snapshot command implementation
//!
//! Manages snapshots for Iceberg tables.
//! Subcommands: list, create, expire, set, cherrypick

use colored::Colorize;

use super::common::{
    TableResolution, confirm_destructive, print_dry_run_header, resolve_table_from_context,
};
use crate::cli::output::{LineageEntry, SnapshotFormatter, SnapshotInfo};
use crate::cli::parser::{CatalogContext, SnapshotArgs, SnapshotCommands};
use crate::core::CatalogConfig;
use crate::core::maintenance::{SnapshotConfig, SnapshotService};
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Configuration for expire snapshots operation
struct ExpireConfig<'a> {
    older_than: Option<String>,
    retain_last: Option<usize>,
    id: Option<Vec<i64>>,
    dry_run: bool,
    force: bool,
    branch: Option<&'a str>,
    output: &'a str,
    resolution: &'a TableResolution,
    cli_catalog: Option<&'a CatalogConfig>,
}

/// Configuration for set snapshot operation
struct SetSnapshotConfig<'a> {
    id: Option<i64>,
    as_of: Option<String>,
    branch: Option<String>,
    tag: Option<String>,
    dry_run: bool,
    output: &'a str,
    resolution: &'a TableResolution,
    cli_catalog: Option<&'a CatalogConfig>,
}

/// Handler for snapshot command
pub struct SnapshotCommand;

impl SnapshotCommand {
    /// Execute snapshot command
    pub async fn execute(args: SnapshotArgs, ctx: &CatalogContext) -> Result<()> {
        use super::constants::MEMORY_MEDIUM_OPS;
        with_resource_limits(MEMORY_MEDIUM_OPS, Self::execute_inner(args, ctx)).await
    }

    async fn execute_inner(args: SnapshotArgs, ctx: &CatalogContext) -> Result<()> {
        // Resolve table to get path and catalog info (namespace/name if from catalog)
        let resolution = resolve_table_from_context(ctx).await?;
        let path = resolution.location();

        Self::execute_iceberg(args, &path, ctx.catalog_config.clone(), &resolution).await
    }

    async fn execute_iceberg(
        args: SnapshotArgs,
        table_path: &str,
        catalog_config: Option<CatalogConfig>,
        resolution: &TableResolution,
    ) -> Result<()> {
        match args.command {
            SnapshotCommands::Ls(a) => {
                Self::iceberg_list(
                    table_path,
                    a.limit,
                    a.all,
                    &a.output,
                    resolution,
                    catalog_config.as_ref(),
                )
                .await
            }
            SnapshotCommands::Create(a) => {
                Self::iceberg_create(table_path, a.force, &a.output).await
            }
            SnapshotCommands::Expire(a) => {
                // --all sets retain_last (defaults to 1, or use --keep N)
                let retain_last = if a.all { a.keep } else { None };

                let config = ExpireConfig {
                    older_than: a.older_than,
                    retain_last,
                    id: a.id,
                    dry_run: a.dry_run,
                    force: a.force,
                    branch: a.branch.as_deref(),
                    output: &a.output,
                    resolution,
                    cli_catalog: catalog_config.as_ref(),
                };
                Self::iceberg_expire(config).await
            }
            SnapshotCommands::Set(a) => {
                let config = SetSnapshotConfig {
                    id: a.id,
                    as_of: a.as_of,
                    branch: a.ref_branch,
                    tag: a.tag,
                    dry_run: a.dry_run,
                    output: &a.output,
                    resolution,
                    cli_catalog: catalog_config.as_ref(),
                };
                Self::iceberg_set(config).await
            }
            SnapshotCommands::Cherrypick(a) => {
                Self::iceberg_cherrypick(table_path, a.snapshot_id, &a.output).await
            }
            SnapshotCommands::Lineage(a) => {
                let limit = if a.all { None } else { Some(a.limit) };
                Self::iceberg_lineage(
                    table_path,
                    a.snapshot_id,
                    limit,
                    &a.output,
                    resolution,
                    catalog_config.as_ref(),
                )
                .await
            }
        }
    }

    async fn iceberg_list(
        path: &str,
        limit: usize,
        all: bool,
        output: &str,
        resolution: &TableResolution,
        cli_catalog: Option<&CatalogConfig>,
    ) -> Result<()> {
        println!("{} Iceberg snapshots at {}", "Listing".green(), path);
        println!();

        // Create metadata service using factory method - handles catalog vs path context automatically
        let metadata_service = resolution.to_readonly_service().await?;
        let _ = cli_catalog; // Used for write operations only
        let snapshot_service = SnapshotService::new();

        let limit = if all { None } else { Some(limit) };
        let result = snapshot_service
            .list_snapshots(&metadata_service, limit)
            .await?;

        // Convert to SnapshotInfo for formatting
        let snapshot_infos: Vec<SnapshotInfo> = result
            .snapshots
            .iter()
            .map(|snap| {
                let mut info = SnapshotInfo::new(snap.id, snap.timestamp)
                    .with_parent(snap.parent_id)
                    .with_current(snap.is_current);
                if let Some(ref op) = snap.operation {
                    info = info.with_operation(op.clone());
                }
                info
            })
            .collect();

        if output == "json" {
            let json_str = SnapshotFormatter::format_list_json(&snapshot_infos).map_err(|e| {
                Error::Serialization {
                    message: e.to_string(),
                }
            })?;
            println!("{}", json_str);
        } else {
            let table_str =
                SnapshotFormatter::format_list_table(&snapshot_infos, "Iceberg Snapshots");
            println!("{}", table_str);
        }

        Ok(())
    }

    async fn iceberg_create(path: &str, _force: bool, output: &str) -> Result<()> {
        // Iceberg creates snapshots automatically on data modifications
        // This command creates a metadata backup for safety
        println!(
            "{} Iceberg metadata snapshot at {}",
            "Creating".green(),
            path
        );

        let snapshot_service = SnapshotService::new();
        let result = snapshot_service.create_metadata_backup(path).await?;

        if output == "json" {
            let json_str = SnapshotFormatter::format_create_json(
                result.version,
                &result.backup_path,
                result.size_bytes,
            )
            .map_err(|e| Error::Serialization {
                message: e.to_string(),
            })?;
            println!("{}", json_str);
        } else {
            let table_str = SnapshotFormatter::format_create_table(
                result.version,
                &result.backup_path,
                result.size_bytes,
                "Metadata snapshot",
            );
            println!("{}", table_str);
        }

        Ok(())
    }

    async fn iceberg_expire(cfg: ExpireConfig<'_>) -> Result<()> {
        // Confirm before destructive operation
        if !confirm_destructive(
            "This will permanently expire snapshots.",
            cfg.force,
            cfg.dry_run,
        ) {
            return Ok(());
        }

        // Create metadata service using factory method - handles catalog vs path context automatically
        let metadata_service = cfg
            .resolution
            .to_writable_service(cfg.cli_catalog, cfg.branch)
            .await?;

        if let Some(b) = cfg.branch {
            println!(
                "{} snapshots for branch '{}'",
                "Expiring".yellow(),
                b.cyan()
            );
        }

        // Load metadata ONCE for validation and display
        let (metadata, _) = metadata_service.load_metadata().await?;
        let snapshots: Vec<_> = metadata.snapshots().collect();
        let current_id = metadata.current_snapshot_id();

        // Validate explicit IDs and show warnings
        if let Some(ref explicit_ids) = cfg.id {
            for id in explicit_ids {
                if Some(*id) == current_id {
                    eprintln!("{}", format!("Cannot expire current snapshot {}", id).red());
                } else if !snapshots.iter().any(|s| s.snapshot_id() == *id) {
                    eprintln!("{}", format!("Snapshot {} not found", id).yellow());
                }
            }
        }

        let config = SnapshotConfig {
            dry_run: cfg.dry_run,
        };
        let snapshot_service = SnapshotService::with_config(config);

        let result = snapshot_service
            .expire_snapshots(&metadata_service, cfg.older_than, cfg.retain_last, cfg.id)
            .await?;

        if result.expired_count == 0 {
            if cfg.output == "json" {
                let json_str = SnapshotFormatter::format_expire_json(
                    0,
                    result.cutoff_timestamp,
                    result.dry_run,
                    Some(&result.expired_ids),
                )
                .map_err(|e| Error::Serialization {
                    message: e.to_string(),
                })?;
                println!("{}", json_str);
            } else {
                println!();
                println!("{}", "No snapshots to expire".yellow());
            }
            return Ok(());
        }

        // Show snapshots to expire (for non-JSON output) - reuse loaded snapshots
        if cfg.output != "json" {
            let expire_set: std::collections::HashSet<i64> =
                result.expired_ids.iter().cloned().collect();

            let (ids, timestamps): (Vec<i64>, Vec<i64>) = snapshots
                .iter()
                .filter(|s| expire_set.contains(&s.snapshot_id()))
                .map(|s| (s.snapshot_id(), s.timestamp_ms()))
                .unzip();

            println!(
                "{}",
                SnapshotFormatter::format_expire_preview(&ids, &timestamps)
            );
        }

        if result.dry_run {
            println!();
            print_dry_run_header();
        }

        if cfg.output == "json" {
            let json_str = SnapshotFormatter::format_expire_json(
                result.expired_count,
                result.cutoff_timestamp,
                result.dry_run,
                Some(&result.expired_ids),
            )
            .map_err(|e| Error::Serialization {
                message: e.to_string(),
            })?;
            println!("{}", json_str);
        } else {
            let table_str = SnapshotFormatter::format_expire_table(
                result.expired_count,
                result.cutoff_timestamp,
                result.dry_run,
            );
            println!("{}", table_str);
        }

        Ok(())
    }

    async fn iceberg_set(cfg: SetSnapshotConfig<'_>) -> Result<()> {
        // Create metadata service using factory method - handles catalog vs path context automatically
        let metadata_service = cfg
            .resolution
            .to_writable_service(cfg.cli_catalog, None)
            .await?;

        let config = SnapshotConfig {
            dry_run: cfg.dry_run,
        };
        let snapshot_service = SnapshotService::with_config(config);

        let result = snapshot_service
            .set_current_snapshot(&metadata_service, cfg.id, cfg.as_of, cfg.branch, cfg.tag)
            .await?;

        if cfg.output == "json" {
            let json_str = SnapshotFormatter::format_set_json(
                result.previous_id,
                result.current_id,
                result.new_version,
                result.dry_run,
            )
            .map_err(|e| Error::Serialization {
                message: e.to_string(),
            })?;
            println!("{}", json_str);
        } else {
            let table_str = SnapshotFormatter::format_set_table(
                result.previous_id,
                result.current_id,
                result.new_version,
                result.dry_run,
            );
            println!("{}", table_str);
        }

        Ok(())
    }

    async fn iceberg_cherrypick(_path: &str, snapshot_id: i64, _output: &str) -> Result<()> {
        let snapshot_service = SnapshotService::new();
        snapshot_service
            .cherry_pick_snapshot(_path, snapshot_id)
            .await
    }

    async fn iceberg_lineage(
        path: &str,
        snapshot_id: Option<i64>,
        limit: Option<usize>,
        output: &str,
        resolution: &TableResolution,
        cli_catalog: Option<&CatalogConfig>,
    ) -> Result<()> {
        // Create metadata service using factory method - handles catalog vs path context automatically
        let metadata_service = resolution.to_readonly_service().await?;
        let _ = cli_catalog; // Used for write operations only
        let snapshot_service = SnapshotService::new();

        // Delegate to service for business logic
        let result = snapshot_service
            .get_lineage(&metadata_service, snapshot_id)
            .await?;

        // Convert to LineageEntry for formatting
        let entries: Vec<LineageEntry> = result
            .entries
            .iter()
            .map(|e| LineageEntry {
                snapshot_id: e.snapshot_id,
                parent_id: e.parent_id,
                timestamp_ms: e.timestamp_ms,
                operation: e.operation.clone(),
                is_current: e.is_current,
                is_root: e.is_root,
            })
            .collect();

        if output == "json" {
            let json_str =
                SnapshotFormatter::format_lineage_json(path, &entries, result.total_count)
                    .map_err(|e| Error::Serialization {
                        message: e.to_string(),
                    })?;
            println!("{}", json_str);
        } else {
            println!("{} snapshot lineage at {}", "Showing".green(), path);
            println!();
            let table_str =
                SnapshotFormatter::format_lineage_table(&entries, result.total_count, limit);
            println!("{}", table_str);
        }

        Ok(())
    }
}
