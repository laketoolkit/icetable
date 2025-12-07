//! Optimize command implementation
//!
//! Subcommands:
//! - `data`: Compact small data files into larger ones
//! - `manifests`: Rewrite and compact manifest files

use colored::Colorize;

use super::common::{resolve_table, TableResolution};
use crate::cli::parser::{OptimizeCommands, OptimizeDataArgs, OptimizeManifestsArgs};
use crate::core::catalog::TableCommitter;
use crate::core::maintenance::{MaintenanceConfig, OptimizeService};
use crate::core::metadata::MaintenanceResult;
use crate::core::utils::parse_bytes;
use crate::core::{CatalogConfig, TableFormat, detect_table_format_async, format_bytes};
use crate::error::{Error, Result};

/// Handler for optimize command
pub struct OptimizeCommand;

impl OptimizeCommand {
    /// Execute optimize command
    pub async fn execute(cmd: OptimizeCommands, catalog_config: Option<CatalogConfig>) -> Result<()> {
        match cmd {
            OptimizeCommands::Data(args) => Self::execute_data(args, catalog_config).await,
            OptimizeCommands::Manifests(args) => Self::execute_manifests(args, catalog_config).await,
        }
    }

    /// Execute optimize data subcommand
    async fn execute_data(args: OptimizeDataArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        let resolution = resolve_table(&args.path, catalog_config.as_ref()).await?;
        let table_path = resolution.location().to_string();

        // Create committer if using catalog
        let committer = Self::create_committer(catalog_config.as_ref(), &resolution);

        // Detect table format (supports remote storage)
        let format = detect_table_format_async(&table_path).await;

        // Parse max_bytes if provided
        let max_bytes = args.max_bytes.as_ref()
            .map(|s| parse_bytes(s))
            .transpose()
            .map_err(Error::General)?;

        // Create service configuration
        let config = MaintenanceConfig {
            target_size: args.target_size,
            min_size: args.min_file_size.unwrap_or(args.target_size / 16),
            dry_run: args.dry_run,
            parallelism: args.max_concurrent_tasks,
            partition_filter: args.partition.clone(),
            max_files: args.max_files,
            max_bytes,
            ..Default::default()
        };

        let service = OptimizeService::with_config(config);

        let result = match format {
            TableFormat::Delta => Self::optimize_delta_data(&args, &service).await?,
            TableFormat::Iceberg => Self::optimize_iceberg_data(&table_path, &service, args.branch.as_deref(), committer).await?,
            TableFormat::Unknown => {
                return Err(Error::General(format!(
                    "Path '{}' is not a Delta Lake or Iceberg table",
                    table_path
                )));
            }
        };

        Self::output_data_result(&result, &args.output)?;
        Ok(())
    }

    /// Create a TableCommitter if catalog is configured
    fn create_committer(
        catalog_config: Option<&CatalogConfig>,
        resolution: &TableResolution,
    ) -> Option<TableCommitter> {
        match (catalog_config, resolution) {
            (Some(config), TableResolution::CatalogTable { namespace, name, .. }) => {
                Some(TableCommitter::with_catalog(
                    config.clone(),
                    namespace.clone(),
                    name.clone(),
                ))
            }
            _ => None,
        }
    }

    /// Execute optimize manifests subcommand
    async fn execute_manifests(args: OptimizeManifestsArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        let resolution = resolve_table(&args.path, catalog_config.as_ref()).await?;
        let table_path = resolution.location().to_string();

        // Create committer if using catalog
        let committer = Self::create_committer(catalog_config.as_ref(), &resolution);

        // Detect table format (supports remote storage)
        let format = detect_table_format_async(&table_path).await;

        match format {
            TableFormat::Delta => {
                println!(
                    "{}",
                    "Delta Lake does not use manifest files. Use 'optimize data' instead.".yellow()
                );
                Ok(())
            }
            TableFormat::Iceberg => Self::rewrite_iceberg_manifests(&table_path, &args, committer).await,
            TableFormat::Unknown => Err(Error::General(format!(
                "Path '{}' is not a Delta Lake or Iceberg table",
                table_path
            ))),
        }
    }

