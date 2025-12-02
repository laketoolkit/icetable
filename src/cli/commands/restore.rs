//! Restore command implementation
//!
//! Restores a Delta Lake or Iceberg table to a previous version.

use colored::Colorize;

use crate::cli::parser::RestoreArgs;
use crate::error::{Error, Result};

/// Handler for restore command
pub struct RestoreCommand;

impl RestoreCommand {
    /// Execute restore command
    pub async fn execute(args: RestoreArgs) -> Result<()> {
        // Validate that either version or as_of is provided
        if args.version.is_none() && args.as_of.is_none() {
            return Err(Error::General(
                "Must specify either --version or --as-of".to_string(),
            ));
        }

        let path = std::path::Path::new(&args.path);

        // Detect table format
        let is_delta = path.join("_delta_log").exists();
        let is_iceberg = path.join("metadata").exists();

        if is_delta {
            Self::restore_delta(&args).await
        } else if is_iceberg {
            Self::restore_iceberg(&args).await
        } else {
            Err(Error::General(format!(
                "Path '{}' is not a Delta Lake or Iceberg table",
                args.path
            )))
        }
    }

    /// Restore Delta Lake table
    #[cfg(feature = "delta")]
    async fn restore_delta(args: &RestoreArgs) -> Result<()> {
        use deltalake::kernel::Action;
        use deltalake::protocol::DeltaOperation;
        use deltalake::DeltaTableBuilder;

        println!(
            "{} Delta table at {}",
            if args.dry_run { "Analyzing" } else { "Restoring" }.green(),
            args.path
        );

        // Open current table
        let table = deltalake::open_table(&args.path)
            .await
            .map_err(|e| Error::General(format!("Failed to open Delta table: {}", e)))?;

        let current_version = table.version().unwrap_or(0);

        // Determine target version
        let target_version = if let Some(v) = args.version {
            v
        } else if let Some(ref ts) = args.as_of {
            Self::find_delta_version_at_timestamp(&args.path, ts).await?
        } else {
            return Err(Error::General("No target version specified".to_string()));
        };

        if target_version >= current_version {
            return Err(Error::General(format!(
                "Target version {} must be less than current version {}",
                target_version, current_version
            )));
        }

        println!(
            "Current version: {}, Target version: {}",
            current_version.to_string().cyan(),
            target_version.to_string().green()
        );

        // Load the target version
        let target_table = DeltaTableBuilder::from_uri(&args.path)
            .with_version(target_version)
            .load()
            .await
            .map_err(|e| Error::General(format!("Failed to load version {}: {}", target_version, e)))?;

        // Get files at target version
        let target_files: Vec<String> = target_table
            .get_file_uris()
            .map_err(|e| Error::General(format!("Failed to get target files: {}", e)))?
            .collect();

        // Get files at current version
        let current_files: Vec<String> = table
            .get_file_uris()
            .map_err(|e| Error::General(format!("Failed to get current files: {}", e)))?
            .collect();

        // Calculate diff
        let files_to_add: Vec<&String> = target_files
            .iter()
            .filter(|f| !current_files.contains(f))
            .collect();
        let files_to_remove: Vec<&String> = current_files
            .iter()
            .filter(|f| !target_files.contains(f))
            .collect();

        println!();
        println!("Changes:");
        println!("  Files to restore: {}", files_to_add.len().to_string().green());
        println!("  Files to remove:  {}", files_to_remove.len().to_string().red());

        if args.dry_run {
            println!();
            println!("{}", "DRY RUN - No changes made".yellow().bold());

            if !files_to_add.is_empty() {
                println!();
                println!("Files to restore:");
                for f in &files_to_add {
                    let name = f.rsplit('/').next().unwrap_or(f);
                    println!("  + {}", name.green());
                }
            }

            if !files_to_remove.is_empty() {
                println!();
                println!("Files to remove:");
                for f in &files_to_remove {
                    let name = f.rsplit('/').next().unwrap_or(f);
                    println!("  - {}", name.red());
                }
            }

            return Ok(());
        }

        // Build restore actions
        let mut actions: Vec<Action> = Vec::new();

        // Add actions for files to restore
        for file_uri in &files_to_add {
            let rel_path = file_uri
                .strip_prefix("file://")
                .unwrap_or(file_uri)
                .strip_prefix(&args.path)
                .unwrap_or(file_uri)
                .trim_start_matches('/');

            let full_path = if file_uri.starts_with("file://") {
                &file_uri[7..]
            } else {
                file_uri.as_str()
            };

            let size = std::fs::metadata(full_path)
                .map(|m| m.len() as i64)
                .unwrap_or(0);

            actions.push(Action::Add(deltalake::kernel::Add {
                path: rel_path.to_string(),
                partition_values: std::collections::HashMap::new(),
                size,
                modification_time: chrono::Utc::now().timestamp_millis(),
                data_change: true,
                stats: None,
                tags: None,
                deletion_vector: None,
                base_row_id: None,
                default_row_commit_version: None,
                clustering_provider: None,
            }));
        }

        // Remove actions for files to remove
        for file_uri in &files_to_remove {
            let rel_path = file_uri
                .strip_prefix("file://")
                .unwrap_or(file_uri)
                .strip_prefix(&args.path)
                .unwrap_or(file_uri)
                .trim_start_matches('/');

            let full_path = if file_uri.starts_with("file://") {
                &file_uri[7..]
            } else {
                file_uri.as_str()
            };

            let size = std::fs::metadata(full_path)
                .map(|m| m.len() as i64)
                .ok();

            actions.push(Action::Remove(deltalake::kernel::Remove {
                path: rel_path.to_string(),
                deletion_timestamp: Some(chrono::Utc::now().timestamp_millis()),
                data_change: true,
                extended_file_metadata: None,
                partition_values: None,
                size,
                deletion_vector: None,
                base_row_id: None,
                default_row_commit_version: None,
                tags: None,
            }));
        }

        // Commit restore
        let log_store = table.log_store();
        let snapshot = table
            .snapshot()
            .map_err(|e| Error::General(format!("Failed to get snapshot: {}", e)))?;

        deltalake::kernel::transaction::CommitBuilder::default()
            .with_actions(actions)
            .build(
                Some(snapshot),
                log_store,
                DeltaOperation::Restore {
                    version: Some(target_version),
                    datetime: None,
                },
            )
            .await
            .map_err(|e| Error::General(format!("Failed to commit restore: {}", e)))?;

        println!();
        println!("{}", "Restore complete!".green().bold());
        println!("Table restored to version {}", target_version);

        Ok(())
    }

