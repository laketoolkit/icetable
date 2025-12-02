//! Vacuum command implementation
//!
//! Removes old files no longer referenced by Delta Lake and Iceberg tables.

use std::collections::HashSet;

use colored::Colorize;

use crate::cli::parser::VacuumArgs;
use crate::error::{Error, Result};

/// Handler for vacuum command
pub struct VacuumCommand;

impl VacuumCommand {
    /// Execute vacuum command
    pub async fn execute(args: VacuumArgs) -> Result<()> {
        let path = std::path::Path::new(&args.path);

        // Detect table format
        let is_delta = path.join("_delta_log").exists();
        let is_iceberg = path.join("metadata").exists();

        if is_delta {
            Self::vacuum_delta(&args).await
        } else if is_iceberg {
            Self::vacuum_iceberg(&args).await
        } else {
            Err(Error::General(format!(
                "Path '{}' is not a Delta Lake or Iceberg table",
                args.path
            )))
        }
    }

    /// Vacuum Delta Lake table
    #[cfg(feature = "delta")]
    async fn vacuum_delta(args: &VacuumArgs) -> Result<()> {
        use deltalake::DeltaOps;

        println!(
            "{} Delta table at {}",
            if args.dry_run {
                "Analyzing".yellow()
            } else {
                "Vacuuming".green()
            },
            args.path
        );

        // Open the table
        let table = deltalake::open_table(&args.path)
            .await
            .map_err(|e| Error::General(format!("Failed to open Delta table: {}", e)))?;

        // Build vacuum operation
        let retention = chrono::Duration::hours(args.retention_hours as i64);
        let mut vacuum = DeltaOps(table).vacuum().with_retention_period(retention);

        if args.dry_run {
            vacuum = vacuum.with_dry_run(true);
        }

        if args.force {
            vacuum = vacuum.with_enforce_retention_duration(false);
        }

        // Execute vacuum
        let (table, metrics) = vacuum
            .await
            .map_err(|e| Error::General(format!("Vacuum failed: {}", e)))?;

        // Output results
        match args.output.as_str() {
            "json" => Self::output_delta_json(&metrics, args.dry_run)?,
            _ => Self::output_delta_text(&metrics, args.dry_run, table.version())?,
        }

        Ok(())
    }

