//! Snapshot command implementation
//!
//! Manages snapshots for Delta Lake and Iceberg tables.
//! Subcommands: list, create, expire, set, cherrypick

use colored::Colorize;
use std::sync::Arc;

use crate::cli::output::{SnapshotFormatter, SnapshotInfo};
use crate::cli::parser::{SnapshotArgs, SnapshotCommands};
use crate::config::ResolvePath;
use crate::core::TableFormat;
use crate::core::maintenance::{SnapshotConfig, SnapshotService};
use crate::core::metadata::IcebergMetadataService;
use crate::core::storage::{StorageBackend, StorageBackendFactory};
use crate::core::utils::detect_table_format_with_storage;
use crate::error::{Error, Result};

/// Handler for snapshot command
pub struct SnapshotCommand;

impl SnapshotCommand {
    /// Execute snapshot command
    pub async fn execute(args: SnapshotArgs) -> Result<()> {
        // Get path from subcommand and resolve via config
        let path = match &args.command {
            SnapshotCommands::List(a) => a.path.resolve()?,
            SnapshotCommands::Create(a) => a.path.resolve()?,
            SnapshotCommands::Expire(a) => a.path.resolve()?,
            SnapshotCommands::Set(a) => a.path.resolve()?,
            SnapshotCommands::Cherrypick(a) => a.path.resolve()?,
        };

        // Create storage backend
        let storage = StorageBackendFactory::create_backend(&path).await?;

        // Detect table format
        let format = detect_table_format_with_storage(&path, &storage).await;

        match format {
            TableFormat::Delta => Self::execute_delta(args, &path, storage).await,
            TableFormat::Iceberg => Self::execute_iceberg(args, &path).await,
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
        _storage: Arc<dyn StorageBackend>,
    ) -> Result<()> {
        Err(Error::General(
            "Delta Lake snapshot management is not supported. Use 'icectl import delta' to convert Delta tables to Iceberg.".to_string(),
        ))
    }

    #[cfg(not(feature = "delta"))]
    async fn execute_delta(
        _args: SnapshotArgs,
        _table_path: &str,
        _storage: Arc<dyn StorageBackend>,
    ) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Delta Lake support not enabled".to_string(),
        })
    }

    // =========================================================================
    // Iceberg implementation
    // =========================================================================

    async fn execute_iceberg(args: SnapshotArgs, table_path: &str) -> Result<()> {
        match args.command {
            SnapshotCommands::List(a) => {
                Self::iceberg_list(table_path, a.limit, a.all, &a.output).await
            }
            SnapshotCommands::Create(a) => {
                Self::iceberg_create(table_path, a.force, &a.output).await
            }
            SnapshotCommands::Expire(a) => {
                Self::iceberg_expire(
                    table_path,
                    a.older_than,
                    a.retain_last,
                    a.ids,
                    a.dry_run,
                    &a.output,
                )
                .await
            }
            SnapshotCommands::Set(a) => {
                Self::iceberg_set(table_path, a.id, a.as_of, a.dry_run, &a.output).await
            }
            SnapshotCommands::Cherrypick(a) => {
                Self::iceberg_cherrypick(table_path, a.snapshot_id, &a.output).await
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

    async fn iceberg_expire(
        path: &str,
        older_than: Option<String>,
        retain_last: Option<usize>,
        ids: Option<Vec<i64>>,
        dry_run: bool,
        output: &str,
    ) -> Result<()> {
        use std::collections::HashSet;

        let metadata_service = IcebergMetadataService::new_async(path.to_string()).await?;
        let config = SnapshotConfig { dry_run };
        let snapshot_service = SnapshotService::with_config(config);

        // Validate explicit IDs and show warnings
        if let Some(ref explicit_ids) = ids {
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
            .expire_snapshots(&metadata_service, path, older_than, retain_last, ids)
            .await?;

        if result.expired_count == 0 {
            if output == "json" {
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
        if output != "json" {
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

        if output == "json" {
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

            if !result.dry_run {
                println!();
                println!(
                    "{}",
                    "Note: Data files are NOT deleted. Use 'icectl vacuum' to remove orphaned data files.".dimmed()
                );
            }
        }

        Ok(())
    }

    async fn iceberg_set(
        path: &str,
        id: Option<i64>,
        as_of: Option<String>,
        dry_run: bool,
        output: &str,
    ) -> Result<()> {
        let metadata_service = IcebergMetadataService::new_async(path.to_string()).await?;
        let config = SnapshotConfig { dry_run };
        let snapshot_service = SnapshotService::with_config(config);

        let result = snapshot_service
            .set_current_snapshot(&metadata_service, path, id, as_of)
            .await?;

        if output == "json" {
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
}
