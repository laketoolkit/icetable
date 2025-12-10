//! Snapshot command implementation
//!
//! Manages snapshots for Iceberg tables.
//! Subcommands: list, create, expire, set, cherrypick

use colored::Colorize;

use super::common::{TableResolution, create_committer, create_table, print_dry_run_header, print_json, resolve_table};
use crate::cli::output::{SnapshotFormatter, SnapshotInfo};
use crate::cli::parser::{SnapshotArgs, SnapshotCommands};
use crate::core::maintenance::{SnapshotConfig, SnapshotService};
use crate::core::metadata::IcebergMetadataService;
use crate::core::{CatalogConfig, TableCommitter};
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Configuration for expire snapshots operation
struct ExpireConfig<'a> {
    path: &'a str,
    older_than: Option<String>,
    retain_last: Option<usize>,
    ids: Option<Vec<i64>>,
    dry_run: bool,
    branch: Option<&'a str>,
    output: &'a str,
    committer: Option<TableCommitter>,
}

/// Configuration for set snapshot operation
struct SetSnapshotConfig<'a> {
    path: &'a str,
    id: Option<i64>,
    as_of: Option<String>,
    branch: Option<String>,
    tag: Option<String>,
    dry_run: bool,
    output: &'a str,
    committer: Option<TableCommitter>,
}

/// Handler for snapshot command
pub struct SnapshotCommand;

impl SnapshotCommand {
    /// Execute snapshot command
    pub async fn execute(args: SnapshotArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        const ESTIMATED_MEMORY: u64 = 64 * 1024 * 1024; // 64MB for snapshot ops
        with_resource_limits(ESTIMATED_MEMORY, Self::execute_inner(args, catalog_config)).await
    }

    async fn execute_inner(args: SnapshotArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        // Get path from subcommand and resolve via config or catalog
        let subcommand_path = match &args.command {
            SnapshotCommands::List(a) => &a.path,
            SnapshotCommands::Create(a) => &a.path,
            SnapshotCommands::Expire(a) => &a.path,
            SnapshotCommands::Set(a) => &a.path,
            SnapshotCommands::Cherrypick(a) => &a.path,
            SnapshotCommands::Lineage(a) => &a.path,
        };

        // Resolve table to get path and catalog info (namespace/name if from catalog)
        let resolution = resolve_table(subcommand_path, catalog_config.as_ref()).await?;
        let path = resolution.location();

        Self::execute_iceberg(args, &path, catalog_config, &resolution).await
    }

    async fn execute_iceberg(
        args: SnapshotArgs,
        table_path: &str,
        catalog_config: Option<CatalogConfig>,
        resolution: &TableResolution,
    ) -> Result<()> {
        // Create committer based on catalog config and table resolution
        let committer = create_committer(catalog_config.as_ref(), resolution);

        match args.command {
            SnapshotCommands::List(a) => {
                Self::iceberg_list(table_path, a.limit, a.all, &a.output).await
            }
            SnapshotCommands::Create(a) => {
                Self::iceberg_create(table_path, a.force, &a.output).await
            }
            SnapshotCommands::Expire(a) => {
                let config = ExpireConfig {
                    path: table_path,
                    older_than: a.older_than,
                    retain_last: a.retain_last,
                    ids: a.ids,
                    dry_run: a.dry_run,
                    branch: a.branch.as_deref(),
                    output: &a.output,
                    committer,
                };
                Self::iceberg_expire(config).await
            }
            SnapshotCommands::Set(a) => {
                let config = SetSnapshotConfig {
                    path: table_path,
                    id: a.id,
                    as_of: a.as_of,
                    branch: a.branch,
                    tag: a.tag,
                    dry_run: a.dry_run,
                    output: &a.output,
                    committer,
                };
                Self::iceberg_set(config).await
            }
            SnapshotCommands::Cherrypick(a) => {
                Self::iceberg_cherrypick(table_path, a.snapshot_id, &a.output).await
            }
            SnapshotCommands::Lineage(a) => {
                let limit = if a.all { None } else { Some(a.limit) };
                Self::iceberg_lineage(table_path, a.snapshot_id, limit, &a.output).await
            }
        }
    }

