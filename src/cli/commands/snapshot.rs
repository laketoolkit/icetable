//! Snapshot command implementation
//!
//! Manages snapshots for Delta Lake and Iceberg tables.
//! Subcommands: list, create, expire, set, cherrypick

use colored::Colorize;

use super::common::{resolve_table, TableResolution};
use crate::cli::output::{SnapshotFormatter, SnapshotInfo};
use crate::cli::parser::{SnapshotArgs, SnapshotCommands};
use crate::core::maintenance::{SnapshotConfig, SnapshotService};
use crate::core::metadata::IcebergMetadataService;
use crate::core::storage::{Storage, create_object_store};
use crate::core::utils::detect_table_format_with_storage;
use crate::core::{CatalogConfig, TableCommitter};
use crate::core::TableFormat;
use crate::error::{Error, Result};

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

        // Create storage backend
        let storage = create_object_store(&path).await?;

        // Detect table format
        let format = detect_table_format_with_storage(&path, &storage).await;

        match format {
            TableFormat::Delta => Self::execute_delta(args, &path, storage).await,
            TableFormat::Iceberg => Self::execute_iceberg(args, &path, catalog_config, &resolution).await,
            TableFormat::Unknown => Err(Error::General(format!(
                "Path '{}' is not a Delta Lake or Iceberg table",
                path
            ))),
        }
    }

    // =========================================================================
    // Delta Lake implementation (limited to import functionality)
    // =========================================================================

    #[cfg(feature = "delta")]
    async fn execute_delta(
        _args: SnapshotArgs,
        _table_path: &str,
        _storage: Storage,
    ) -> Result<()> {
        Err(Error::General(
            "Delta Lake snapshot management is not supported. Use 'icetable import delta' to convert Delta tables to Iceberg.".to_string(),
        ))
    }

    #[cfg(not(feature = "delta"))]
    async fn execute_delta(
        _args: SnapshotArgs,
        _table_path: &str,
        _storage: Storage,
    ) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Delta Lake support not enabled".to_string(),
        })
    }

    // =========================================================================
    // Iceberg implementation
    // =========================================================================

    async fn execute_iceberg(
        args: SnapshotArgs,
        table_path: &str,
        catalog_config: Option<CatalogConfig>,
        resolution: &TableResolution,
    ) -> Result<()> {
        // Create committer based on catalog config and table resolution
        let committer = Self::create_committer(catalog_config.as_ref(), resolution);

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

    /// Create a TableCommitter based on catalog config and table resolution
    fn create_committer(
        catalog_config: Option<&CatalogConfig>,
        resolution: &TableResolution,
    ) -> Option<TableCommitter> {
        // Only create committer if we have catalog config AND the table came from a catalog
        match (catalog_config, resolution) {
            (Some(config), TableResolution::CatalogTable { namespace, name, .. }) => {
                // Use the actual namespace and name from catalog resolution
                Some(TableCommitter::with_catalog(config.clone(), namespace.clone(), name.clone()))
            }
            _ => {
                // No catalog or table is from direct path - use direct mode (no committer)
                None
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
        let config = SnapshotConfig { dry_run: cfg.dry_run };
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
            .expire_snapshots(&metadata_service, cfg.path, cfg.older_than, cfg.retain_last, cfg.ids)
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
            println!("{}", "DRY RUN - No changes made".yellow().bold());
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
        let config = SnapshotConfig { dry_run: cfg.dry_run };
        let snapshot_service = if let Some(c) = cfg.committer {
            SnapshotService::with_committer(config, c)
        } else {
            SnapshotService::with_config(config)
        };

        let result = snapshot_service
            .set_current_snapshot(&metadata_service, cfg.path, cfg.id, cfg.as_of, cfg.branch, cfg.tag)
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
        use std::collections::HashMap;

        let metadata_service = IcebergMetadataService::new_async(path.to_string()).await?;
        let (metadata, _) = metadata_service.load_metadata().await?;

        // Get starting snapshot
        let start_id = snapshot_id.or_else(|| metadata.current_snapshot_id()).ok_or_else(|| {
            Error::General("No snapshot specified and table has no current snapshot".to_string())
        })?;

        // Build parent map for quick lookup
        let parent_map: HashMap<i64, Option<i64>> = metadata
            .snapshots()
            .map(|s| (s.snapshot_id(), s.parent_snapshot_id()))
            .collect();

        // Build snapshot info map
        let snapshot_map: HashMap<i64, _> = metadata
            .snapshots()
            .map(|s| (s.snapshot_id(), s))
            .collect();

        // Walk the full lineage first to get total count and root
        let mut full_lineage: Vec<(i64, Option<i64>, i64, String)> = Vec::new();
        let mut current = Some(start_id);

        while let Some(id) = current {
            let parent = parent_map.get(&id).copied().flatten();
            let (timestamp, operation) = snapshot_map
                .get(&id)
                .map(|s| {
                    let ts = s.timestamp_ms();
                    let op = format!("{:?}", s.summary().operation).to_lowercase();
                    (ts, op)
                })
                .unwrap_or((0, "unknown".to_string()));

            full_lineage.push((id, parent, timestamp, operation));
            current = parent;
        }

        let total_count = full_lineage.len();
        let max_items = limit.unwrap_or(usize::MAX);
        let is_truncated = total_count > max_items && limit.is_some();

        let current_id = metadata.current_snapshot_id();

        if output == "json" {
            let json_lineage: Vec<serde_json::Value> = full_lineage
                .iter()
                .map(|(id, parent, ts, op)| {
                    serde_json::json!({
                        "snapshot_id": id,
                        "parent_id": parent,
                        "timestamp": chrono::DateTime::from_timestamp_millis(*ts)
                            .map(|dt| dt.to_rfc3339())
                            .unwrap_or_default(),
                        "operation": op,
                        "is_current": Some(*id) == current_id,
                    })
                })
                .collect();

            let json = serde_json::json!({
                "table": path,
                "lineage": json_lineage,
                "total": total_count,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            use comfy_table::{presets::UTF8_FULL, Cell, CellAlignment, ContentArrangement};

            println!("{} snapshot lineage at {}", "Showing".green(), path);
            println!();

            let mut table = comfy_table::Table::new();
            table.load_preset(UTF8_FULL);
            table.set_content_arrangement(ContentArrangement::Dynamic);

            table.set_header(vec![
                Cell::new("Snapshot".cyan().to_string()).set_alignment(CellAlignment::Center),
                Cell::new("Operation".cyan().to_string()).set_alignment(CellAlignment::Center),
                Cell::new("Timestamp".cyan().to_string()).set_alignment(CellAlignment::Center),
                Cell::new("Status".cyan().to_string()).set_alignment(CellAlignment::Center),
            ]);

            // Show items up to limit
            let display_count = if is_truncated { max_items - 1 } else { total_count };

            for (id, parent, ts, op) in full_lineage.iter().take(display_count) {
                let ts_str = chrono::DateTime::from_timestamp_millis(*ts)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| ts.to_string());

                let is_current = Some(*id) == current_id;
                let is_root = parent.is_none() && !is_truncated;

                let status = if is_current {
                    "● current".green().to_string()
                } else if is_root {
                    "● root".green().to_string()
                } else {
                    "".to_string()
                };

                table.add_row(vec![
                    Cell::new(id.to_string()).set_alignment(CellAlignment::Right),
                    Cell::new(op),
                    Cell::new(ts_str),
                    Cell::new(status),
                ]);
            }

            println!("{}", table);

            // Show truncation indicator and root outside the table
            if is_truncated {
                let skipped = total_count - max_items;
                println!("         {} ({})", "...".dimmed(), format!("{} more", skipped).dimmed());

                // Show root in a separate mini-table
                if let Some((id, _, ts, op)) = full_lineage.last() {
                    let ts_str = chrono::DateTime::from_timestamp_millis(*ts)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| ts.to_string());

                    let mut root_table = comfy_table::Table::new();
                    root_table.load_preset(UTF8_FULL);
                    root_table.set_content_arrangement(ContentArrangement::Dynamic);
                    root_table.set_header(vec![
                        Cell::new("Snapshot".cyan().to_string()).set_alignment(CellAlignment::Center),
                        Cell::new("Operation".cyan().to_string()).set_alignment(CellAlignment::Center),
                        Cell::new("Timestamp".cyan().to_string()).set_alignment(CellAlignment::Center),
                        Cell::new("Status".cyan().to_string()).set_alignment(CellAlignment::Center),
                    ]);
                    root_table.add_row(vec![
                        Cell::new(id.to_string()).set_alignment(CellAlignment::Right),
                        Cell::new(op),
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