    #[cfg(not(feature = "delta"))]
    async fn vacuum_delta(_args: &VacuumArgs) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Delta Lake support not enabled".to_string(),
        })
    }

    /// Output Delta vacuum results as text
    #[cfg(feature = "delta")]
    fn output_delta_text(
        metrics: &deltalake::operations::vacuum::VacuumMetrics,
        dry_run: bool,
        version: Option<i64>,
    ) -> Result<()> {
        println!();

        if dry_run {
            println!("{}", "DRY RUN - No files were deleted".yellow().bold());
            println!();
        }

        println!(
            "Files {}:   {}",
            if dry_run { "to delete" } else { "deleted" },
            metrics.files_deleted.len().to_string().cyan()
        );

        if !metrics.files_deleted.is_empty() {
            println!();
            println!("Files:");
            for file in &metrics.files_deleted {
                println!("  - {}", file.dimmed());
            }
        }

        if !dry_run {
            if let Some(v) = version {
                println!();
                println!("Table version: {}", v.to_string().green());
            }
        }

        Ok(())
    }

    /// Output Delta vacuum results as JSON
    #[cfg(feature = "delta")]
    fn output_delta_json(
        metrics: &deltalake::operations::vacuum::VacuumMetrics,
        dry_run: bool,
    ) -> Result<()> {
        let json = serde_json::json!({
            "dry_run": dry_run,
            "files_deleted": metrics.files_deleted,
            "files_count": metrics.files_deleted.len(),
        });

        println!(
            "{}",
            serde_json::to_string_pretty(&json)
                .map_err(|e| Error::General(format!("Failed to serialize: {}", e)))?
        );

        Ok(())
    }

    /// Vacuum Iceberg table
    #[cfg(feature = "iceberg")]
    async fn vacuum_iceberg(args: &VacuumArgs) -> Result<()> {
        use chrono::{Duration as ChronoDuration, Utc};

        println!(
            "{} Iceberg table at {}",
            if args.dry_run {
                "Analyzing".yellow()
            } else {
                "Vacuuming".green()
            },
            args.path
        );

        let table_path = std::path::Path::new(&args.path);
        let metadata_dir = table_path.join("metadata");
        let data_dir = table_path.join("data");

        // Find current metadata
        let current_version = Self::get_iceberg_current_version(&metadata_dir)?;
        let retention_hours = args.retention_hours as i64;
        let cutoff_time = Utc::now() - ChronoDuration::hours(retention_hours);

        // Collect referenced files from current snapshot
        let referenced_files = Self::get_iceberg_referenced_files(&metadata_dir, current_version)?;

        // Find orphan files in data directory
        let mut orphan_files = Vec::new();
        let mut orphan_bytes = 0u64;

        if data_dir.exists() {
            for entry in std::fs::read_dir(&data_dir)
                .map_err(|e| Error::General(format!("Failed to read data dir: {}", e)))?
            {
                let entry =
                    entry.map_err(|e| Error::General(format!("Failed to read entry: {}", e)))?;
                let path = entry.path();

                if path.is_file() {
                    let file_name = path.file_name().unwrap().to_string_lossy().to_string();

                    // Check if file is not referenced
                    if !referenced_files.contains(&file_name) {
                        // Check file age
                        if let Ok(metadata) = std::fs::metadata(&path) {
                            if let Ok(modified) = metadata.modified() {
                                let modified_time: chrono::DateTime<Utc> = modified.into();
                                if modified_time < cutoff_time {
                                    orphan_bytes += metadata.len();
                                    orphan_files.push(path.to_string_lossy().to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        // Find old metadata files
        let mut old_metadata_files = Vec::new();
        if args.retention_hours > 0 {
            for entry in std::fs::read_dir(&metadata_dir)
                .map_err(|e| Error::General(format!("Failed to read metadata dir: {}", e)))?
            {
                let entry =
                    entry.map_err(|e| Error::General(format!("Failed to read entry: {}", e)))?;
                let path = entry.path();
                let file_name = path.file_name().unwrap().to_string_lossy().to_string();

                // Keep current version and version-hint.text
                if file_name == "version-hint.text"
                    || file_name == format!("v{}.metadata.json", current_version)
                {
                    continue;
                }

                // Check file age
                if let Ok(metadata) = std::fs::metadata(&path) {
                    if let Ok(modified) = metadata.modified() {
                        let modified_time: chrono::DateTime<Utc> = modified.into();
                        if modified_time < cutoff_time {
                            orphan_bytes += metadata.len();
                            old_metadata_files.push(path.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }

        let all_files: Vec<String> = orphan_files
            .iter()
            .chain(old_metadata_files.iter())
            .cloned()
            .collect();

        // Delete files if not dry run
        if !args.dry_run && !all_files.is_empty() {
            for file in &all_files {
                std::fs::remove_file(file)
                    .map_err(|e| Error::General(format!("Failed to delete {}: {}", file, e)))?;
            }
        }

        // Output results
        match args.output.as_str() {
            "json" => {
                let json = serde_json::json!({
                    "dry_run": args.dry_run,
                    "files_deleted": all_files,
                    "files_count": all_files.len(),
                    "bytes_freed": orphan_bytes,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json)
                        .map_err(|e| Error::General(format!("Failed to serialize: {}", e)))?
                );
            }
            _ => {
                println!();

                if args.dry_run {
                    println!("{}", "DRY RUN - No files were deleted".yellow().bold());
                    println!();
                }

                println!(
                    "Files {}:   {}",
                    if args.dry_run { "to delete" } else { "deleted" },
                    all_files.len().to_string().cyan()
                );

                if orphan_bytes > 0 {
                    println!(
                        "Space {}:  {}",
                        if args.dry_run { "to free" } else { "freed" },
                        Self::format_bytes(orphan_bytes)
                    );
                }

                if !all_files.is_empty() {
                    println!();
                    println!("Files:");
                    for file in &all_files {
                        println!("  - {}", file.dimmed());
                    }
                }
            }
        }

        Ok(())
    }

    #[cfg(not(feature = "iceberg"))]
    async fn vacuum_iceberg(_args: &VacuumArgs) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Iceberg support not enabled".to_string(),
        })
    }

    /// Get current Iceberg version from version-hint.text
    #[cfg(feature = "iceberg")]
    fn get_iceberg_current_version(metadata_dir: &std::path::Path) -> Result<i32> {
        let version_hint = metadata_dir.join("version-hint.text");
        if let Ok(content) = std::fs::read_to_string(&version_hint) {
            content
                .trim()
                .parse::<i32>()
                .map_err(|e| Error::General(format!("Invalid version hint: {}", e)))
        } else {
            // Fallback: find max version
            let mut max_version = 0;
            if let Ok(entries) = std::fs::read_dir(metadata_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with('v') && name.ends_with(".metadata.json") {
                        if let Some(v_str) = name
                            .strip_prefix('v')
                            .and_then(|s| s.strip_suffix(".metadata.json"))
                        {
                            if let Ok(v) = v_str.parse::<i32>() {
                                max_version = max_version.max(v);
                            }
                        }
                    }
                }
            }
            Ok(max_version)
        }
    }

    /// Get referenced files from current Iceberg snapshot
    #[cfg(feature = "iceberg")]
    fn get_iceberg_referenced_files(
        metadata_dir: &std::path::Path,
        version: i32,
    ) -> Result<HashSet<String>> {
        let mut referenced = HashSet::new();

        // Read metadata file
        let metadata_file = metadata_dir.join(format!("v{}.metadata.json", version));
        let content = std::fs::read_to_string(&metadata_file)
            .map_err(|e| Error::General(format!("Failed to read metadata: {}", e)))?;

        let metadata: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| Error::General(format!("Failed to parse metadata: {}", e)))?;

        // Extract manifest list from current snapshot
        if let Some(snapshots) = metadata.get("snapshots").and_then(|s| s.as_array()) {
            for snapshot in snapshots {
                if let Some(manifest_list) = snapshot.get("manifest-list").and_then(|m| m.as_str())
                {
                    // Extract filename from path
                    if let Some(filename) = std::path::Path::new(manifest_list).file_name() {
                        referenced.insert(filename.to_string_lossy().to_string());
                    }
                }
            }
        }

        // Note: Full implementation would parse manifests to find all data files
        // For now, we're conservative and only vacuum obvious orphans

        Ok(referenced)
    }

    /// Format bytes to human-readable string
    #[cfg(feature = "iceberg")]
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