    async fn iceberg_list(path: &str, limit: usize, all: bool, output: &str) -> Result<()> {
        println!("{} Iceberg snapshots at {}", "Listing".green(), path);
        println!();

        let metadata_service = IcebergMetadataService::new_async(path.to_string()).await?;
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
            let json_str = SnapshotFormatter::format_list_json(&snapshot_infos)
                .map_err(|e| Error::General(e.to_string()))?;
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
            .map_err(|e| Error::General(e.to_string()))?;
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
        use std::collections::HashSet;

        let metadata_service = IcebergMetadataService::new_with_branch(
            cfg.path.to_string(),
            cfg.branch.map(|s| s.to_string()),
        )
        .await?;

        if let Some(b) = cfg.branch {
            println!(
                "{} snapshots for branch '{}'",
                "Expiring".yellow(),
                b.cyan()
            );
        }
        let config = SnapshotConfig {
            dry_run: cfg.dry_run,
        };
        let snapshot_service = if let Some(c) = cfg.committer {
            SnapshotService::with_committer(config, c)
        } else {
            SnapshotService::with_config(config)
        };

        // Validate explicit IDs and show warnings
        if let Some(ref explicit_ids) = cfg.ids {
            let (metadata, _) = metadata_service.load_metadata().await?;
            let snapshots: Vec<_> = metadata.snapshots().collect();
            let current_id = metadata.current_snapshot_id();

            for id in explicit_ids {
                if Some(*id) == current_id {
                    eprintln!("{}", format!("Cannot expire current snapshot {}", id).red());
                } else if !snapshots.iter().any(|s| s.snapshot_id() == *id) {
                    eprintln!("{}", format!("Snapshot {} not found", id).yellow());
                }
            }
        }

        let result = snapshot_service
            .expire_snapshots(
                &metadata_service,
                cfg.path,
                cfg.older_than,
                cfg.retain_last,
                cfg.ids,
            )
            .await?;

        if result.expired_count == 0 {
            if cfg.output == "json" {
                let json_str = SnapshotFormatter::format_expire_json(
                    0,
                    result.cutoff_timestamp,
                    result.dry_run,
                    Some(&result.expired_ids),
                )
                .map_err(|e| Error::General(e.to_string()))?;
                println!("{}", json_str);
            } else {
                println!();
                println!("{}", "No snapshots to expire".yellow());
            }
            return Ok(());
        }

        // Show snapshots to expire (for non-JSON output)
        if cfg.output != "json" {
            let (metadata, _) = metadata_service.load_metadata().await?;
            let snapshots: Vec<_> = metadata.snapshots().collect();
            let expire_set: HashSet<i64> = result.expired_ids.iter().cloned().collect();

            println!();
            println!("Snapshots to expire: {}", result.expired_count);
            for snap in snapshots
                .iter()
                .filter(|s| expire_set.contains(&s.snapshot_id()))
            {
                let ts = chrono::DateTime::from_timestamp_millis(snap.timestamp_ms())
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| snap.timestamp_ms().to_string());
                println!("  - {} ({})", snap.snapshot_id(), ts);
            }
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
            .map_err(|e| Error::General(e.to_string()))?;
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
        let metadata_service = IcebergMetadataService::new_async(cfg.path.to_string()).await?;
        let config = SnapshotConfig {
            dry_run: cfg.dry_run,
        };
        let snapshot_service = if let Some(c) = cfg.committer {
            SnapshotService::with_committer(config, c)
        } else {
            SnapshotService::with_config(config)
        };

        let result = snapshot_service
            .set_current_snapshot(
                &metadata_service,
                cfg.path,
                cfg.id,
                cfg.as_of,
                cfg.branch,
                cfg.tag,
            )
            .await?;

        if cfg.output == "json" {
            let json_str = SnapshotFormatter::format_set_json(
                result.previous_id,
                result.current_id,
                result.new_version,
                result.dry_run,
            )
            .map_err(|e| Error::General(e.to_string()))?;
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
    ) -> Result<()> {
        let metadata_service = IcebergMetadataService::new_async(path.to_string()).await?;
        let snapshot_service = SnapshotService::new();

        // Delegate to service for business logic
        let result = snapshot_service
            .get_lineage(&metadata_service, snapshot_id)
            .await?;

        let total_count = result.total_count;
        let max_items = limit.unwrap_or(usize::MAX);
        let is_truncated = total_count > max_items && limit.is_some();

        if output == "json" {
            let json_lineage: Vec<serde_json::Value> = result
                .entries
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "snapshot_id": entry.snapshot_id,
                        "parent_id": entry.parent_id,
                        "timestamp": chrono::DateTime::from_timestamp_millis(entry.timestamp_ms)
                            .map(|dt| dt.to_rfc3339())
                            .unwrap_or_default(),
                        "operation": entry.operation,
                        "is_current": entry.is_current,
                    })
                })
                .collect();

            let json = serde_json::json!({
                "table": path,
                "lineage": json_lineage,
                "total": total_count,
            });
            print_json(&json)?;
        } else {
            use comfy_table::{Cell, CellAlignment};

            println!("{} snapshot lineage at {}", "Showing".green(), path);
            println!();

            let mut table = create_table();

            table.set_header(vec![
                Cell::new("Snapshot".cyan().to_string()).set_alignment(CellAlignment::Center),
                Cell::new("Operation".cyan().to_string()).set_alignment(CellAlignment::Center),
                Cell::new("Timestamp".cyan().to_string()).set_alignment(CellAlignment::Center),
                Cell::new("Status".cyan().to_string()).set_alignment(CellAlignment::Center),
            ]);

            // Show items up to limit
            let display_count = if is_truncated {
                max_items - 1
            } else {
                total_count
            };

            for entry in result.entries.iter().take(display_count) {
                let ts_str = chrono::DateTime::from_timestamp_millis(entry.timestamp_ms)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| entry.timestamp_ms.to_string());

                let status = if entry.is_current {
                    "● current".green().to_string()
                } else if entry.is_root && !is_truncated {
                    "● root".green().to_string()
                } else {
                    "".to_string()
                };

                table.add_row(vec![
                    Cell::new(entry.snapshot_id.to_string()).set_alignment(CellAlignment::Right),
                    Cell::new(&entry.operation),
                    Cell::new(ts_str),
                    Cell::new(status),
                ]);
            }

            println!("{}", table);

            // Show truncation indicator and root outside the table
            if is_truncated {
                let skipped = total_count - max_items;
                println!(
                    "         {} ({})",
                    "...".dimmed(),
                    format!("{} more", skipped).dimmed()
                );

                // Show root in a separate mini-table
                if let Some(root) = result.entries.last() {
                    let ts_str = chrono::DateTime::from_timestamp_millis(root.timestamp_ms)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| root.timestamp_ms.to_string());

                    let mut root_table = create_table();
                    root_table.set_header(vec![
                        Cell::new("Snapshot".cyan().to_string())
                            .set_alignment(CellAlignment::Center),
                        Cell::new("Operation".cyan().to_string())
                            .set_alignment(CellAlignment::Center),
                        Cell::new("Timestamp".cyan().to_string())
                            .set_alignment(CellAlignment::Center),
                        Cell::new("Status".cyan().to_string()).set_alignment(CellAlignment::Center),
                    ]);
                    root_table.add_row(vec![
                        Cell::new(root.snapshot_id.to_string()).set_alignment(CellAlignment::Right),
                        Cell::new(&root.operation),
                        Cell::new(ts_str),
                        Cell::new("● root".green().to_string()),
                    ]);
                    println!("{}", root_table);
                }
            }

            println!();
            println!("{}", format!("{} snapshots total", total_count).dimmed());
        }

        Ok(())
    }
}
