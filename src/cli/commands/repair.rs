//! Repair command implementation
//!
//! Repairs metadata inconsistencies in Delta Lake and Iceberg tables.

use std::collections::HashSet;

use colored::Colorize;

use crate::cli::parser::RepairArgs;
use crate::error::{Error, Result};

/// Handler for repair command
pub struct RepairCommand;

/// Repair action to take
#[derive(Debug, Clone)]
enum RepairAction {
    RemoveMissing { path: String },
    AddOrphan { path: String, size: u64 },
}

impl RepairCommand {
    /// Execute repair command
    pub async fn execute(args: RepairArgs) -> Result<()> {
        // Validate at least one repair option is specified
        if !args.sync_metadata && !args.remove_missing && !args.add_orphans {
            return Err(Error::General(
                "Must specify at least one repair option: --sync-metadata, --remove-missing, or --add-orphans".to_string(),
            ));
        }

        let path = std::path::Path::new(&args.path);

        // Detect table format
        let is_delta = path.join("_delta_log").exists();
        let is_iceberg = path.join("metadata").exists();

        if is_delta {
            Self::repair_delta(&args).await
        } else if is_iceberg {
            Self::repair_iceberg(&args).await
        } else {
            Err(Error::General(format!(
                "Path '{}' is not a Delta Lake or Iceberg table",
                args.path
            )))
        }
    }