    /// Find Delta version at a given timestamp
    #[cfg(feature = "delta")]
    async fn find_delta_version_at_timestamp(path: &str, ts: &str) -> Result<i64> {
        use chrono::{NaiveDate, NaiveDateTime, TimeZone, Utc};

        // Parse timestamp
        let target_ms = if let Ok(dt) = NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S") {
            Utc.from_utc_datetime(&dt).timestamp_millis()
        } else if let Ok(date) = NaiveDate::parse_from_str(ts, "%Y-%m-%d") {
            let dt = date.and_hms_opt(23, 59, 59).unwrap();
            Utc.from_utc_datetime(&dt).timestamp_millis()
        } else {
            return Err(Error::General(format!(
                "Invalid timestamp '{}'. Use YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS",
                ts
            )));
        };

        // Read commit timestamps from log
        let log_dir = std::path::Path::new(path).join("_delta_log");
        let mut versions: Vec<(i64, i64)> = Vec::new(); // (version, timestamp)

        for entry in std::fs::read_dir(&log_dir)
            .map_err(|e| Error::General(format!("Failed to read log: {}", e)))?
        {
            let entry = entry.map_err(|e| Error::General(format!("Read error: {}", e)))?;
            let name = entry.file_name().to_string_lossy().to_string();

            if name.ends_with(".json") && !name.contains("checkpoint") {
                if let Ok(version) = name.trim_end_matches(".json").parse::<i64>() {
                    // Read commit timestamp
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        for line in content.lines() {
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                                if let Some(commit) = json.get("commitInfo") {
                                    if let Some(ts) = commit.get("timestamp").and_then(|t| t.as_i64()) {
                                        versions.push((version, ts));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        versions.sort_by_key(|(v, _)| *v);

        // Find latest version at or before target timestamp
        let mut best_version = 0i64;
        for (version, ts) in versions {
            if ts <= target_ms {
                best_version = version;
            } else {
                break;
            }
        }

        Ok(best_version)
    }

    #[cfg(not(feature = "delta"))]
    async fn restore_delta(_args: &RestoreArgs) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Delta Lake support not enabled".to_string(),
        })
    }

    /// Restore Iceberg table
    #[cfg(feature = "iceberg")]
    async fn restore_iceberg(args: &RestoreArgs) -> Result<()> {
        use chrono::{NaiveDate, NaiveDateTime, TimeZone, Utc};
        use iceberg::io::FileIOBuilder;
        use iceberg::spec::{TableMetadataBuilder, MAIN_BRANCH};
        use iceberg::table::StaticTable;
        use iceberg::TableIdent;

        println!(
            "{} Iceberg table at {}",
            if args.dry_run { "Analyzing" } else { "Restoring" }.green(),
            args.path
        );

        let table_path = std::path::Path::new(&args.path);
        let metadata_dir = table_path.join("metadata");

        // Get current version
        let version_hint = metadata_dir.join("version-hint.text");
        let current_version: i32 = std::fs::read_to_string(&version_hint)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1);

        // Load current metadata
        let metadata_file = metadata_dir.join(format!("v{}.metadata.json", current_version));
        let file_io = FileIOBuilder::new_fs_io()
            .build()
            .map_err(|e| Error::General(format!("Failed to create FileIO: {}", e)))?;

        let table_ident = TableIdent::from_strs(&["iceberg", "table"])
            .map_err(|e| Error::General(format!("Failed to create ident: {}", e)))?;

        let static_table = StaticTable::from_metadata_file(
            &metadata_file.to_string_lossy(),
            table_ident,
            file_io,
        )
        .await
        .map_err(|e| Error::General(format!("Failed to load table: {}", e)))?;

        let metadata = static_table.metadata();

        // Find target snapshot
        let target_snapshot = if let Some(snapshot_id) = args.version {
            metadata
                .snapshot_by_id(snapshot_id)
                .ok_or_else(|| Error::General(format!("Snapshot {} not found", snapshot_id)))?
        } else if let Some(ref ts) = args.as_of {
            // Parse timestamp
            let target_ms = if let Ok(dt) = NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S") {
                Utc.from_utc_datetime(&dt).timestamp_millis()
            } else if let Ok(date) = NaiveDate::parse_from_str(ts, "%Y-%m-%d") {
                let dt = date.and_hms_opt(23, 59, 59).unwrap();
                Utc.from_utc_datetime(&dt).timestamp_millis()
            } else {
                return Err(Error::General(format!("Invalid timestamp '{}'", ts)));
            };

            // Find snapshot at or before timestamp
            let mut best_snapshot = None;
            let mut best_ts = 0i64;

            for snapshot in metadata.snapshots() {
                let ts = snapshot.timestamp_ms();
                if ts <= target_ms && ts > best_ts {
                    best_ts = ts;
                    best_snapshot = Some(snapshot);
                }
            }

            best_snapshot.ok_or_else(|| {
                Error::General(format!("No snapshot found at or before '{}'", ts))
            })?
        } else {
            return Err(Error::General("No target version specified".to_string()));
        };

        let current_snapshot = metadata.current_snapshot();

        println!(
            "Current snapshot: {}",
            current_snapshot
                .map(|s| s.snapshot_id().to_string())
                .unwrap_or_else(|| "none".to_string())
                .cyan()
        );
        println!(
            "Target snapshot:  {}",
            target_snapshot.snapshot_id().to_string().green()
        );

        if args.dry_run {
            println!();
            println!("{}", "DRY RUN - No changes made".yellow().bold());
            return Ok(());
        }

        // Create new metadata pointing to target snapshot
        let old_metadata: iceberg::spec::TableMetadata = (*metadata).clone();
        let metadata_log_path = format!("v{}.metadata.json", current_version);

        let new_metadata = TableMetadataBuilder::new_from_metadata(old_metadata, Some(metadata_log_path))
            .set_branch_snapshot(target_snapshot.as_ref().clone(), MAIN_BRANCH)
            .map_err(|e| Error::General(format!("Failed to set snapshot: {}", e)))?
            .build()
            .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

        // Write new metadata
        let new_version = current_version + 1;
        let new_metadata_file = metadata_dir.join(format!("v{}.metadata.json", new_version));

        let metadata_json = serde_json::to_string_pretty(&new_metadata.metadata)
            .map_err(|e| Error::General(format!("Failed to serialize: {}", e)))?;

        std::fs::write(&new_metadata_file, metadata_json)
            .map_err(|e| Error::General(format!("Failed to write metadata: {}", e)))?;

        std::fs::write(&version_hint, new_version.to_string())
            .map_err(|e| Error::General(format!("Failed to update version: {}", e)))?;

        println!();
        println!("{}", "Restore complete!".green().bold());
        println!(
            "Table restored to snapshot {}",
            target_snapshot.snapshot_id()
        );

        Ok(())
    }

    #[cfg(not(feature = "iceberg"))]
    async fn restore_iceberg(_args: &RestoreArgs) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Iceberg support not enabled".to_string(),
        })
    }
}
