//! Restore command implementation
//!
//! Restores an Iceberg table to a previous snapshot.

use colored::Colorize;

use crate::cli::parser::RestoreArgs;
use crate::config::ResolvePath;
use crate::error::{Error, Result};

/// Handler for restore command
pub struct RestoreCommand;

impl RestoreCommand {
    /// Execute restore command
    pub async fn execute(args: RestoreArgs) -> Result<()> {
        let path_str = args.path.resolve()?;

        // Validate that either version or as_of is provided
        if args.version.is_none() && args.as_of.is_none() {
            return Err(Error::General(
                "Must specify either --version (snapshot ID) or --as-of".to_string(),
            ));
        }

        let path = std::path::Path::new(&path_str);

        // Verify it's an Iceberg table
        if !path.join("metadata").exists() {
            return Err(Error::General(format!(
                "Path '{}' is not an Iceberg table (no metadata directory found)",
                path_str
            )));
        }

        Self::restore_iceberg(&path_str, &args).await
    }

    /// Restore Iceberg table
    async fn restore_iceberg(path_str: &str, args: &RestoreArgs) -> Result<()> {
        use chrono::{NaiveDate, NaiveDateTime, TimeZone, Utc};
        use iceberg::TableIdent;
        use iceberg::io::FileIOBuilder;
        use iceberg::spec::{MAIN_BRANCH, TableMetadataBuilder};
        use iceberg::table::StaticTable;

        println!(
            "{} Iceberg table at {}",
            if args.dry_run {
                "Analyzing"
            } else {
                "Restoring"
            }
            .green(),
            path_str
        );

        let table_path = std::path::Path::new(path_str);
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

        let static_table =
            StaticTable::from_metadata_file(&metadata_file.to_string_lossy(), table_ident, file_io)
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

            best_snapshot
                .ok_or_else(|| Error::General(format!("No snapshot found at or before '{}'", ts)))?
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

        let new_metadata =
            TableMetadataBuilder::new_from_metadata(old_metadata, Some(metadata_log_path))
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
}