    /// Optimize Delta Lake data files - not supported, use Iceberg instead
    async fn optimize_delta_data(
        _args: &OptimizeDataArgs,
        _service: &OptimizeService,
    ) -> Result<MaintenanceResult> {
        Err(Error::UnsupportedFeature {
            feature: "Delta Lake optimize is not supported. Use 'icetable import delta' to convert to Iceberg.".to_string(),
        })
    }

    /// Optimize Iceberg data files
    async fn optimize_iceberg_data(
        table_path: &str,
        service: &OptimizeService,
        branch: Option<&str>,
        committer: Option<TableCommitter>,
    ) -> Result<MaintenanceResult> {
        use crate::core::metadata::IcebergMetadataService;

        if let Some(b) = branch {
            println!(
                "{} Iceberg table at {} (branch: {})",
                "Optimizing".green(),
                table_path,
                b.cyan()
            );
        } else {
            println!("{} Iceberg table at {}", "Optimizing".green(), table_path);
        }

        let metadata_service = match committer {
            Some(c) => IcebergMetadataService::new_with_committer(
                table_path.to_string(),
                branch.map(|s| s.to_string()),
                c,
            ).await?,
            None => IcebergMetadataService::new_with_branch(
                table_path.to_string(),
                branch.map(|s| s.to_string()),
            ).await?,
        };
        service.execute(&metadata_service).await
    }

    /// Rewrite Iceberg manifest files
    async fn rewrite_iceberg_manifests(
        table_path: &str,
        args: &OptimizeManifestsArgs,
        committer: Option<TableCommitter>,
    ) -> Result<()> {
        use crate::core::metadata::IcebergMetadataService;
        use iceberg::spec::{
            ManifestContentType, ManifestEntry, ManifestListWriter, ManifestStatus,
            ManifestWriterBuilder,
        };
        use indicatif::{ProgressBar, ProgressStyle};
        use std::sync::Arc;

        // Determine target branch
        let target_branch = args.branch.as_deref().unwrap_or("main");

        if args.branch.is_some() {
            println!(
                "{} Iceberg manifests at {} (branch: {})",
                "Rewriting".green(),
                table_path,
                target_branch.cyan()
            );
        } else {
            println!(
                "{} Iceberg manifests at {}",
                "Rewriting".green(),
                table_path
            );
        }

        // Load metadata
        let service = IcebergMetadataService::new_with_branch(
            table_path.to_string(),
            args.branch.clone(),
        ).await?;
        let (metadata, _current_version) = service.load_metadata().await?;
        let file_io = service.file_io().clone();

        // Get snapshot for the target branch
        let current_snapshot = if target_branch == "main" {
            metadata
                .current_snapshot()
                .ok_or_else(|| Error::General("No current snapshot found".to_string()))?
        } else {
            metadata
                .snapshot_for_ref(target_branch)
                .ok_or_else(|| Error::General(format!("Branch '{}' not found", target_branch)))?
        };
        let snapshot_id = current_snapshot.snapshot_id();
        let parent_snapshot_id = current_snapshot.parent_snapshot_id();
        let sequence_number = current_snapshot.sequence_number();

        // Load manifest list for current snapshot
        let manifest_list = current_snapshot
            .load_manifest_list(&file_io, &metadata)
            .await
            .map_err(|e| Error::General(format!("Failed to load manifest list: {}", e)))?;

        let manifest_entries = manifest_list.entries();
        let total_manifests = manifest_entries.len();

        if total_manifests < args.min_manifests {
            println!();
            println!(
                "{} Only {} manifests found (minimum: {})",
                "Skipping:".yellow(),
                total_manifests,
                args.min_manifests
            );
            return Ok(());
        }

        println!("Current manifests: {}", total_manifests.to_string().cyan());

        // Group manifests by content type (data vs delete)
        let mut data_manifests = Vec::new();
        let mut delete_manifests = Vec::new();

        for entry in manifest_entries {
            match entry.content {
                ManifestContentType::Data => data_manifests.push(entry.clone()),
                ManifestContentType::Deletes => delete_manifests.push(entry.clone()),
            }
        }

        println!(
            "  Data manifests:   {}",
            data_manifests.len().to_string().cyan()
        );
        println!(
            "  Delete manifests: {}",
            delete_manifests.len().to_string().cyan()
        );

        // Calculate entries per manifest
        let entries_per_manifest = (args.target_size as usize / 500).max(100);

        if args.dry_run {
            // For dry run, we need to count entries
            let pb = ProgressBar::new(data_manifests.len() as u64);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.cyan} Counting entries {bar:30.dim.white/dim} {pos}/{len}")
                    .unwrap()
                    .progress_chars("━━╺"),
            );
            pb.enable_steady_tick(std::time::Duration::from_millis(100));

