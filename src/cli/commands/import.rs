//! Import command implementation
//!
//! Imports data from external sources (Delta Lake, Parquet files) into Iceberg tables.

use colored::Colorize;

#[cfg(feature = "delta")]
use crate::cli::parser::ImportDeltaArgs;
use crate::cli::parser::ImportParquetArgs;
use crate::core::storage::traits::{GetOptions, ObjectMetadata};
use crate::error::{Error, Result};

/// Handler for import commands
pub struct ImportCommand;

impl ImportCommand {
    /// Import from Delta Lake table
    #[cfg(feature = "delta")]
    pub async fn delta(args: ImportDeltaArgs) -> Result<()> {
        use deltalake::DeltaTableBuilder;

        println!(
            "{} Delta table from {} to Iceberg at {}",
            if args.dry_run {
                "Analyzing"
            } else {
                "Importing"
            }
            .green(),
            args.source,
            args.target
        );

        // Load the Delta table
        let delta_table = DeltaTableBuilder::from_uri(&args.source)
            .load()
            .await
            .map_err(|e| Error::General(format!("Failed to load Delta table: {}", e)))?;

        let log_store = delta_table.log_store();
        let snapshot = delta_table
            .snapshot()
            .map_err(|e| Error::General(format!("Failed to get Delta snapshot: {}", e)))?;

        // Get schema and files from Delta
        let schema = snapshot.schema();
        let files = snapshot
            .file_actions(log_store.as_ref())
            .await
            .map_err(|e| Error::General(format!("Failed to get Delta files: {}", e)))?;

        let total_files = files.len();
        let total_bytes: i64 = files.iter().map(|f| f.size).sum();

        println!();
        println!("Delta Table Summary:");
        println!(
            "  Schema columns: {}",
            schema.fields().len().to_string().cyan()
        );
        println!("  Data files:     {}", total_files.to_string().cyan());
        println!(
            "  Total size:     {}",
            crate::core::format_bytes(total_bytes as u64)
        );

        if args.dry_run {
            println!();
            println!("{}", "DRY RUN - No changes made".yellow().bold());
            println!();
            println!("Schema:");
            for field in schema.fields() {
                println!("  {} ({})", field.name().cyan(), field.data_type());
            }
            println!();
            println!("Files to import:");
            for (i, file) in files.iter().take(10).enumerate() {
                let name = file.path.rsplit('/').next().unwrap_or(&file.path);
                println!(
                    "  {}. {} ({})",
                    i + 1,
                    name,
                    crate::core::format_bytes(file.size as u64)
                );
            }
            if total_files > 10 {
                println!("  ... and {} more files", total_files - 10);
            }
            return Ok(());
        }

        // Create/load Iceberg table and import data
        Self::import_to_iceberg(&args.target, &args.source, &files, args.name.as_deref()).await?;

        println!();
        println!("{}", "Import complete!".green().bold());
        println!(
            "Imported {} files ({}) from Delta to Iceberg",
            total_files,
            crate::core::format_bytes(total_bytes as u64)
        );

        Ok(())
    }

    /// Helper to import files into Iceberg table
    #[cfg(feature = "delta")]
    async fn import_to_iceberg(
        target_path: &str,
        source_path: &str,
        files: &[deltalake::kernel::Add],
        _table_name: Option<&str>,
    ) -> Result<()> {
        use crate::core::metadata::IcebergMetadataService;
        use std::collections::HashMap;

        // Check if Iceberg table exists
        let metadata_service = match IcebergMetadataService::new_async(target_path.to_string())
            .await
        {
            Ok(service) => service,
            Err(_) => {
                // Table doesn't exist - create it
                return Err(Error::General(format!(
                    "Target Iceberg table does not exist at '{}'. Use 'icebergctl init iceberg {}' first.",
                    target_path, target_path
                )));
            }
        };

        // Build data file changes
        let mut changes = crate::core::metadata::DataFileChanges::new();

        for file in files {
            // Construct the source file path
            let file_path = if file.path.starts_with("s3://") || file.path.starts_with('/') {
                file.path.clone()
            } else {
                format!("{}/{}", source_path.trim_end_matches('/'), file.path)
            };

            changes.added.push(crate::core::metadata::DataFileInfo {
                path: file_path,
                size: file.size as u64,
                record_count: file
                    .stats
                    .as_ref()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                    .and_then(|v| v.get("numRecords").and_then(|n| n.as_u64()))
                    .unwrap_or(0),
                partition: HashMap::new(),
            });
        }

        // Write snapshot
        use crate::core::metadata::{MetadataService, OperationType};
        let mut summary = HashMap::new();
        summary.insert("source".to_string(), "delta-import".to_string());

        metadata_service
            .write_snapshot(changes, OperationType::Append, summary)
            .await?;

        Ok(())
    }

