//! Snapshot command implementation
//!
//! Creates checkpoints for Delta Lake and Iceberg tables.

use colored::Colorize;

use crate::cli::parser::SnapshotArgs;
use crate::error::{Error, Result};

/// Handler for snapshot command
pub struct SnapshotCommand;

impl SnapshotCommand {
    /// Execute snapshot command
    pub async fn execute(args: SnapshotArgs) -> Result<()> {
        let path = std::path::Path::new(&args.path);

        // Detect table format
        let is_delta = path.join("_delta_log").exists();
        let is_iceberg = path.join("metadata").exists();

        if is_delta {
            Self::snapshot_delta(&args).await
        } else if is_iceberg {
            Self::snapshot_iceberg(&args).await
        } else {
            Err(Error::General(format!(
                "Path '{}' is not a Delta Lake or Iceberg table",
                args.path
            )))
        }
    }

    /// Create Delta Lake checkpoint
    #[cfg(feature = "delta")]
    async fn snapshot_delta(args: &SnapshotArgs) -> Result<()> {
        use deltalake::checkpoints::create_checkpoint;

        println!("{} Delta checkpoint at {}", "Creating".green(), args.path);

        // Open the table
        let table = deltalake::open_table(&args.path)
            .await
            .map_err(|e| Error::General(format!("Failed to open Delta table: {}", e)))?;

        let version = table.version().unwrap_or(0);

        // Check if checkpoint already exists for this version
        let log_dir = std::path::Path::new(&args.path).join("_delta_log");
        let checkpoint_file = log_dir.join(format!("{:020}.checkpoint.parquet", version));

        if checkpoint_file.exists() && !args.force {
            println!();
            println!(
                "{}",
                format!("Checkpoint already exists for version {}", version).yellow()
            );
            println!("Use --force to create anyway");
            return Ok(());
        }

        // Create checkpoint
        create_checkpoint(&table, None)
            .await
            .map_err(|e| Error::General(format!("Failed to create checkpoint: {}", e)))?;

        // Get checkpoint info
        let checkpoint_size = std::fs::metadata(&checkpoint_file)
            .map(|m| m.len())
            .unwrap_or(0);

        match args.output.as_str() {
            "json" => {
                let json = serde_json::json!({
                    "version": version,
                    "checkpoint_file": checkpoint_file.to_string_lossy(),
                    "size_bytes": checkpoint_size,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json)
                        .map_err(|e| Error::General(format!("Failed to serialize: {}", e)))?
                );
            }
            _ => {
                println!();
                println!("{}", "Checkpoint created!".green().bold());
                println!();
                println!("Version:    {}", version.to_string().cyan());
                println!("File:       {}", checkpoint_file.display());
                println!("Size:       {}", Self::format_bytes(checkpoint_size));
            }
        }

        Ok(())
    }

    #[cfg(not(feature = "delta"))]
    async fn snapshot_delta(_args: &SnapshotArgs) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Delta Lake support not enabled".to_string(),
        })
    }

    /// Create Iceberg snapshot metadata
    #[cfg(feature = "iceberg")]
    async fn snapshot_iceberg(args: &SnapshotArgs) -> Result<()> {
        // Iceberg doesn't have checkpoints in the same way as Delta
        // But we can create a metadata snapshot for backup purposes

        println!(
            "{} Iceberg metadata snapshot at {}",
            "Creating".green(),
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

        let metadata_file = metadata_dir.join(format!("v{}.metadata.json", current_version));

        if !metadata_file.exists() {
            return Err(Error::General(format!(
                "Metadata file not found: {}",
                metadata_file.display()
            )));
        }

        // Create a backup copy with timestamp
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

        match args.output.as_str() {
            "json" => {
                let json = serde_json::json!({
                    "version": current_version,
                    "backup_file": backup_file.to_string_lossy(),
                    "size_bytes": backup_size,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json)
                        .map_err(|e| Error::General(format!("Failed to serialize: {}", e)))?
                );
            }
            _ => {
                println!();
                println!("{}", "Metadata snapshot created!".green().bold());
                println!();
                println!("Version:    {}", current_version.to_string().cyan());
                println!("Backup:     {}", backup_file.display());
                println!("Size:       {}", Self::format_bytes(backup_size));
            }
        }

        Ok(())
    }

    #[cfg(not(feature = "iceberg"))]
    async fn snapshot_iceberg(_args: &SnapshotArgs) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Iceberg support not enabled".to_string(),
        })
    }

    /// Format bytes to human-readable string
    fn format_bytes(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} bytes", bytes)
        }
    }
}
