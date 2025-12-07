//! Vacuum command implementation
//!
//! Removes old files no longer referenced by Iceberg tables.

use colored::Colorize;

use super::common::resolve_table_path;
use crate::cli::parser::VacuumArgs;
use crate::core::format_bytes;
use crate::core::CatalogConfig;
use crate::error::{Error, Result};

/// Handler for vacuum command
pub struct VacuumCommand;

impl VacuumCommand {
    /// Execute vacuum command
    pub async fn execute(args: VacuumArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        let table_path = resolve_table_path(&args.path, catalog_config.as_ref()).await?;
        Self::vacuum_iceberg(&table_path, &args).await
    }

    /// Vacuum Iceberg table
    async fn vacuum_iceberg(table_path: &str, args: &VacuumArgs) -> Result<()> {
        use crate::core::metadata::IcebergMetadataService;
        use crate::core::storage::StorageBackendFactory;
        use crate::core::storage::traits::ListOptions;
        use futures::stream::{self, StreamExt};
        use iceberg::spec::ManifestList;
        use indicatif::{ProgressBar, ProgressStyle};
        use std::collections::HashSet;

        // Note: Branch parameter is accepted for API consistency but vacuum always
        // considers ALL snapshots to ensure safety. A file is only orphaned if
        // it's not referenced by ANY snapshot in the table.
        if let Some(ref branch) = args.branch {
            println!(
                "{} Iceberg table at {} (branch: {})",
                if args.dry_run {
                    "Analyzing".yellow()
                } else {
                    "Vacuuming".green()
                },
                table_path,
                branch.cyan()
            );
            println!(
                "{}",
                "Note: Vacuum always considers all snapshots for safety".dimmed()
            );
        } else {
            println!(
                "{} Iceberg table at {}",
                if args.dry_run {
                    "Analyzing".yellow()
                } else {
                    "Vacuuming".green()
                },
                table_path
            );
        }

        // Load metadata
        // Note: We don't use branch-specific metadata here because vacuum must
        // consider ALL snapshots to avoid deleting files that any branch needs
        let service = match IcebergMetadataService::new_async(table_path.to_string()).await {
            Ok(s) => s,
            Err(_) => {
                return Err(Error::General(format!(
                    "Path '{}' is not an Iceberg table",
                    table_path
                )));
            }
        };
        let (metadata, _) = service.load_metadata().await?;
        let file_io = service.file_io().clone();

        let snapshots: Vec<_> = metadata.snapshots().collect();

        // Step 1: Collect all unique manifest entries from all snapshots
        let mut seen_manifest_paths: HashSet<String> = HashSet::new();
        let mut manifest_entries: Vec<iceberg::spec::ManifestFile> = Vec::new();

        for snapshot in &snapshots {
            let manifest_list_path = snapshot.manifest_list();

            let manifest_list_content = match file_io
                .new_input(manifest_list_path)
                .map_err(|e| Error::General(format!("Failed to open manifest list: {}", e)))?
                .read()
                .await
            {
                Ok(content) => content,
                Err(_) => continue,
            };

            let manifest_list = match ManifestList::parse_with_version(
                &manifest_list_content,
                metadata.format_version(),
            ) {
                Ok(ml) => ml,
                Err(_) => continue,
            };

            for entry in manifest_list.entries() {
                if !seen_manifest_paths.contains(&entry.manifest_path) {
                    seen_manifest_paths.insert(entry.manifest_path.clone());
                    manifest_entries.push(entry.clone());
                }
            }
        }

        let total_manifests = manifest_entries.len();

        // Step 2: Load all manifests in parallel with progress bar
        let pb = ProgressBar::new(total_manifests as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.cyan} Scanning manifests {bar:30.dim.white/dim} {pos}/{len}")
                .unwrap()
                .progress_chars("━━╺"),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        let manifest_results: Vec<Vec<String>> = stream::iter(manifest_entries.into_iter())
            .map(|manifest_entry| {
                let file_io = file_io.clone();
                let pb = pb.clone();
                async move {
                    let result = if let Ok(manifest) = manifest_entry.load_manifest(&file_io).await
                    {
                        manifest
                            .entries()
                            .iter()
                            .map(|e| e.file_path().to_string())
                            .collect()
                    } else {
                        Vec::new()
                    };
                    pb.inc(1);
                    result
                }
            })
            .buffer_unordered(32)
            .collect()
            .await;

        pb.finish_and_clear();

        // Collect all referenced files
        let mut referenced_files: HashSet<String> = HashSet::new();
        for paths in manifest_results {
            referenced_files.extend(paths);
        }

        // Calculate cutoff time
        let cutoff_time = chrono::Utc::now() - chrono::Duration::hours(args.retention_hours as i64);
        let cutoff_ms = cutoff_time.timestamp_millis();

        // List all files in data directory
        let storage = StorageBackendFactory::create_backend(table_path).await?;
        let base_path = table_path.trim_end_matches('/');
        let data_prefix = format!("{}/data/", base_path);

        let list_opts = ListOptions {
            prefix: Some(data_prefix),
            delimiter: None,
            max_results: None,
            continuation_token: None,
        };

        let all_files = storage.list(&list_opts).await?;

        // Build a HashSet of normalized file names for O(1) lookup
        let referenced_filenames: HashSet<String> = referenced_files
            .iter()
            .filter_map(|r| r.rsplit('/').next().map(|s| s.to_string()))
            .collect();

        let mut orphan_files: Vec<(String, u64)> = Vec::new();
        let mut orphan_bytes: u64 = 0;

        for obj in &all_files.objects {
            let filename = obj.path.rsplit('/').next().unwrap_or(&obj.path);
            let is_referenced = referenced_filenames.contains(filename);

            if !is_referenced {
                let file_time_ms = obj.last_modified.timestamp_millis();
                if file_time_ms < cutoff_ms {
                    orphan_files.push((obj.path.clone(), obj.size));
                    orphan_bytes += obj.size;
                }
            }
        }

        // Output analysis
        println!();
        println!(
            "Referenced files: {}",
            referenced_files.len().to_string().cyan()
        );
        println!(
            "Files to delete:   {} ({})",
            orphan_files.len().to_string().cyan(),
            format_bytes(orphan_bytes)
        );
        println!(
            "Retention:        {} hours",
            args.retention_hours.to_string().cyan()
        );

        if orphan_files.is_empty() {
            println!();
            println!("{}", "No orphan files to delete".yellow());
            return Ok(());
        }

        if args.dry_run {
            println!();
            println!("{}", "DRY RUN - No files will be deleted".yellow().bold());

            if args.output == "json" {
                let files: Vec<&str> = orphan_files.iter().map(|(p, _)| p.as_str()).collect();
                let json = serde_json::json!({
                    "dry_run": true,
                    "files_to_delete": files,
                    "files_count": orphan_files.len(),
                    "bytes_to_free": orphan_bytes,
                    "retention_hours": args.retention_hours,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json)
                        .map_err(|e| Error::General(e.to_string()))?
                );
            } else {
                println!();
                println!("{}", "Would delete the following files:".cyan());

                // Show first 10 files, then summary if more
                let show_count = 10;
                for (path, size) in orphan_files.iter().take(show_count) {
                    let name = path.rsplit('/').next().unwrap_or(path);
                    println!("  - {} ({})", name, format_bytes(*size));
                }

                if orphan_files.len() > show_count {
                    println!(
                        "  {} {} more files...",
                        "...and".dimmed(),
                        (orphan_files.len() - show_count).to_string().dimmed()
                    );
                }

                println!();
                println!(
                    "Total: {} files, {} to free",
                    orphan_files.len().to_string().yellow(),
                    format_bytes(orphan_bytes).yellow()
                );
                println!();
                println!(
                    "{}",
                    "Run without --dry-run to delete these files.".dimmed()
                );
            }
            return Ok(());
        }

        // Actually delete files with progress bar
        let pb = ProgressBar::new(orphan_files.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} Deleting {bar:30.cyan/blue} {pos}/{len} ({percent}%)")
                .unwrap()
                .progress_chars("━━╺"),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        let mut deleted_count = 0;
        let mut deleted_bytes = 0u64;
        let mut errors = Vec::new();

        for (path, size) in &orphan_files {
            match storage.delete(path).await {
                Ok(_) => {
                    deleted_count += 1;
                    deleted_bytes += size;
                }
                Err(e) => {
                    errors.push(format!("{}: {}", path, e));
                }
            }
            pb.inc(1);
        }

        pb.finish_and_clear();

        if args.output == "json" {
            let json = serde_json::json!({
                "files_deleted": deleted_count,
                "bytes_freed": deleted_bytes,
                "errors": errors.len(),
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!();
            println!(
                "{} {} files, freed {}",
                "Deleted".green().bold(),
                deleted_count,
                format_bytes(deleted_bytes)
            );
            if !errors.is_empty() {
                println!("{} errors occurred:", errors.len().to_string().red());
                for err in errors.iter().take(5) {
                    println!("  - {}", err);
                }
                if errors.len() > 5 {
                    println!("  ... and {} more", errors.len() - 5);
                }
            }
        }

        Ok(())
    }
}
