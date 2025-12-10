//! Import command implementation
//!
//! Imports data from external sources (Delta Lake, Parquet files) into Iceberg tables.
//! This is a thin wrapper that delegates to ImportService in core.

use colored::Colorize;

#[cfg(feature = "delta")]
use crate::cli::parser::ImportDeltaArgs;
use crate::cli::parser::ImportParquetArgs;
use crate::core::operations::{ImportConfig, ImportService};
use crate::core::storage::{create_object_store, ObjectStoreExt};
use crate::core::format_bytes;
use crate::error::{Error, Result};
use super::common::print_dry_run_header;
use crate::utils::with_resource_limits;

/// Handler for import commands
pub struct ImportCommand;

impl ImportCommand {
    /// Import from Delta Lake table
    #[cfg(feature = "delta")]
    pub async fn delta(args: ImportDeltaArgs) -> Result<()> {
        // Apply resource limits (timeout, cancellation, memory tracking)
        const ESTIMATED_MEMORY: u64 = 128 * 1024 * 1024; // 128MB for Delta operations
        with_resource_limits(ESTIMATED_MEMORY, Self::delta_inner(args)).await
    }

    #[cfg(feature = "delta")]
    async fn delta_inner(args: ImportDeltaArgs) -> Result<()> {
        use deltalake::DeltaTableBuilder;

        println!(
            "{} Delta table from {} to Iceberg at {}",
            if args.dry_run { "Analyzing" } else { "Importing" }.green(),
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
        println!("  Schema columns: {}", schema.fields().len().to_string().cyan());
        println!("  Data files:     {}", total_files.to_string().cyan());
        println!("  Total size:     {}", format_bytes(total_bytes as u64));

        if args.dry_run {
            println!();
            print_dry_run_header();
            println!("Schema:");
            for field in schema.fields() {
                println!("  {} ({})", field.name().cyan(), field.data_type());
            }
            println!();
            println!("Files to import:");
            for (i, file) in files.iter().take(10).enumerate() {
                let name = file.path.rsplit('/').next().unwrap_or(&file.path);
                println!("  {}. {} ({})", i + 1, name, format_bytes(file.size));
            }
            if total_files > 10 {
                println!("  ... and {} more files", total_files - 10);
            }
            return Ok(());
        }

        // Delegate to ImportService
        let config = ImportConfig {
            source: "delta-import".to_string(),
        };
        let service = ImportService::with_config(config);
        let result = service.import_delta(&args.target, &args.source, &files).await?;

        println!();
        println!("{}", "Import complete!".green().bold());
        println!(
            "Imported {} files ({}) from Delta to Iceberg",
            result.files_imported,
            format_bytes(result.bytes_imported)
        );

        Ok(())
    }

    /// Import from Parquet files
    pub async fn parquet(args: ImportParquetArgs) -> Result<()> {
        // Apply resource limits (timeout, cancellation, memory tracking)
        const ESTIMATED_MEMORY: u64 = 256 * 1024 * 1024; // 256MB for Parquet operations
        with_resource_limits(ESTIMATED_MEMORY, Self::parquet_inner(args)).await
    }

    async fn parquet_inner(args: ImportParquetArgs) -> Result<()> {
        println!(
            "{} Parquet files from {} to Iceberg at {}",
            if args.dry_run { "Analyzing" } else { "Importing" }.green(),
            args.source,
            args.target
        );

        // Create storage backend for source
        let storage = create_object_store(&args.source).await?;

        // Simple pattern matching for file names
        let pattern = args.pattern.clone();

        // List all files in the source directory
        let all_objects = storage.list_all(None).await?;

        let parquet_files: Vec<_> = all_objects
            .iter()
            .filter(|obj| {
                let path_str = obj.location.to_string();
                let name = path_str.rsplit('/').next().unwrap_or(&path_str);
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
        println!("  Total size:   {}", format_bytes(total_bytes));

        if args.dry_run {
            println!();
            print_dry_run_header();
            println!("Files to import:");
            for (i, file) in parquet_files.iter().take(10).enumerate() {
                let path_str = file.location.to_string();
                let name = path_str.rsplit('/').next().unwrap_or(&path_str);
                println!("  {}. {} ({})", i + 1, name, format_bytes(file.size));
            }
            if total_files > 10 {
                println!("  ... and {} more files", total_files - 10);
            }
            return Ok(());
        }

        // Delegate to ImportService
        let config = ImportConfig {
            source: "parquet-import".to_string(),
        };
        let service = ImportService::with_config(config);
        let result = service.import_parquet(&args.target, &parquet_files, &storage).await?;

        println!();
        println!("{}", "Import complete!".green().bold());
        println!(
            "Imported {} Parquet files ({}) to Iceberg",
            result.files_imported,
            format_bytes(result.bytes_imported)
        );

        Ok(())
    }
}
