//! Optimize command implementation
//!
//! Compacts small files into larger ones for Delta Lake and Iceberg tables.
//! Uses Arrow for reading/writing without requiring DataFusion.

use std::sync::Arc;

use arrow::compute::concat_batches;
use arrow::record_batch::RecordBatch;
use colored::Colorize;

use crate::cli::parser::OptimizeArgs;
use crate::error::{Error, Result};

/// Handler for optimize command
pub struct OptimizeCommand;

/// Information about a file to compact
#[derive(Debug, Clone)]
struct FileInfo {
    path: String,
    size: u64,
}

/// Compaction metrics
#[derive(Debug, Default)]
struct CompactionMetrics {
    files_read: usize,
    files_written: usize,
    bytes_read: u64,
    bytes_written: u64,
    batches_processed: usize,
}

impl OptimizeCommand {
    /// Execute optimize command
    pub async fn execute(args: OptimizeArgs) -> Result<()> {
        let path = std::path::Path::new(&args.path);

        // Detect table format
        let is_delta = path.join("_delta_log").exists();
        let is_iceberg = path.join("metadata").exists();

        if is_delta {
            Self::optimize_delta(&args).await
        } else if is_iceberg {
            Self::optimize_iceberg(&args).await
        } else {
            Err(Error::General(format!(
                "Path '{}' is not a Delta Lake or Iceberg table",
                args.path
            )))
        }
    }

    /// Optimize Delta Lake table with real compaction
    #[cfg(feature = "delta")]
    async fn optimize_delta(args: &OptimizeArgs) -> Result<()> {
        use bytes::Bytes;
        use deltalake::kernel::Action;
        use deltalake::protocol::{DeltaOperation, SaveMode};
        use deltalake::writer::{DeltaWriter, RecordBatchWriter};
        use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

        println!("{} Delta table at {}", "Optimizing".green(), args.path);

        // Open the table
        let table = deltalake::open_table(&args.path)
            .await
            .map_err(|e| Error::General(format!("Failed to open Delta table: {}", e)))?;

        // Get file list with sizes
        let file_uris: Vec<String> = table
            .get_file_uris()
            .map_err(|e| Error::General(format!("Failed to get files: {}", e)))?
            .collect();

        let mut files: Vec<FileInfo> = Vec::new();
        for uri in &file_uris {
            let path = Self::uri_to_path(uri);
            if let Ok(metadata) = std::fs::metadata(&path) {
                files.push(FileInfo {
                    path: uri.clone(),
                    size: metadata.len(),
                });
            }
        }

        // Find small files to compact
        let small_files: Vec<&FileInfo> = files
            .iter()
            .filter(|f| f.size < args.target_size)
            .collect();

        if small_files.len() < 2 {
            println!();
            println!("{}", "Table is already optimized.".green());
            println!("No small files to compact (need at least 2 files < target size)");
            return Ok(());
        }

        println!("Found {} small files to compact", small_files.len());

        // Read all small files into batches
        let mut all_batches: Vec<RecordBatch> = Vec::new();
        let mut metrics = CompactionMetrics::default();
        let mut schema = None;

        for file_info in &small_files {
            let path = Self::uri_to_path(&file_info.path);
            let data = std::fs::read(&path)
                .map_err(|e| Error::General(format!("Failed to read {}: {}", path, e)))?;

            metrics.files_read += 1;
            metrics.bytes_read += file_info.size;

            let reader = ParquetRecordBatchReaderBuilder::try_new(Bytes::from(data))
                .map_err(|e| Error::General(format!("Failed to open parquet: {}", e)))?
                .build()
                .map_err(|e| Error::General(format!("Failed to build reader: {}", e)))?;

            for batch_result in reader {
                let batch = batch_result
                    .map_err(|e| Error::General(format!("Failed to read batch: {}", e)))?;

                if schema.is_none() {
                    schema = Some(batch.schema());
                }

                metrics.batches_processed += 1;
                all_batches.push(batch);
            }
        }

        if all_batches.is_empty() {
            println!("No data to compact");
            return Ok(());
        }

        let schema = schema.unwrap();
        println!(
            "Read {} batches from {} files",
            metrics.batches_processed, metrics.files_read
        );

        // Merge all batches into larger ones targeting args.target_size
        let merged_batches = Self::merge_batches_to_target_size(&all_batches, &schema, args.target_size)?;

        println!("Writing {} compacted files...", merged_batches.len());

        // Write new compacted files
        let table_path = args.path.clone();
        let mut writer = RecordBatchWriter::try_new(
            &table_path,
            schema.clone(),
            None,
            None,
        )
        .map_err(|e| Error::General(format!("Failed to create writer: {}", e)))?;

        for batch in &merged_batches {
            writer
                .write(batch.clone())
                .await
                .map_err(|e| Error::General(format!("Failed to write batch: {}", e)))?;
        }

        let adds = writer
            .flush()
            .await
            .map_err(|e| Error::General(format!("Failed to flush: {}", e)))?;

        metrics.files_written = adds.len();

        // Calculate bytes written
        for add in &adds {
            metrics.bytes_written += add.size as u64;
        }

        // Build remove actions for old files
        let removes: Vec<Action> = small_files
            .iter()
            .filter_map(|f| {
                // Extract relative path from URI
                let rel_path = f.path
                    .strip_prefix("file://")
                    .unwrap_or(&f.path)
                    .strip_prefix(&args.path)
                    .unwrap_or(&f.path)
                    .trim_start_matches('/');

                Some(Action::Remove(deltalake::kernel::Remove {
                    path: rel_path.to_string(),
                    deletion_timestamp: Some(chrono::Utc::now().timestamp_millis()),
                    data_change: false,
                    extended_file_metadata: None,
                    partition_values: None,
                    size: Some(f.size as i64),
                    deletion_vector: None,
                    base_row_id: None,
                    default_row_commit_version: None,
                    tags: None,
                }))
            })
            .collect();

        // Combine add and remove actions
        let mut actions: Vec<Action> = adds.into_iter().map(Action::Add).collect();
        actions.extend(removes);

        // Commit transaction
        let log_store = table.log_store();
        let snapshot = table
            .snapshot()
            .map_err(|e| Error::General(format!("Failed to get snapshot: {}", e)))?;

        deltalake::kernel::transaction::CommitBuilder::default()
            .with_actions(actions)
            .build(
                Some(snapshot),
                log_store,
                DeltaOperation::Optimize {
                    predicate: None,
                    target_size: args.target_size as i64,
                },
            )
            .await
            .map_err(|e| Error::General(format!("Failed to commit: {}", e)))?;

        // Output results
        Self::output_metrics(&metrics, &args.output)?;

        Ok(())
    }