    /// Repair Delta Lake table
    #[cfg(feature = "delta")]
    async fn repair_delta(args: &RepairArgs) -> Result<()> {
        use deltalake::kernel::Action;
        use deltalake::protocol::DeltaOperation;

        println!(
            "{} Delta table at {}",
            if args.dry_run { "Analyzing" } else { "Repairing" }.green(),
            args.path
        );

        // Open the table
        let table = deltalake::open_table(&args.path)
            .await
            .map_err(|e| Error::General(format!("Failed to open Delta table: {}", e)))?;

        // Get tracked files from metadata
        let tracked_files: HashSet<String> = table
            .get_file_uris()
            .map_err(|e| Error::General(format!("Failed to get files: {}", e)))?
            .map(|uri| {
                uri.strip_prefix("file://")
                    .unwrap_or(&uri)
                    .to_string()
            })
            .collect();

        // Get actual files on disk
        let table_path = std::path::Path::new(&args.path);
        let mut actual_files: HashSet<String> = HashSet::new();

        for entry in std::fs::read_dir(table_path)
            .map_err(|e| Error::General(format!("Failed to read directory: {}", e)))?
        {
            let entry = entry.map_err(|e| Error::General(format!("Read error: {}", e)))?;
            let path = entry.path();

            if path.is_file() && path.extension().map_or(false, |e| e == "parquet") {
                actual_files.insert(path.to_string_lossy().to_string());
            }
        }

        // Find issues
        let mut actions: Vec<RepairAction> = Vec::new();

        // Missing files (tracked but not on disk)
        if args.remove_missing || args.sync_metadata {
            for tracked in &tracked_files {
                if !actual_files.contains(tracked) {
                    actions.push(RepairAction::RemoveMissing {
                        path: tracked.clone(),
                    });
                }
            }
        }

        // Orphan files (on disk but not tracked)
        if args.add_orphans || args.sync_metadata {
            for actual in &actual_files {
                if !tracked_files.contains(actual) {
                    let size = std::fs::metadata(actual)
                        .map(|m| m.len())
                        .unwrap_or(0);
                    actions.push(RepairAction::AddOrphan {
                        path: actual.clone(),
                        size,
                    });
                }
            }
        }

        if actions.is_empty() {
            println!();
            println!("{}", "No issues found - table is healthy!".green());
            return Ok(());
        }

        // Report findings
        let missing_count = actions
            .iter()
            .filter(|a| matches!(a, RepairAction::RemoveMissing { .. }))
            .count();
        let orphan_count = actions
            .iter()
            .filter(|a| matches!(a, RepairAction::AddOrphan { .. }))
            .count();

        println!();
        println!("Issues found:");
        if missing_count > 0 {
            println!(
                "  Missing files:  {}",
                missing_count.to_string().red()
            );
        }
        if orphan_count > 0 {
            println!(
                "  Orphan files:   {}",
                orphan_count.to_string().yellow()
            );
        }

        if args.dry_run {
            println!();
            println!("{}", "DRY RUN - No changes made".yellow().bold());

            for action in &actions {
                match action {
                    RepairAction::RemoveMissing { path } => {
                        let name = path.rsplit('/').next().unwrap_or(path);
                        println!("  Would remove reference: {}", name.red());
                    }
                    RepairAction::AddOrphan { path, size } => {
                        let name = path.rsplit('/').next().unwrap_or(path);
                        println!(
                            "  Would add: {} ({})",
                            name.green(),
                            Self::format_bytes(*size)
                        );
                    }
                }
            }

            return Ok(());
        }

        // Apply repairs
        let mut delta_actions: Vec<Action> = Vec::new();

        for action in &actions {
            match action {
                RepairAction::RemoveMissing { path } => {
                    let rel_path = path
                        .strip_prefix(&args.path)
                        .unwrap_or(path)
                        .trim_start_matches('/');

                    delta_actions.push(Action::Remove(deltalake::kernel::Remove {
                        path: rel_path.to_string(),
                        deletion_timestamp: Some(chrono::Utc::now().timestamp_millis()),
                        data_change: false,
                        extended_file_metadata: None,
                        partition_values: None,
                        size: None,
                        deletion_vector: None,
                        base_row_id: None,
                        default_row_commit_version: None,
                        tags: None,
                    }));
                }
                RepairAction::AddOrphan { path, size } => {
                    let rel_path = path
                        .strip_prefix(&args.path)
                        .unwrap_or(path)
                        .trim_start_matches('/');

                    delta_actions.push(Action::Add(deltalake::kernel::Add {
                        path: rel_path.to_string(),
                        partition_values: std::collections::HashMap::new(),
                        size: *size as i64,
                        modification_time: chrono::Utc::now().timestamp_millis(),
                        data_change: false,
                        stats: None,
                        tags: None,
                        deletion_vector: None,
                        base_row_id: None,
                        default_row_commit_version: None,
                        clustering_provider: None,
                    }));
                }
            }
        }

        // Commit repairs
        let log_store = table.log_store();
        let snapshot = table
            .snapshot()
            .map_err(|e| Error::General(format!("Failed to get snapshot: {}", e)))?;

        deltalake::kernel::transaction::CommitBuilder::default()
            .with_actions(delta_actions)
            .build(
                Some(snapshot),
                log_store,
                DeltaOperation::FileSystemCheck {},
            )
            .await
            .map_err(|e| Error::General(format!("Failed to commit repair: {}", e)))?;

        println!();
        println!("{}", "Repair complete!".green().bold());
        println!(
            "Removed {} missing references, added {} orphan files",
            missing_count, orphan_count
        );

        Ok(())
    }

