//! Snapshot command implementation
//!
//! Manages snapshots for Delta Lake and Iceberg tables.
//! Subcommands: list, create, expire, set, cherrypick

use colored::Colorize;
use std::sync::Arc;

use crate::cli::parser::{SnapshotArgs, SnapshotCommands};
use crate::config::ResolvePath;
use crate::core::storage::{StorageBackend, StorageBackendFactory};
use crate::core::utils::detect_table_format_with_storage;
use crate::core::{format_bytes, TableFormat};
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
            TableFormat::Iceberg => Self::execute_iceberg(args, &path, storage).await,
            TableFormat::Unknown => Err(Error::General(format!(
                "Path '{}' is not a Delta Lake or Iceberg table",
                path
            ))),
        }
    }

    // =========================================================================
    // Delta Lake implementation
    // =========================================================================

    #[cfg(feature = "delta")]
    async fn execute_delta(
        args: SnapshotArgs,
        table_path: &str,
        _storage: Arc<dyn StorageBackend>,
    ) -> Result<()> {
        match args.command {
            SnapshotCommands::List(a) => {
                Self::delta_list(table_path, a.limit, a.all, &a.output).await
            }
            SnapshotCommands::Create(a) => Self::delta_create(table_path, a.force, &a.output).await,
            SnapshotCommands::Expire(a) => {
                Self::delta_expire(
                    table_path,
                    a.older_than,
                    a.retain_last,
                    a.ids,
                    a.dry_run,
                    &a.output,
                )
                .await
            }
            SnapshotCommands::Set(a) => Self::delta_set(table_path, a.id, a.as_of, &a.output).await,
            SnapshotCommands::Cherrypick(_) => Err(Error::General(
                "Cherry-pick is not supported for Delta Lake tables".to_string(),
            )),
        }
    }

    #[cfg(feature = "delta")]
    async fn delta_list(path: &str, limit: usize, all: bool, output: &str) -> Result<()> {
        use deltalake::DeltaTableBuilder;

        println!("{} Delta versions at {}", "Listing".green(), path);
        println!();

        let table = DeltaTableBuilder::from_uri(path)
            .load()
            .await
            .map_err(|e| Error::General(format!("Failed to load Delta table: {}", e)))?;

        let current_version = table.version().unwrap_or(0);
        let log_store = table.log_store();

        let limit = if all {
            current_version as usize + 1
        } else {
            limit.min(current_version as usize + 1)
        };

        // Collect commit info
        let mut commits = Vec::new();
        for version in 0..=current_version {
            if let Ok(Some(bytes)) = log_store.read_commit_entry(version).await {
                let content = String::from_utf8_lossy(&bytes);
                let (timestamp, operation) = Self::parse_delta_commit_info(&content);
                commits.push((version, timestamp, operation));
            }
        }

        // Sort by timestamp descending (most recent first)
        commits.sort_by(|a, b| {
            match (&b.1, &a.1) {
                (Some(tb), Some(ta)) => tb.cmp(ta),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => b.0.cmp(&a.0), // Fall back to version descending
            }
        });

        // Apply limit after sorting
        commits.truncate(limit);

        if output == "json" {
            let versions: Vec<_> = commits
                .iter()
                .map(|(version, timestamp, operation)| {
                    serde_json::json!({
                        "version": version,
                        "timestamp": timestamp,
                        "operation": operation,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&versions)
                    .map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!(
                "{:<10} {:<25} {}",
                "VERSION".cyan(),
                "TIMESTAMP".cyan(),
                "OPERATION".cyan()
            );
            println!("{}", "-".repeat(70));

            for (version, timestamp, operation) in &commits {
                let ts_str = timestamp
                    .as_ref()
                    .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| "-".to_string());

                println!("{:<10} {:<25} {}", version, ts_str, operation);
            }
        }

        Ok(())
    }

    #[cfg(feature = "delta")]
    fn parse_delta_commit_info(content: &str) -> (Option<chrono::DateTime<chrono::Utc>>, String) {
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(commit_info) = json.get("commitInfo") {
                    let timestamp = commit_info
                        .get("timestamp")
                        .and_then(|v| v.as_i64())
                        .and_then(chrono::DateTime::from_timestamp_millis);
                    let operation = commit_info
                        .get("operation")
                        .and_then(|v| v.as_str())
                        .unwrap_or("UNKNOWN")
                        .to_string();
                    return (timestamp, operation);
                }
            }
        }
        (None, "UNKNOWN".to_string())
    }

    #[cfg(feature = "delta")]
    async fn delta_create(path: &str, force: bool, output: &str) -> Result<()> {
        use deltalake::checkpoints::create_checkpoint;

        println!("{} Delta checkpoint at {}", "Creating".green(), path);

        let table = deltalake::open_table(path)
            .await
            .map_err(|e| Error::General(format!("Failed to open Delta table: {}", e)))?;

        let version = table.version().unwrap_or(0);

        // Check if checkpoint already exists
        let log_dir = std::path::Path::new(path).join("_delta_log");
        let checkpoint_file = log_dir.join(format!("{:020}.checkpoint.parquet", version));

        if checkpoint_file.exists() && !force {
            println!();
            println!(
                "{}",
                format!("Checkpoint already exists for version {}", version).yellow()
            );
            println!("Use --force to create anyway");
            return Ok(());
        }

        create_checkpoint(&table, None)
            .await
            .map_err(|e| Error::General(format!("Failed to create checkpoint: {}", e)))?;

        let checkpoint_size = std::fs::metadata(&checkpoint_file)
            .map(|m| m.len())
            .unwrap_or(0);

        if output == "json" {
            let json = serde_json::json!({
                "version": version,
                "checkpoint_file": checkpoint_file.to_string_lossy(),
                "size_bytes": checkpoint_size,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!();
            println!("{}", "Checkpoint created!".green().bold());
            println!();
            println!("Version:    {}", version.to_string().cyan());
            println!("File:       {}", checkpoint_file.display());
            println!("Size:       {}", format_bytes(checkpoint_size));
        }

        Ok(())
    }

    #[cfg(feature = "delta")]
    async fn delta_expire(
        path: &str,
        older_than: Option<String>,
        retain_last: Option<usize>,
        _ids: Option<Vec<i64>>,
        dry_run: bool,
        output: &str,
    ) -> Result<()> {
        use deltalake::DeltaTableBuilder;
        use deltalake::checkpoints::{cleanup_expired_logs_for, create_checkpoint};

        println!("{} Delta log entries at {}", "Expiring".green(), path);

        let table = DeltaTableBuilder::from_uri(path)
            .load()
            .await
            .map_err(|e| Error::General(format!("Failed to load Delta table: {}", e)))?;

        let current_version = table.version().unwrap_or(0);
        let log_store = table.log_store();

        // Collect all commits with timestamps
        let mut commits: Vec<(i64, Option<chrono::DateTime<chrono::Utc>>)> = Vec::new();
        for version in 0..=current_version {
            if let Ok(Some(bytes)) = log_store.read_commit_entry(version).await {
                let content = String::from_utf8_lossy(&bytes);
                let (timestamp, _) = Self::parse_delta_commit_info(&content);
                commits.push((version, timestamp));
            }
        }

        // Determine cutoff timestamp
        let cutoff_timestamp = if let Some(older_than) = &older_than {
            parse_timestamp(older_than)?
        } else if let Some(retain) = retain_last {
            // Sort by timestamp descending
            let mut sorted = commits.clone();
            sorted.sort_by(|a, b| match (&b.1, &a.1) {
                (Some(tb), Some(ta)) => tb.cmp(ta),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => b.0.cmp(&a.0),
            });

            if sorted.len() <= retain {
                println!();
                println!(
                    "{}",
                    format!(
                        "Table has {} versions, retaining {} - nothing to expire",
                        sorted.len(),
                        retain
                    )
                    .yellow()
                );
                return Ok(());
            }

            // Get timestamp of the (retain)th newest commit - everything older will be expired
            sorted
                .get(retain)
                .and_then(|(_, ts)| *ts)
                .unwrap_or_else(chrono::Utc::now)
        } else {
            // Default: 7 days retention
            chrono::Utc::now() - chrono::Duration::days(7)
        };

        // Find versions to expire
        let versions_to_expire: Vec<i64> = commits
            .iter()
            .filter(|(_, ts)| ts.map(|t| t < cutoff_timestamp).unwrap_or(false))
            .map(|(v, _)| *v)
            .collect();

        if versions_to_expire.is_empty() {
            println!();
            println!("{}", "No log entries to expire".yellow());
            return Ok(());
        }

        println!();
        println!(
            "Cutoff time: {}",
            cutoff_timestamp.format("%Y-%m-%d %H:%M:%S UTC")
        );
        println!("Log entries to expire: {}", versions_to_expire.len());
        println!(
            "Versions: {} to {}",
            versions_to_expire.iter().min().unwrap_or(&0),
            versions_to_expire.iter().max().unwrap_or(&0)
        );

        if dry_run {
            println!();
            println!("{}", "DRY RUN - No changes made".yellow().bold());

            if output == "json" {
                let json = serde_json::json!({
                    "dry_run": true,
                    "versions_to_expire": versions_to_expire,
                    "cutoff_timestamp": cutoff_timestamp.to_rfc3339(),
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json)
                        .map_err(|e| Error::General(e.to_string()))?
                );
            }
            return Ok(());
        }

        // First, ensure we have a checkpoint so we don't corrupt the table
        // Delta requires at least one checkpoint to reconstruct state
        println!();
        println!("Creating checkpoint before cleanup...");
        create_checkpoint(&table, None)
            .await
            .map_err(|e| Error::General(format!("Failed to create checkpoint: {}", e)))?;

        // Now cleanup expired logs
        let cutoff_ms = cutoff_timestamp.timestamp_millis();
        let deleted_count =
            cleanup_expired_logs_for(current_version, table.log_store().as_ref(), cutoff_ms, None)
                .await
                .map_err(|e| Error::General(format!("Failed to cleanup logs: {}", e)))?;

        if output == "json" {
            let json = serde_json::json!({
                "deleted_log_entries": deleted_count,
                "cutoff_timestamp": cutoff_timestamp.to_rfc3339(),
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!();
            println!("{}", "Expire complete!".green().bold());
            println!("Log entries deleted: {}", deleted_count);
            println!();
            println!(
                "{}",
                "Note: Data files are NOT deleted. Use 'icectl vacuum' to remove orphaned data files.".dimmed()
            );
        }

        Ok(())
    }

    #[cfg(feature = "delta")]
    async fn delta_set(
        path: &str,
        id: Option<i64>,
        as_of: Option<String>,
        output: &str,
    ) -> Result<()> {
        use deltalake::DeltaTableBuilder;

        let version = if let Some(v) = id {
            v
        } else if let Some(as_of) = &as_of {
            // Find version by timestamp
            let table = DeltaTableBuilder::from_uri(path)
                .load()
                .await
                .map_err(|e| Error::General(format!("Failed to load Delta table: {}", e)))?;

            let cutoff = parse_timestamp(as_of)?;
            let cutoff_ms = cutoff.timestamp_millis();
            let current_version = table.version().unwrap_or(0);
            let log_store = table.log_store();

            // Find the most recent version before or at the timestamp
            let mut best_version = None;
            for v in (0..=current_version).rev() {
                if let Ok(Some(bytes)) = log_store.read_commit_entry(v).await {
                    let content = String::from_utf8_lossy(&bytes);
                    let (timestamp, _) = Self::parse_delta_commit_info(&content);
                    if let Some(ts) = timestamp {
                        if ts.timestamp_millis() <= cutoff_ms {
                            best_version = Some(v);
                            break;
                        }
                    }
                }
            }
            best_version
                .ok_or_else(|| Error::General(format!("No version found before {}", as_of)))?
        } else {
            return Err(Error::General("Must specify --id or --as-of".to_string()));
        };

        println!("{} Delta table to version {}", "Setting".green(), version);

        // Load at specific version to verify it exists
        let _table = DeltaTableBuilder::from_uri(path)
            .with_version(version)
            .load()
            .await
            .map_err(|e| Error::General(format!("Failed to load version {}: {}", version, e)))?;

        if output == "json" {
            let json = serde_json::json!({
                "version": version,
                "status": "loaded",
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!();
            println!("{}", "Table set to version!".green().bold());
            println!("Version: {}", version.to_string().cyan());
            println!();
            println!(
                "{}",
                "Note: This is a read-only time-travel operation.".dimmed()
            );
            println!(
                "{}",
                "To restore permanently, use 'icectl restore --version'".dimmed()
            );
        }

        Ok(())
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

    #[cfg(feature = "iceberg")]
    async fn execute_iceberg(
        args: SnapshotArgs,
        table_path: &str,
        _storage: Arc<dyn StorageBackend>,
    ) -> Result<()> {
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
                Self::iceberg_set(table_path, a.id, a.as_of, &a.output).await
            }
            SnapshotCommands::Cherrypick(a) => {
                Self::iceberg_cherrypick(table_path, a.snapshot_id, &a.output).await
            }
        }
    }

    #[cfg(feature = "iceberg")]
    async fn iceberg_list(path: &str, limit: usize, all: bool, output: &str) -> Result<()> {
        use crate::core::metadata::IcebergMetadataService;

        println!("{} Iceberg snapshots at {}", "Listing".green(), path);
        println!();

        let service = IcebergMetadataService::new_async(path.to_string()).await?;
        let (metadata, _) = service.load_metadata().await?;

        let mut snapshots: Vec<_> = metadata.snapshots().collect();

        // Sort by timestamp descending (most recent first)
        snapshots.sort_by(|a, b| b.timestamp_ms().cmp(&a.timestamp_ms()));

        let limit = if all {
            snapshots.len()
        } else {
            limit.min(snapshots.len())
        };

        let current_snapshot_id = metadata.current_snapshot_id();

        if output == "json" {
            let snap_json: Vec<_> = snapshots
                .iter()
                .take(limit)
                .map(|snap| {
                    serde_json::json!({
                        "snapshot_id": snap.snapshot_id(),
                        "timestamp_ms": snap.timestamp_ms(),
                        "parent_snapshot_id": snap.parent_snapshot_id(),
                        "is_current": Some(snap.snapshot_id()) == current_snapshot_id,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&snap_json)
                    .map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!(
                "{:<20} {:<25} {:<20} {}",
                "SNAPSHOT ID".cyan(),
                "TIMESTAMP".cyan(),
                "PARENT".cyan(),
                "".cyan()
            );
            println!("{}", "-".repeat(80));

            for snap in snapshots.iter().take(limit) {
                let is_current = Some(snap.snapshot_id()) == current_snapshot_id;
                let timestamp = chrono::DateTime::from_timestamp_millis(snap.timestamp_ms())
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| snap.timestamp_ms().to_string());
                let parent = snap
                    .parent_snapshot_id()
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "-".to_string());
                let marker = if is_current {
                    " (current)".green().to_string()
                } else {
                    "".to_string()
                };

                println!(
                    "{:<20} {:<25} {:<20} {}",
                    snap.snapshot_id(),
                    timestamp,
                    parent,
                    marker
                );
            }
        }

        Ok(())
    }

    #[cfg(feature = "iceberg")]
    async fn iceberg_create(path: &str, _force: bool, output: &str) -> Result<()> {
        // Iceberg creates snapshots automatically on data modifications
        // This command creates a metadata backup for safety

        println!(
            "{} Iceberg metadata snapshot at {}",
            "Creating".green(),
            path
        );

        let table_path = std::path::Path::new(path);
        let metadata_dir = table_path.join("metadata");

        // Get current version
        let version_hint = metadata_dir.join("version-hint.text");
        let current_version: i32 = std::fs::read_to_string(&version_hint)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1);

        let metadata_file = metadata_dir.join(format!("v{}.metadata.json", current_version));

        if !metadata_file.exists() {
            return Err(Error::General(format!(
                "Metadata file not found: {}",
                metadata_file.display()
            )));
        }

        // Create backup with timestamp
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let backup_file = metadata_dir.join(format!(
            "v{}.metadata.{}.backup.json",
            current_version, timestamp
        ));

        std::fs::copy(&metadata_file, &backup_file)
            .map_err(|e| Error::General(format!("Failed to create backup: {}", e)))?;

        let backup_size = std::fs::metadata(&backup_file)
            .map(|m| m.len())
            .unwrap_or(0);

        if output == "json" {
            let json = serde_json::json!({
                "version": current_version,
                "backup_file": backup_file.to_string_lossy(),
                "size_bytes": backup_size,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!();
            println!("{}", "Metadata snapshot created!".green().bold());
            println!();
            println!("Version:    {}", current_version.to_string().cyan());
            println!("Backup:     {}", backup_file.display());
            println!("Size:       {}", format_bytes(backup_size));
        }

        Ok(())
    }

    #[cfg(feature = "iceberg")]
    async fn iceberg_expire(
        path: &str,
        older_than: Option<String>,
        retain_last: Option<usize>,
        ids: Option<Vec<i64>>,
        dry_run: bool,
        output: &str,
    ) -> Result<()> {
        use crate::core::metadata::IcebergMetadataService;
        use crate::core::storage::StorageBackendFactory;
        use crate::core::storage::traits::PutOptions;
        use std::collections::HashSet;

        println!("{} Iceberg snapshots at {}", "Expiring".green(), path);

        // Load metadata using iceberg-rs
        let service = IcebergMetadataService::new_async(path.to_string()).await?;
        let (metadata, current_version) = service.load_metadata().await?;

        // Get all snapshots from iceberg-rs
        let snapshots: Vec<_> = metadata.snapshots().collect();
        let current_id = metadata.current_snapshot_id();

        // Determine which snapshots to expire
        let mut to_expire: Vec<i64> = Vec::new();

        if let Some(ids) = &ids {
            // Explicit IDs
            for id in ids {
                if Some(*id) == current_id {
                    println!("{}", format!("Cannot expire current snapshot {}", id).red());
                    continue;
                }
                if snapshots.iter().any(|s| s.snapshot_id() == *id) {
                    to_expire.push(*id);
                } else {
                    println!("{}", format!("Snapshot {} not found", id).yellow());
                }
            }
        } else if let Some(older_than) = &older_than {
            let cutoff = parse_timestamp(older_than)?;
            let cutoff_ms = cutoff.timestamp_millis();

            for snap in &snapshots {
                if snap.timestamp_ms() < cutoff_ms && Some(snap.snapshot_id()) != current_id {
                    to_expire.push(snap.snapshot_id());
                }
            }
        } else if let Some(retain) = retain_last {
            // Sort by timestamp descending
            let mut sorted: Vec<_> = snapshots.iter().collect();
            sorted.sort_by(|a, b| b.timestamp_ms().cmp(&a.timestamp_ms()));

            // Skip the first N, expire the rest
            for snap in sorted.iter().skip(retain) {
                if Some(snap.snapshot_id()) != current_id {
                    to_expire.push(snap.snapshot_id());
                }
            }
        } else {
            // Default: 7 days retention
            let cutoff = chrono::Utc::now() - chrono::Duration::days(7);
            let cutoff_ms = cutoff.timestamp_millis();

            for snap in &snapshots {
                if snap.timestamp_ms() < cutoff_ms && Some(snap.snapshot_id()) != current_id {
                    to_expire.push(snap.snapshot_id());
                }
            }
        }

        if to_expire.is_empty() {
            println!();
            println!("{}", "No snapshots to expire".yellow());
            return Ok(());
        }

        // Get snapshot details for display
        let expire_set: HashSet<i64> = to_expire.iter().cloned().collect();
        println!();
        println!("Snapshots to expire: {}", to_expire.len());
        for snap in snapshots
            .iter()
            .filter(|s| expire_set.contains(&s.snapshot_id()))
        {
            let ts = chrono::DateTime::from_timestamp_millis(snap.timestamp_ms())
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| snap.timestamp_ms().to_string());
            println!("  - {} ({})", snap.snapshot_id(), ts);
        }

        if dry_run {
            println!();
            println!("{}", "DRY RUN - No changes made".yellow().bold());

            if output == "json" {
                let json = serde_json::json!({
                    "dry_run": true,
                    "snapshots_to_expire": to_expire,
                    "total_snapshots": snapshots.len(),
                    "remaining_snapshots": snapshots.len() - to_expire.len(),
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json)
                        .map_err(|e| Error::General(e.to_string()))?
                );
            }
            return Ok(());
        }

        // Use TableMetadataBuilder to properly remove snapshots
        // This handles snapshots, snapshot-log, and refs correctly
        let metadata_file_path = service.current_metadata_path().await?;
        let metadata_clone = (*metadata).clone();
        let build_result = metadata_clone
            .into_builder(Some(metadata_file_path.clone()))
            .remove_snapshots(&to_expire)
            .build()
            .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

        let new_metadata = build_result.metadata;

        // Write new metadata file with incremented version
        let storage = StorageBackendFactory::create_backend(path).await?;
        let metadata_dir = format!("{}/metadata", path.trim_end_matches('/'));
        let new_version = current_version + 1;
        let new_metadata_filename = format!("v{}.metadata.json", new_version);
        let new_metadata_path = format!("{}/{}", metadata_dir, new_metadata_filename);

        let new_metadata_bytes = serde_json::to_vec_pretty(&new_metadata)
            .map_err(|e| Error::General(format!("Failed to serialize metadata: {}", e)))?;

        storage
            .put(
                &new_metadata_path,
                bytes::Bytes::from(new_metadata_bytes),
                &PutOptions::default(),
            )
            .await?;

        // Update version-hint.text
        let version_hint_path = format!("{}/version-hint.text", metadata_dir);
        storage
            .put(
                &version_hint_path,
                bytes::Bytes::from(new_version.to_string()),
                &PutOptions::default(),
            )
            .await?;

        if output == "json" {
            let json = serde_json::json!({
                "expired_snapshots": to_expire,
                "new_metadata_version": new_version,
                "remaining_snapshots": snapshots.len() - to_expire.len(),
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!();
            println!("{}", "Expire complete!".green().bold());
            println!("Snapshots expired: {}", to_expire.len());
            println!("Remaining snapshots: {}", snapshots.len() - to_expire.len());
            println!("New metadata version: {}", new_version);
            println!();
            println!(
                "{}",
                "Note: Data files are NOT deleted. Use 'icectl vacuum' to remove orphaned data files.".dimmed()
            );
        }

        Ok(())
    }

    #[cfg(feature = "iceberg")]
    async fn iceberg_set(
        path: &str,
        id: Option<i64>,
        as_of: Option<String>,
        output: &str,
    ) -> Result<()> {
        use crate::core::metadata::IcebergMetadataService;

        let service = IcebergMetadataService::new_async(path.to_string()).await?;
        let (metadata, _) = service.load_metadata().await?;

        let target_id = if let Some(id) = id {
            id
        } else if let Some(as_of) = &as_of {
            let cutoff = parse_timestamp(as_of)?;
            let cutoff_ms = cutoff.timestamp_millis();

            // Find snapshot at or before timestamp
            let snapshots: Vec<_> = metadata.snapshots().collect();
            let mut best: Option<i64> = None;
            let mut best_ts = i64::MIN;

            for snap in &snapshots {
                if snap.timestamp_ms() <= cutoff_ms && snap.timestamp_ms() > best_ts {
                    best = Some(snap.snapshot_id());
                    best_ts = snap.timestamp_ms();
                }
            }

            best.ok_or_else(|| Error::General(format!("No snapshot found before {}", as_of)))?
        } else {
            return Err(Error::General("Must specify --id or --as-of".to_string()));
        };

        println!(
            "{} Iceberg table to snapshot {}",
            "Setting".green(),
            target_id
        );

        // Verify snapshot exists
        let snapshots: Vec<_> = metadata.snapshots().collect();
        if !snapshots.iter().any(|s| s.snapshot_id() == target_id) {
            return Err(Error::General(format!("Snapshot {} not found", target_id)));
        }

        if output == "json" {
            let json = serde_json::json!({
                "snapshot_id": target_id,
                "status": "validated",
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!();
            println!("{}", "Snapshot validated!".green().bold());
            println!("Snapshot ID: {}", target_id.to_string().cyan());
            println!();
            println!(
                "{}",
                "Note: To set as current, use 'icectl restore --version'".dimmed()
            );
        }

        Ok(())
    }

    #[cfg(feature = "iceberg")]
    async fn iceberg_cherrypick(path: &str, snapshot_id: i64, _output: &str) -> Result<()> {
        use crate::core::metadata::IcebergMetadataService;

        println!(
            "{} snapshot {} to {}",
            "Cherry-picking".green(),
            snapshot_id,
            path
        );

        let service = IcebergMetadataService::new_async(path.to_string()).await?;
        let (metadata, _) = service.load_metadata().await?;

        // Verify source snapshot exists
        let snapshots: Vec<_> = metadata.snapshots().collect();
        let source = snapshots
            .iter()
            .find(|s| s.snapshot_id() == snapshot_id)
            .ok_or_else(|| Error::General(format!("Snapshot {} not found", snapshot_id)))?;

        println!();
        println!("Source snapshot: {}", snapshot_id);
        println!(
            "  Timestamp: {}",
            chrono::DateTime::from_timestamp_millis(source.timestamp_ms())
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| source.timestamp_ms().to_string())
        );

        // TODO: Implement actual cherry-pick using iceberg-rs Transaction API
        println!();
        println!(
            "{}",
            "Cherry-pick requires iceberg-rs Transaction API".yellow()
        );
        println!(
            "{}",
            "This is a planned feature - use catalog tools for now".dimmed()
        );

        Ok(())
    }

    #[cfg(not(feature = "iceberg"))]
    async fn execute_iceberg(
        _args: SnapshotArgs,
        _table_path: &str,
        _storage: Arc<dyn StorageBackend>,
    ) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Iceberg support not enabled".to_string(),
        })
    }
}

/// Parse timestamp string to DateTime
///
/// Supports:
/// - Absolute: "2024-01-15", "2024-01-15T10:30:00", "2024-01-15 10:30:00"
/// - Relative durations: "7d" (days), "24h" (hours), "30m" (minutes), "2w" (weeks)
fn parse_timestamp(s: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    let s = s.trim();

    // Try relative duration first (e.g., "7d", "24h", "30m", "2w")
    if let Some(duration) = parse_relative_duration(s) {
        return Ok(chrono::Utc::now() - duration);
    }

    // Try absolute formats
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(dt.and_utc());
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(dt.and_utc());
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(date.and_hms_opt(0, 0, 0).unwrap().and_utc());
    }

    Err(Error::General(format!(
        "Invalid timestamp format: '{}'. Expected:\n  \
         - Relative: 7d (days), 24h (hours), 30m (minutes), 2w (weeks)\n  \
         - Absolute: YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS",
        s
    )))
}

/// Parse relative duration string (e.g., "7d", "24h", "30m", "2w")
fn parse_relative_duration(s: &str) -> Option<chrono::Duration> {
    let s = s.trim().to_lowercase();

    if s.is_empty() {
        return None;
    }

    // Split into number and unit
    let (num_str, unit) = if s.ends_with('d') {
        (&s[..s.len() - 1], 'd')
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], 'h')
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], 'm')
    } else if s.ends_with('w') {
        (&s[..s.len() - 1], 'w')
    } else if s.ends_with('s') {
        (&s[..s.len() - 1], 's')
    } else {
        return None;
    };

    let num: i64 = num_str.parse().ok()?;

    match unit {
        'w' => Some(chrono::Duration::weeks(num)),
        'd' => Some(chrono::Duration::days(num)),
        'h' => Some(chrono::Duration::hours(num)),
        'm' => Some(chrono::Duration::minutes(num)),
        's' => Some(chrono::Duration::seconds(num)),
        _ => None,
    }
}