    /// Import from Parquet files
    pub async fn parquet(args: ImportParquetArgs) -> Result<()> {
        use crate::core::storage::StorageBackendFactory;
        use crate::core::storage::traits::ListOptions;

        println!(
            "{} Parquet files from {} to Iceberg at {}",
            if args.dry_run {
                "Analyzing"
            } else {
                "Importing"
            }
            .green(),
            args.source,
            args.target
        );

        // Create storage backend for source
        let storage = StorageBackendFactory::create_backend(&args.source).await?;

        // Simple pattern matching for file names
        let pattern = args.pattern.clone();

        let list_opts = ListOptions {
            prefix: Some(args.source.clone()),
            delimiter: None,
            max_results: None,
            continuation_token: None,
        };

        let result = storage.list(&list_opts).await?;

        let parquet_files: Vec<_> = result
            .objects
            .iter()
            .filter(|obj| {
                let name = obj.path.rsplit('/').next().unwrap_or(&obj.path);
                if name.ends_with(".parquet") {
                    // Simple glob matching: *.parquet matches all, **/*.parquet matches all
                    if pattern == "*.parquet" || pattern == "**/*.parquet" {
                        return true;
                    }
                    // Check if name matches pattern (basic support)
                    name.ends_with(".parquet")
                } else {
                    false
                }
            })
            .collect();

        if parquet_files.is_empty() {
            return Err(Error::General(format!(
                "No Parquet files found matching pattern '{}' at '{}'",
                args.pattern, args.source
            )));
        }

        let total_files = parquet_files.len();
        let total_bytes: u64 = parquet_files.iter().map(|f| f.size).sum();

        println!();
        println!("Parquet Files Summary:");
        println!("  Files found:  {}", total_files.to_string().cyan());
        println!("  Total size:   {}", crate::core::format_bytes(total_bytes));

        if args.dry_run {
            println!();
            println!("{}", "DRY RUN - No changes made".yellow().bold());
            println!();
            println!("Files to import:");
            for (i, file) in parquet_files.iter().take(10).enumerate() {
                let name = file.path.rsplit('/').next().unwrap_or(&file.path);
                println!(
                    "  {}. {} ({})",
                    i + 1,
                    name,
                    crate::core::format_bytes(file.size)
                );
            }
            if total_files > 10 {
                println!("  ... and {} more files", total_files - 10);
            }
            return Ok(());
        }

        // Import files into Iceberg
        Self::import_parquet_to_iceberg(&args.target, &parquet_files, &storage).await?;

        println!();
        println!("{}", "Import complete!".green().bold());
        println!(
            "Imported {} Parquet files ({}) to Iceberg",
            total_files,
            crate::core::format_bytes(total_bytes)
        );

        Ok(())
    }

    /// Helper to import parquet files into Iceberg table
    async fn import_parquet_to_iceberg(
        target_path: &str,
        files: &[&ObjectMetadata],
        storage: &std::sync::Arc<dyn crate::core::storage::StorageBackend>,
    ) -> Result<()> {
        use crate::core::metadata::IcebergMetadataService;
        use parquet::file::reader::FileReader;
        use std::collections::HashMap;

        // Load Iceberg table
        let metadata_service = match IcebergMetadataService::new_async(target_path.to_string())
            .await
        {
            Ok(service) => service,
            Err(_) => {
                return Err(Error::General(format!(
                    "Target Iceberg table does not exist at '{}'. Use 'icebergctl init iceberg {}' first.",
                    target_path, target_path
                )));
            }
        };

        // Build data file changes
        let mut changes = crate::core::metadata::DataFileChanges::new();

        for file in files {
            // Read parquet file to get row count
            let get_opts = GetOptions::default();
            let data = storage.get(&file.path, &get_opts).await?;
            let reader = parquet::file::reader::SerializedFileReader::new(data)
                .map_err(|e| Error::General(format!("Failed to read parquet: {}", e)))?;
            let metadata = reader.metadata();
            let row_count: i64 = metadata.row_groups().iter().map(|rg| rg.num_rows()).sum();

            changes.added.push(crate::core::metadata::DataFileInfo {
                path: file.path.clone(),
                size: file.size,
                record_count: row_count as u64,
                partition: HashMap::new(),
            });
        }

        // Write snapshot
        use crate::core::metadata::{MetadataService, OperationType};
        let mut summary = HashMap::new();
        summary.insert("source".to_string(), "parquet-import".to_string());

        metadata_service
            .write_snapshot(changes, OperationType::Append, summary)
            .await?;

        Ok(())
    }
}