    #[cfg(not(feature = "delta"))]
    async fn optimize_delta(_args: &OptimizeArgs) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Delta Lake support not enabled".to_string(),
        })
    }

    /// Optimize Iceberg table with real compaction
    ///
    /// This performs a proper Iceberg compaction:
    /// 1. Reads data files from current snapshot's manifests
    /// 2. Identifies small files to compact
    /// 3. Writes compacted files
    /// 4. Creates new manifest with both retained large files and new compacted files
    /// 5. Commits new snapshot with proper lineage
    #[cfg(feature = "iceberg")]
    async fn optimize_iceberg(args: &OptimizeArgs) -> Result<()> {
        use bytes::Bytes;
        use iceberg::io::FileIOBuilder;
        use iceberg::spec::{
            DataContentType, DataFile, DataFileBuilder, DataFileFormat, ManifestListWriter,
            ManifestStatus, ManifestWriterBuilder, Snapshot, Struct, Summary, TableMetadataBuilder,
        };
        use iceberg::table::StaticTable;
        use iceberg::TableIdent;
        use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
        use parquet::arrow::ArrowWriter;
        use parquet::file::properties::WriterProperties;
        use std::collections::HashMap;
        use std::fs::File;

        println!("{} Iceberg table at {}", "Optimizing".green(), args.path);

        let table_path = std::path::Path::new(&args.path);
        let data_dir = table_path.join("data");
        let metadata_dir = table_path.join("metadata");

        // Load table metadata
        let version_hint = metadata_dir.join("version-hint.text");
        let current_version: i32 = std::fs::read_to_string(&version_hint)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1);

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

        // Get current snapshot
        let current_snapshot = match old_metadata.current_snapshot() {
            Some(s) => s,
            None => {
                println!("No snapshot found - table is empty");
                return Ok(());
            }
        };

        // Read all data files from current snapshot's manifests
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

        // Collect all current data files
        let mut all_data_files: Vec<(DataFile, u64)> = Vec::new(); // (file, size)

        for manifest_file_entry in manifest_list.entries() {
            // Load manifest using the API method
            let manifest = manifest_file_entry
                .load_manifest(&file_io)
                .await
                .map_err(|e| Error::General(format!("Failed to load manifest: {}", e)))?;

            for entry in manifest.entries() {
                // Only consider ADDED or EXISTING entries (not DELETED)
                if entry.status != ManifestStatus::Deleted {
                    let data_file = entry.data_file.clone();
                    let size = data_file.file_size_in_bytes();
                    all_data_files.push((data_file, size as u64));
                }
            }
        }

        if all_data_files.is_empty() {
            println!("No data files found in current snapshot");
            return Ok(());
        }

        println!(
            "Found {} data files in current snapshot",
            all_data_files.len()
        );

        // Separate into small files (to compact) and large files (to keep)
        let mut small_files: Vec<&DataFile> = Vec::new();
        let mut large_files: Vec<&DataFile> = Vec::new();

        for (data_file, size) in &all_data_files {
            if *size < args.target_size {
                small_files.push(data_file);
            } else {
                large_files.push(data_file);
            }
        }

        if small_files.len() < 2 {
            println!();
            println!("{}", "Table is already optimized.".green());
            println!(
                "Found {} large files, {} small files (need at least 2 to compact)",
                large_files.len(),
                small_files.len()
            );
            return Ok(());
        }

        println!(
            "Compacting {} small files, keeping {} large files",
            small_files.len(),
            large_files.len()
        );

        // Read all small files into batches
        let mut all_batches: Vec<RecordBatch> = Vec::new();
        let mut metrics = CompactionMetrics::default();
        let mut arrow_schema = None;

        for data_file in &small_files {
            let file_path = data_file.file_path();

            let data = std::fs::read(file_path)
                .map_err(|e| Error::General(format!("Failed to read {}: {}", file_path, e)))?;

            metrics.files_read += 1;
            metrics.bytes_read += data_file.file_size_in_bytes() as u64;

            let reader = ParquetRecordBatchReaderBuilder::try_new(Bytes::from(data))
                .map_err(|e| Error::General(format!("Failed to open parquet: {}", e)))?
                .build()
                .map_err(|e| Error::General(format!("Failed to build reader: {}", e)))?;

            for batch_result in reader {
                let batch = batch_result
                    .map_err(|e| Error::General(format!("Failed to read batch: {}", e)))?;

                if arrow_schema.is_none() {
                    arrow_schema = Some(batch.schema());
                }
                metrics.batches_processed += 1;
                all_batches.push(batch);
            }
        }

        if all_batches.is_empty() {
            println!("No data to compact");
            return Ok(());
        }

        let arrow_schema = arrow_schema.unwrap();
        println!(
            "Read {} batches from {} files",
            metrics.batches_processed, metrics.files_read
        );

        // Merge batches into target-sized chunks
        let merged_batches =
            Self::merge_batches_to_target_size(&all_batches, &arrow_schema, args.target_size)?;

        println!("Writing {} compacted files...", merged_batches.len());

        // Ensure data directory exists
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir)
                .map_err(|e| Error::General(format!("Failed to create data dir: {}", e)))?;
        }

        // Write compacted files
        let timestamp_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);

        let mut new_data_files: Vec<DataFile> = Vec::new();
        let mut total_new_records = 0u64;

        for (idx, batch) in merged_batches.iter().enumerate() {
            let file_id = format!("{:016x}-{}", timestamp_nanos, idx);
            let parquet_filename = format!("compacted-{}.parquet", file_id);
            let parquet_path = data_dir.join(&parquet_filename);

            // Add field IDs to schema for Iceberg compatibility
            let fields_with_ids: Vec<arrow::datatypes::Field> = arrow_schema
                .fields()
                .iter()
                .enumerate()
                .map(|(i, field)| {
                    let field_id = iceberg_schema
                        .as_struct()
                        .fields()
                        .iter()
                        .find(|f| f.name == *field.name())
                        .map(|f| f.id)
                        .unwrap_or((i + 1) as i32);

                    let mut metadata = field.metadata().clone();
                    metadata.insert("PARQUET:field_id".to_string(), field_id.to_string());
                    field.as_ref().clone().with_metadata(metadata)
                })
                .collect();

            let write_schema = Arc::new(arrow::datatypes::Schema::new(fields_with_ids));

            let file = File::create(&parquet_path)
                .map_err(|e| Error::General(format!("Failed to create file: {}", e)))?;
            let props = WriterProperties::builder().build();
            let mut writer = ArrowWriter::try_new(file, write_schema, Some(props))
                .map_err(|e| Error::General(format!("Failed to create writer: {}", e)))?;

            writer
                .write(batch)
                .map_err(|e| Error::General(format!("Failed to write: {}", e)))?;
            writer
                .close()
                .map_err(|e| Error::General(format!("Failed to close: {}", e)))?;

            let file_size = std::fs::metadata(&parquet_path)
                .map_err(|e| Error::General(format!("Failed to get size: {}", e)))?
                .len();

            metrics.files_written += 1;
            metrics.bytes_written += file_size;
            total_new_records += batch.num_rows() as u64;

            // Build DataFile for the new compacted file
            let data_file = DataFileBuilder::default()
                .content(DataContentType::Data)
                .file_path(parquet_path.to_string_lossy().to_string())
                .file_format(DataFileFormat::Parquet)
                .partition(Struct::empty())
                .partition_spec_id(partition_spec.spec_id())
                .record_count(batch.num_rows() as u64)
                .file_size_in_bytes(file_size)
                .build()
                .map_err(|e| Error::General(format!("Failed to build DataFile: {}", e)))?;

            new_data_files.push(data_file);
        }

        // Create new snapshot
        let snapshot_id = chrono::Utc::now().timestamp_millis();
        let sequence_number = current_snapshot.sequence_number() + 1;
        let parent_snapshot_id = Some(current_snapshot.snapshot_id());

        // Write manifest for ALL files (large files kept + new compacted files)
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

        // Add large files (existing, unchanged)
        let mut total_records = 0u64;
        for data_file in &large_files {
            manifest_writer
                .add_file((*data_file).clone(), sequence_number)
                .map_err(|e| Error::General(format!("Failed to add existing file: {}", e)))?;
            total_records += data_file.record_count();
        }

        // Add new compacted files
        for data_file in &new_data_files {
            manifest_writer
                .add_file(data_file.clone(), sequence_number)
                .map_err(|e| Error::General(format!("Failed to add new file: {}", e)))?;
        }
        total_records += total_new_records;

        let manifest_file = manifest_writer
            .write_manifest_file()
            .await
            .map_err(|e| Error::General(format!("Failed to write manifest: {}", e)))?;

        // Write manifest list
        let manifest_list_filename = format!("snap-{}-0-{:016x}.avro", snapshot_id, timestamp_nanos);
        let manifest_list_path_new = metadata_dir.join(&manifest_list_filename);

        let manifest_list_output = file_io
            .new_output(&manifest_list_path_new.to_string_lossy())
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

        // Create snapshot with summary
        let timestamp_ms = chrono::Utc::now().timestamp_millis();
        let total_files = large_files.len() + new_data_files.len();

        let summary = Summary {
            operation: iceberg::spec::Operation::Replace,
            additional_properties: HashMap::from([
                ("total-records".to_string(), total_records.to_string()),
                ("total-data-files".to_string(), total_files.to_string()),
                (
                    "compacted-data-files".to_string(),
                    small_files.len().to_string(),
                ),
                (
                    "result-data-files".to_string(),
                    new_data_files.len().to_string(),
                ),
            ]),
        };

        let snapshot = Snapshot::builder()
            .with_snapshot_id(snapshot_id)
            .with_parent_snapshot_id(parent_snapshot_id)
            .with_sequence_number(sequence_number)
            .with_timestamp_ms(timestamp_ms)
            .with_manifest_list(manifest_list_path_new.to_string_lossy().to_string())
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

        // Now safe to delete old small files (after successful commit)
        for data_file in &small_files {
            let file_path = data_file.file_path();
            if let Err(e) = std::fs::remove_file(file_path) {
                // Log warning but don't fail - files will be cleaned by vacuum
                eprintln!(
                    "Warning: Failed to delete compacted file {}: {}",
                    file_path, e
                );
            }
        }

        Self::output_metrics(&metrics, &args.output)?;

        Ok(())
    }

    #[cfg(not(feature = "iceberg"))]
    async fn optimize_iceberg(_args: &OptimizeArgs) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Iceberg support not enabled".to_string(),
        })
    }

    /// Merge batches into larger ones targeting a specific file size
    fn merge_batches_to_target_size(
        batches: &[RecordBatch],
        schema: &Arc<arrow::datatypes::Schema>,
        target_size: u64,
    ) -> Result<Vec<RecordBatch>> {
        if batches.is_empty() {
            return Ok(Vec::new());
        }

        // Estimate bytes per row from first batch
        let first_batch = &batches[0];
        let bytes_per_row = if first_batch.num_rows() > 0 {
            // Rough estimate: sum of column sizes / rows
            let total_bytes: usize = first_batch
                .columns()
                .iter()
                .map(|col| col.get_buffer_memory_size())
                .sum();
            (total_bytes / first_batch.num_rows()).max(1)
        } else {
            100 // default estimate
        };

        let rows_per_file = (target_size as usize / bytes_per_row).max(1000);

        // Concatenate all batches first
        let combined = concat_batches(schema, batches)
            .map_err(|e| Error::General(format!("Failed to concat batches: {}", e)))?;

        let total_rows = combined.num_rows();

        // Split into target-sized chunks
        let mut result = Vec::new();
        let mut offset = 0;

        while offset < total_rows {
            let length = (total_rows - offset).min(rows_per_file);
            let chunk = combined.slice(offset, length);
            result.push(chunk);
            offset += length;
        }

        Ok(result)
    }

    /// Convert file URI to local path
    #[cfg(feature = "delta")]
    fn uri_to_path(uri: &str) -> String {
        if uri.starts_with("file://") {
            uri[7..].to_string()
        } else {
            uri.to_string()
        }
    }

    /// Output compaction metrics
    fn output_metrics(metrics: &CompactionMetrics, output_format: &str) -> Result<()> {
        match output_format {
            "json" => {
                let json = serde_json::json!({
                    "files_read": metrics.files_read,
                    "files_written": metrics.files_written,
                    "bytes_read": metrics.bytes_read,
                    "bytes_written": metrics.bytes_written,
                    "batches_processed": metrics.batches_processed,
                    "compression_ratio": if metrics.bytes_read > 0 {
                        metrics.bytes_written as f64 / metrics.bytes_read as f64
                    } else {
                        1.0
                    },
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json)
                        .map_err(|e| Error::General(format!("Failed to serialize: {}", e)))?
                );
            }
            _ => {
                println!();
                println!("{}", "Compaction complete!".green().bold());
                println!();
                println!("Files read:       {}", metrics.files_read.to_string().cyan());
                println!(
                    "Files written:    {}",
                    metrics.files_written.to_string().cyan()
                );
                println!("Bytes read:       {}", Self::format_bytes(metrics.bytes_read));
                println!(
                    "Bytes written:    {}",
                    Self::format_bytes(metrics.bytes_written)
                );

                if metrics.bytes_read > 0 {
                    let ratio = metrics.bytes_written as f64 / metrics.bytes_read as f64;
                    println!("Compression:      {:.1}%", ratio * 100.0);
                }
            }
        }

        Ok(())
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