            let mut total_entries = 0usize;
            for manifest_entry in &data_manifests {
                if let Ok(manifest) = manifest_entry.load_manifest(&file_io).await {
                    total_entries += manifest
                        .entries()
                        .iter()
                        .filter(|e| e.status() != ManifestStatus::Deleted)
                        .count();
                }
                pb.inc(1);
            }
            pb.finish_and_clear();

            let new_manifest_count = (total_entries / entries_per_manifest).max(1);

            println!();
            println!("{}", "DRY RUN - No changes made".yellow().bold());
            println!("Total data entries: {}", total_entries.to_string().cyan());
            println!(
                "Would rewrite into: {} manifests",
                new_manifest_count.to_string().cyan()
            );

            if args.output == "json" {
                let json = serde_json::json!({
                    "dry_run": true,
                    "current_manifests": total_manifests,
                    "data_manifests": data_manifests.len(),
                    "delete_manifests": delete_manifests.len(),
                    "total_entries": total_entries,
                    "estimated_after": new_manifest_count + delete_manifests.len(),
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json)
                        .map_err(|e| Error::General(e.to_string()))?
                );
            }
            return Ok(());
        }

        // Progress bar for reading manifests
        let pb = ProgressBar::new(data_manifests.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.cyan} Reading manifests {bar:30.dim.white/dim} {pos}/{len}")
                .unwrap()
                .progress_chars("━━╺"),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        // Read all data file entries from existing manifests
        let mut all_data_entries: Vec<Arc<ManifestEntry>> = Vec::new();

        for manifest_entry in &data_manifests {
            match manifest_entry.load_manifest(&file_io).await {
                Ok(manifest) => {
                    for entry in manifest.entries() {
                        // Only include existing/added files, not deleted
                        if entry.status() != ManifestStatus::Deleted {
                            all_data_entries.push(entry.clone());
                        }
                    }
                }
                Err(e) => {
                    pb.println(format!("Warning: Failed to load manifest: {}", e));
                }
            }
            pb.inc(1);
        }

        pb.finish_and_clear();

        let total_entries = all_data_entries.len();
        println!(
            "Total data file entries: {}",
            total_entries.to_string().cyan()
        );

        // Calculate how many new manifests we need
        let new_manifest_count = (total_entries / entries_per_manifest).max(1);
        println!(
            "Rewriting into {} manifests (~{} entries each)",
            new_manifest_count.to_string().cyan(),
            entries_per_manifest
        );

        // Get schema and partition spec for writing
        let schema = metadata.current_schema().clone();
        let partition_spec = metadata.default_partition_spec().clone();

        // Create output paths
        let base_path = table_path.trim_end_matches('/');
        let metadata_dir = format!("{}/metadata", base_path);
        let new_snapshot_id = chrono::Utc::now().timestamp_millis();

        // Progress bar for writing manifests
        let pb = ProgressBar::new(new_manifest_count as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.cyan} Writing manifests {bar:30.dim.white/dim} {pos}/{len}")
                .unwrap()
                .progress_chars("━━╺"),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        // Write new manifests
        let mut new_manifest_files = Vec::new();
        let chunks: Vec<_> = all_data_entries.chunks(entries_per_manifest).collect();

        for (idx, chunk) in chunks.iter().enumerate() {
            let manifest_filename = format!(
                "{:x}-m{}.avro",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64
                    ^ (idx as u64),
                idx
            );
            let manifest_path = format!("{}/{}", metadata_dir, manifest_filename);

            // Create output file
            let output = file_io
                .new_output(&manifest_path)
                .map_err(|e| Error::General(format!("Failed to create manifest output: {}", e)))?;

            // Build manifest writer
            let mut writer = ManifestWriterBuilder::new(
                output,
                Some(new_snapshot_id),
                None, // key_metadata
                schema.clone(),
                (*partition_spec).clone(),
            )
            .build_v2_data();

            // Add entries as existing files
            for entry in chunk.iter() {
                writer
                    .add_existing_file(
                        entry.data_file().clone(),
                        entry.snapshot_id().unwrap_or(snapshot_id),
                        entry.sequence_number().unwrap_or(sequence_number),
                        Some(entry.sequence_number().unwrap_or(sequence_number)),
                    )
                    .map_err(|e| Error::General(format!("Failed to add entry: {}", e)))?;
            }

            // Write manifest file
            let manifest_file = writer
                .write_manifest_file()
                .await
                .map_err(|e| Error::General(format!("Failed to write manifest: {}", e)))?;

            new_manifest_files.push(manifest_file);
            pb.inc(1);
        }

        pb.finish_and_clear();

        // Keep delete manifests as-is
        for delete_manifest in &delete_manifests {
            new_manifest_files.push(delete_manifest.clone());
        }

        let final_manifest_count = new_manifest_files.len();
        println!(
            "New manifest files: {}",
            final_manifest_count.to_string().cyan()
        );

        // Write new manifest list
        let manifest_list_filename = format!(
            "snap-{}-0-{:x}.avro",
            new_snapshot_id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64
        );
        let manifest_list_path = format!("{}/{}", metadata_dir, manifest_list_filename);

        let manifest_list_output = file_io
            .new_output(&manifest_list_path)
            .map_err(|e| Error::General(format!("Failed to create manifest list output: {}", e)))?;

        let mut manifest_list_writer = ManifestListWriter::v2(
            manifest_list_output,
            new_snapshot_id,
            parent_snapshot_id,
            sequence_number + 1,
        );

        // Use iceberg-rs ManifestListWriter directly
        manifest_list_writer
            .add_manifests(new_manifest_files.clone().into_iter())
            .map_err(|e| Error::General(format!("Failed to add manifests: {}", e)))?;
        manifest_list_writer
            .close()
            .await
            .map_err(|e| Error::General(format!("Failed to close manifest list writer: {}", e)))?;

        // Create new snapshot pointing to the new manifest list
        use iceberg::spec::{Operation, Snapshot, Summary};
        use std::collections::HashMap;

        // Calculate total statistics for the summary
        // Sum up existing files/rows from all manifests
        let total_data_files: u64 = new_manifest_files
            .iter()
            .filter(|m| m.content == iceberg::spec::ManifestContentType::Data)
            .map(|m| {
                m.added_files_count.unwrap_or(0) as u64 + m.existing_files_count.unwrap_or(0) as u64
            })
            .sum();
        let total_rows: u64 = new_manifest_files
            .iter()
            .filter(|m| m.content == iceberg::spec::ManifestContentType::Data)
            .map(|m| m.added_rows_count.unwrap_or(0) + m.existing_rows_count.unwrap_or(0))
            .sum();

        // Get total file size from original snapshot if available
        let original_summary = current_snapshot.summary();
        let total_files_size: u64 = original_summary
            .additional_properties
            .get("total-files-size")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        // Build summary for the new snapshot
        // Note: don't add "operation" to additional_properties - it comes from the `operation` field
        let mut summary_map: HashMap<String, String> = HashMap::new();
        summary_map.insert("spark.app.id".to_string(), "icetable".to_string());
        summary_map.insert(
            "manifests-rewritten".to_string(),
            data_manifests.len().to_string(),
        );
        summary_map.insert(
            "manifests-created".to_string(),
            final_manifest_count.to_string(),
        );

        // Required statistics for inspect and tools to work
        summary_map.insert("total-data-files".to_string(), total_data_files.to_string());
        summary_map.insert("total-records".to_string(), total_rows.to_string());
        summary_map.insert("total-files-size".to_string(), total_files_size.to_string());

        // Added/deleted files (we didn't add or delete any data files, just rewrote manifests)
        summary_map.insert("added-data-files".to_string(), "0".to_string());
        summary_map.insert("deleted-data-files".to_string(), "0".to_string());
        summary_map.insert("added-records".to_string(), "0".to_string());
        summary_map.insert("deleted-records".to_string(), "0".to_string());

        let summary = Summary {
            operation: Operation::Replace,
            additional_properties: summary_map,
        };

        // Build the new snapshot
        let new_snapshot = Snapshot::builder()
            .with_snapshot_id(new_snapshot_id)
            .with_parent_snapshot_id(Some(snapshot_id))
            .with_sequence_number(sequence_number + 1)
            .with_timestamp_ms(new_snapshot_id) // timestamp is the snapshot_id
            .with_manifest_list(manifest_list_path.clone())
            .with_summary(summary)
            .with_schema_id(metadata.current_schema_id())
            .build();

        // Commit the snapshot - either via catalog or direct write
        let metadata_file_path = service.current_metadata_path().await?;

        let new_version = if let Some(ref c) = committer {
            if c.uses_catalog() {
                // Catalog mode: commit via REST API
                c.commit_add_snapshot(&metadata, new_snapshot, target_branch)
                    .await?
                    .unwrap_or(1) as u32
            } else {
                // Direct mode: build metadata and write to storage
                let metadata_clone = (*metadata).clone();
                let build_result = metadata_clone
                    .into_builder(Some(metadata_file_path.clone()))
                    .add_snapshot(new_snapshot)
                    .map_err(|e| Error::General(format!("Failed to add snapshot: {}", e)))?
                    .set_ref(
                        target_branch,
                        iceberg::spec::SnapshotReference {
                            snapshot_id: new_snapshot_id,
                            retention: iceberg::spec::SnapshotRetention::Branch {
                                min_snapshots_to_keep: None,
                                max_snapshot_age_ms: None,
                                max_ref_age_ms: None,
                            },
                        },
                    )
                    .map_err(|e| Error::General(format!("Failed to set ref: {}", e)))?
                    .build()
                    .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

                let new_metadata = build_result.metadata;
                Self::write_metadata_direct(table_path, &metadata_dir, &metadata_file_path, &new_metadata).await?
            }
        } else {
            // No committer: build metadata and write to storage
            let metadata_clone = (*metadata).clone();
            let build_result = metadata_clone
                .into_builder(Some(metadata_file_path.clone()))
                .add_snapshot(new_snapshot)
                .map_err(|e| Error::General(format!("Failed to add snapshot: {}", e)))?
                .set_ref(
                    target_branch,
                    iceberg::spec::SnapshotReference {
                        snapshot_id: new_snapshot_id,
                        retention: iceberg::spec::SnapshotRetention::Branch {
                            min_snapshots_to_keep: None,
                            max_snapshot_age_ms: None,
                            max_ref_age_ms: None,
                        },
                    },
                )
                .map_err(|e| Error::General(format!("Failed to set ref: {}", e)))?
                .build()
                .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

            let new_metadata = build_result.metadata;
            Self::write_metadata_direct(table_path, &metadata_dir, &metadata_file_path, &new_metadata).await?
        };

        println!();
        println!("{}", "Manifests rewritten successfully!".green().bold());
        println!(
            "Manifests: {} -> {}",
            total_manifests.to_string().cyan(),
            final_manifest_count.to_string().cyan()
        );
        println!("Snapshot:  {}", new_snapshot_id.to_string().cyan());
        println!("Version:   {}", new_version.to_string().cyan());

        if args.output == "json" {
            let json = serde_json::json!({
                "previous_manifests": total_manifests,
                "new_manifests": final_manifest_count,
                "data_manifests_rewritten": data_manifests.len(),
                "delete_manifests_kept": delete_manifests.len(),
                "total_entries": total_entries,
                "snapshot_id": new_snapshot_id,
                "metadata_version": new_version,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        }

        Ok(())
    }

    /// Output result in the requested format
    fn output_data_result(result: &MaintenanceResult, output_format: &str) -> Result<()> {
        let is_dry_run = result.operation.contains("dry-run")
            || result.details.get("mode").map(|m| m == "dry-run").unwrap_or(false);

        match output_format {
            "json" => {
                let json = serde_json::json!({
                    "dry_run": is_dry_run,
                    "operation": result.operation,
                    "files_added": result.files_added,
                    "files_removed": result.files_removed,
                    "bytes_added": result.bytes_added,
                    "bytes_removed": result.bytes_removed,
                    "records_affected": result.records_affected,
                    "details": result.details,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json)
                        .map_err(|e| Error::General(format!("Failed to serialize: {}", e)))?
                );
            }
            _ => {
                println!();

                // Check if dry-run
                if is_dry_run {
                    println!("{}", "DRY RUN - No changes made".yellow().bold());
                    println!();

                    if result.files_added == 0 && result.files_removed == 0 {
                        println!("{}", "Table is already optimized.".green());
                        if let Some(reason) = result.details.get("reason") {
                            println!("{}", reason);
                        }
                    } else {
                        println!("{}", "Would perform the following changes:".cyan());
                        println!();
                        println!(
                            "  Files to compact:  {} -> {}",
                            result.files_removed.to_string().yellow(),
                            result.files_added.to_string().yellow()
                        );

                        if let Some(partitions) = result.details.get("partitions") {
                            println!(
                                "  Partitions:        {}",
                                partitions.yellow()
                            );
                        }

                        if let Some(would_compact) = result.details.get("would_compact") {
                            println!(
                                "  Summary:           {}",
                                would_compact.yellow()
                            );
                        }

                        println!();
                        println!(
                            "{}",
                            "Run without --dry-run to apply these changes.".dimmed()
                        );
                    }
                } else if result.files_added == 0 && result.files_removed == 0 {
                    println!("{}", "Table is already optimized.".green());
                    if let Some(reason) = result.details.get("reason") {
                        println!("{}", reason);
                    }
                } else {
                    println!("{}", "Compaction complete!".green().bold());
                    println!();
                    println!(
                        "Files compacted:  {} -> {}",
                        result.files_removed.to_string().cyan(),
                        result.files_added.to_string().cyan()
                    );
                    println!(
                        "Bytes saved:      {}",
                        format_bytes(result.bytes_removed.saturating_sub(result.bytes_added))
                    );
                    println!(
                        "Records affected: {}",
                        result.records_affected.to_string().cyan()
                    );

                    if let Some(snapshot_id) = result.details.get("snapshot_id") {
                        println!("Snapshot:         {}", snapshot_id.cyan());
                    }
                }
            }
        }
        Ok(())
    }

    /// Write metadata directly to storage (used when no catalog is configured)
    async fn write_metadata_direct(
        table_path: &str,
        metadata_dir: &str,
        metadata_file_path: &str,
        new_metadata: &iceberg::spec::TableMetadata,
    ) -> Result<u32> {
        use crate::core::utils::{extract_version_from_path, metadata_location_filename, new_metadata_location, next_metadata_location};
        use crate::core::storage::{StorageBackendFactory, PutOptions};

        let storage = StorageBackendFactory::create_backend(table_path).await?;

        // Generate next metadata location from current
        let next_location = next_metadata_location(metadata_file_path)
            .unwrap_or_else(|_| new_metadata_location(table_path));

        let new_version = extract_version_from_path(&next_location.to_string()).unwrap_or(0) as u32;
        let new_metadata_path = format!("{}/{}", metadata_dir, metadata_location_filename(&next_location));

        let new_metadata_bytes = serde_json::to_vec_pretty(new_metadata)
            .map_err(|e| Error::General(format!("Failed to serialize metadata: {}", e)))?;

        storage
            .put(
                &new_metadata_path,
                bytes::Bytes::from(new_metadata_bytes),
                &PutOptions::default(),
            )
            .await?;

        Ok(new_version)
    }
}