    #[cfg(not(feature = "delta"))]
    async fn repair_delta(_args: &RepairArgs) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Delta Lake support not enabled".to_string(),
        })
    }

    /// Repair Iceberg table
    ///
    /// Performs full repair including:
    /// 1. Fix version-hint.text if pointing to missing metadata
    /// 2. Create missing data directory
    /// 3. Remove references to missing files from manifest
    /// 4. Add orphan parquet files to manifest
    #[cfg(feature = "iceberg")]
    async fn repair_iceberg(args: &RepairArgs) -> Result<()> {
        use bytes::Bytes;
        use iceberg::io::FileIOBuilder;
        use iceberg::spec::{
            DataContentType, DataFile, DataFileBuilder, DataFileFormat, ManifestListWriter,
            ManifestStatus, ManifestWriterBuilder, Snapshot, Struct, Summary, TableMetadataBuilder,
        };
        use iceberg::table::StaticTable;
        use iceberg::TableIdent;
        use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
        use std::collections::HashMap;

        println!(
            "{} Iceberg table at {}",
            if args.dry_run { "Analyzing" } else { "Repairing" }.green(),
            args.path
        );

        let table_path = std::path::Path::new(&args.path);
        let metadata_dir = table_path.join("metadata");
        let data_dir = table_path.join("data");

        // Check and fix version-hint.text
        let version_hint = metadata_dir.join("version-hint.text");
        let mut current_version: i32 = std::fs::read_to_string(&version_hint)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);

        let metadata_file = metadata_dir.join(format!("v{}.metadata.json", current_version));

        // Fix version hint if needed
        if !metadata_file.exists() {
            println!(
                "{}",
                format!("Version hint points to missing v{}.metadata.json", current_version).yellow()
            );

            let mut max_version = 0;
            if let Ok(entries) = std::fs::read_dir(&metadata_dir) {
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

            if max_version > 0 {
                if !args.dry_run {
                    std::fs::write(&version_hint, max_version.to_string())
                        .map_err(|e| Error::General(format!("Failed to fix version hint: {}", e)))?;
                    println!("  Fixed version-hint.text → v{}", max_version);
                } else {
                    println!("  Would fix version-hint.text → v{}", max_version);
                }
                current_version = max_version;
            } else {
                return Err(Error::General("No valid metadata files found".to_string()));
            }
        }

        // Ensure data directory exists
        if !data_dir.exists() {
            if !args.dry_run {
                std::fs::create_dir_all(&data_dir)
                    .map_err(|e| Error::General(format!("Failed to create data dir: {}", e)))?;
                println!("Created missing data directory");
            } else {
                println!("Would create missing data directory");
            }
        }

        // Load table metadata
        let metadata_file = metadata_dir.join(format!("v{}.metadata.json", current_version));
        let file_io = FileIOBuilder::new_fs_io()
            .build()
            .map_err(|e| Error::General(format!("Failed to create FileIO: {}", e)))?;

        let table_ident = TableIdent::from_strs(&["iceberg", "table"])
            .map_err(|e| Error::General(format!("Failed to create table ident: {}", e)))?;

        let static_table = StaticTable::from_metadata_file(
            &metadata_file.to_string_lossy(),
            table_ident,
            file_io.clone(),
        )
        .await
        .map_err(|e| Error::General(format!("Failed to load table: {}", e)))?;

        let old_metadata = static_table.metadata().clone();
        let iceberg_schema = old_metadata.current_schema();
        let partition_spec = old_metadata.default_partition_spec();

        // Get tracked files from current snapshot
        let mut tracked_files: HashSet<String> = HashSet::new();
        let mut current_data_files: Vec<DataFile> = Vec::new();

        if let Some(current_snapshot) = old_metadata.current_snapshot() {
            let manifest_list_path = current_snapshot.manifest_list();
            let manifest_list_content = file_io
                .new_input(manifest_list_path)
                .map_err(|e| Error::General(format!("Failed to open manifest list: {}", e)))?
                .read()
                .await
                .map_err(|e| Error::General(format!("Failed to read manifest list: {}", e)))?;

            let manifest_list = iceberg::spec::ManifestList::parse_with_version(
                &manifest_list_content,
                iceberg::spec::FormatVersion::V2,
            )
            .map_err(|e| Error::General(format!("Failed to parse manifest list: {}", e)))?;

            for manifest_file_entry in manifest_list.entries() {
                let manifest = manifest_file_entry
                    .load_manifest(&file_io)
                    .await
                    .map_err(|e| Error::General(format!("Failed to load manifest: {}", e)))?;

                for entry in manifest.entries() {
                    if entry.status != ManifestStatus::Deleted {
                        let file_path = entry.data_file.file_path().to_string();
                        tracked_files.insert(file_path);
                        current_data_files.push(entry.data_file.clone());
                    }
                }
            }
        }

        println!("Tracked files in manifest: {}", tracked_files.len());

        // Get actual files on disk
        let mut actual_files: HashSet<String> = HashSet::new();
        if data_dir.exists() {
            for entry in std::fs::read_dir(&data_dir)
                .map_err(|e| Error::General(format!("Failed to read data dir: {}", e)))?
            {
                let entry = entry.map_err(|e| Error::General(format!("Read error: {}", e)))?;
                let path = entry.path();

                if path.is_file() && path.extension().map_or(false, |e| e == "parquet") {
                    actual_files.insert(path.to_string_lossy().to_string());
                }
            }
        }

        println!("Parquet files on disk: {}", actual_files.len());

        // Find issues
        let mut actions: Vec<RepairAction> = Vec::new();

        // Missing files (tracked but not on disk)
        if args.remove_missing || args.sync_metadata {
            for tracked in &tracked_files {
                if !actual_files.contains(tracked) {
                    actions.push(RepairAction::RemoveMissing {
                        path: tracked.clone(),
                    });
                }
            }
        }

        // Orphan files (on disk but not tracked)
        if args.add_orphans || args.sync_metadata {
            for actual in &actual_files {
                if !tracked_files.contains(actual) {
                    let size = std::fs::metadata(actual).map(|m| m.len()).unwrap_or(0);
                    actions.push(RepairAction::AddOrphan {
                        path: actual.clone(),
                        size,
                    });
                }
            }
        }

        if actions.is_empty() {
            println!();
            println!("{}", "No issues found - table is healthy!".green());
            return Ok(());
        }

        // Report findings
        let missing_count = actions
            .iter()
            .filter(|a| matches!(a, RepairAction::RemoveMissing { .. }))
            .count();
        let orphan_count = actions
            .iter()
            .filter(|a| matches!(a, RepairAction::AddOrphan { .. }))
            .count();

        println!();
        println!("Issues found:");
        if missing_count > 0 {
            println!("  Missing files:  {}", missing_count.to_string().red());
        }
        if orphan_count > 0 {
            println!("  Orphan files:   {}", orphan_count.to_string().yellow());
        }

        if args.dry_run {
            println!();
            println!("{}", "DRY RUN - No changes made".yellow().bold());

            for action in &actions {
                match action {
                    RepairAction::RemoveMissing { path } => {
                        let name = path.rsplit('/').next().unwrap_or(path);
                        println!("  Would remove reference: {}", name.red());
                    }
                    RepairAction::AddOrphan { path, size } => {
                        let name = path.rsplit('/').next().unwrap_or(path);
                        println!(
                            "  Would add: {} ({})",
                            name.green(),
                            Self::format_bytes(*size)
                        );
                    }
                }
            }

            return Ok(());
        }

        // Build new list of data files
        let mut new_data_files: Vec<DataFile> = Vec::new();

        // Keep existing files that still exist on disk
        for data_file in &current_data_files {
            let file_path = data_file.file_path();
            if actual_files.contains(file_path) {
                new_data_files.push(data_file.clone());
            }
        }

        // Add orphan files
        for action in &actions {
            if let RepairAction::AddOrphan { path, size } = action {
                // Read record count from parquet file
                let record_count = match std::fs::read(path) {
                    Ok(data) => {
                        match ParquetRecordBatchReaderBuilder::try_new(Bytes::from(data)) {
                            Ok(builder) => builder
                                .metadata()
                                .file_metadata()
                                .num_rows() as u64,
                            Err(_) => 0,
                        }
                    }
                    Err(_) => 0,
                };

                let data_file = DataFileBuilder::default()
                    .content(DataContentType::Data)
                    .file_path(path.clone())
                    .file_format(DataFileFormat::Parquet)
                    .partition(Struct::empty())
                    .partition_spec_id(partition_spec.spec_id())
                    .record_count(record_count)
                    .file_size_in_bytes(*size)
                    .build()
                    .map_err(|e| Error::General(format!("Failed to build DataFile: {}", e)))?;

                new_data_files.push(data_file);
            }
        }

        // Create new snapshot with repaired manifest
        let timestamp_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);

        let snapshot_id = chrono::Utc::now().timestamp_millis();
        let sequence_number = old_metadata
            .current_snapshot()
            .map(|s| s.sequence_number() + 1)
            .unwrap_or(1);
        let parent_snapshot_id = old_metadata.current_snapshot().map(|s| s.snapshot_id());

        // Write manifest
        let manifest_filename = format!("{:016x}-m0.avro", timestamp_nanos);
        let manifest_path = metadata_dir.join(&manifest_filename);

        let output_file = file_io
            .new_output(&manifest_path.to_string_lossy())
            .map_err(|e| Error::General(format!("Failed to create manifest output: {}", e)))?;

        let mut manifest_writer = ManifestWriterBuilder::new(
            output_file,
            Some(snapshot_id),
            None,
            iceberg_schema.clone(),
            (**partition_spec).clone(),
        )
        .build_v2_data();

        let mut total_records = 0u64;
        for data_file in &new_data_files {
            manifest_writer
                .add_file(data_file.clone(), sequence_number)
                .map_err(|e| Error::General(format!("Failed to add file to manifest: {}", e)))?;
            total_records += data_file.record_count();
        }

        let manifest_file = manifest_writer
            .write_manifest_file()
            .await
            .map_err(|e| Error::General(format!("Failed to write manifest: {}", e)))?;

        // Write manifest list
        let manifest_list_filename = format!("snap-{}-0-{:016x}.avro", snapshot_id, timestamp_nanos);
        let manifest_list_path = metadata_dir.join(&manifest_list_filename);

        let manifest_list_output = file_io
            .new_output(&manifest_list_path.to_string_lossy())
            .map_err(|e| Error::General(format!("Failed to create manifest list: {}", e)))?;

        let mut manifest_list_writer = ManifestListWriter::v2(
            manifest_list_output,
            snapshot_id,
            parent_snapshot_id,
            sequence_number,
        );

        manifest_list_writer
            .add_manifests(vec![manifest_file].into_iter())
            .map_err(|e| Error::General(format!("Failed to add manifest: {}", e)))?;

        manifest_list_writer
            .close()
            .await
            .map_err(|e| Error::General(format!("Failed to close manifest list: {}", e)))?;

        // Create snapshot
        let timestamp_ms = chrono::Utc::now().timestamp_millis();
        let summary = Summary {
            operation: iceberg::spec::Operation::Replace,
            additional_properties: HashMap::from([
                ("total-records".to_string(), total_records.to_string()),
                (
                    "total-data-files".to_string(),
                    new_data_files.len().to_string(),
                ),
                ("repair-removed".to_string(), missing_count.to_string()),
                ("repair-added".to_string(), orphan_count.to_string()),
            ]),
        };

        let snapshot = Snapshot::builder()
            .with_snapshot_id(snapshot_id)
            .with_parent_snapshot_id(parent_snapshot_id)
            .with_sequence_number(sequence_number)
            .with_timestamp_ms(timestamp_ms)
            .with_manifest_list(manifest_list_path.to_string_lossy().to_string())
            .with_summary(summary)
            .with_schema_id(iceberg_schema.schema_id())
            .build();

        // Write new metadata
        let old_metadata_owned: iceberg::spec::TableMetadata = (*old_metadata).clone();
        let metadata_log_path = format!("v{}.metadata.json", current_version);

        let new_metadata = TableMetadataBuilder::new_from_metadata(
            old_metadata_owned,
            Some(metadata_log_path),
        )
        .set_branch_snapshot(snapshot, iceberg::spec::MAIN_BRANCH)
        .map_err(|e| Error::General(format!("Failed to set snapshot: {}", e)))?
        .build()
        .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

        let new_version = current_version + 1;
        let new_metadata_file = metadata_dir.join(format!("v{}.metadata.json", new_version));

        let metadata_json = serde_json::to_string_pretty(&new_metadata.metadata)
            .map_err(|e| Error::General(format!("Failed to serialize: {}", e)))?;

        std::fs::write(&new_metadata_file, metadata_json)
            .map_err(|e| Error::General(format!("Failed to write metadata: {}", e)))?;

        std::fs::write(&version_hint, new_version.to_string())
            .map_err(|e| Error::General(format!("Failed to update version hint: {}", e)))?;

        println!();
        println!("{}", "Repair complete!".green().bold());
        println!(
            "Removed {} missing references, added {} orphan files",
            missing_count, orphan_count
        );
        println!("New metadata version: v{}", new_version);

        Ok(())
    }

    #[cfg(not(feature = "iceberg"))]
    async fn repair_iceberg(_args: &RepairArgs) -> Result<()> {
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
