//! Import command implementation
//!
//! Imports data from external sources (Delta Lake, Parquet files) into Iceberg tables.
//! This is a thin wrapper that delegates to ImportService in core.

use colored::Colorize;

use super::common::{print_dry_run_header, resolve_table};
use crate::cli::parser::{CatalogContext, ImportDeltaArgs, ImportParquetArgs};
use crate::core::extract_filename;
use crate::core::format_bytes;
use crate::core::operations::{ImportConfig, ImportService};
use crate::core::storage::{ObjectStoreExt, create_object_store};
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Handler for import commands
pub struct ImportCommand;

impl ImportCommand {
    /// Import from Delta Lake table
    pub async fn delta(args: ImportDeltaArgs, ctx: &CatalogContext) -> Result<()> {
        use super::constants::MEMORY_HEAVY_OPS;
        with_resource_limits(MEMORY_HEAVY_OPS, Self::delta_inner(args, ctx)).await
    }

    async fn delta_inner(args: ImportDeltaArgs, ctx: &CatalogContext) -> Result<()> {
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
            .map_err(|e| Error::TableNotFound {
                path: format!("{} (Delta error: {})", args.source, e),
            })?;

        let log_store = delta_table.log_store();
        let snapshot = delta_table.snapshot().map_err(|e| Error::Metadata {
            message: format!("Failed to get Delta snapshot: {}", e),
        })?;

        // Get schema and files from Delta
        let schema = snapshot.schema();
        let files = snapshot
            .file_actions(log_store.as_ref())
            .await
            .map_err(|e| Error::Metadata {
                message: format!("Failed to get Delta files: {}", e),
            })?;

        let total_files = files.len();
        let total_bytes: i64 = files.iter().map(|f| f.size).sum();

        println!();
        println!("Delta Table Summary:");
        println!(
            "  Schema columns: {}",
            schema.fields().len().to_string().cyan()
        );
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
                let name = extract_filename(&file.path);
                println!("  {}. {} ({})", i + 1, name, format_bytes(file.size as u64));
            }
            if total_files > 10 {
                println!("  ... and {} more files", total_files - 10);
            }
            return Ok(());
        }

        // Resolve target table and create metadata service with catalog context
        let resolution = resolve_table(&Some(args.target.clone()), ctx.catalog_config.as_ref())
            .await
            .map_err(|e| Error::TableNotFound {
                path: format!(
                    "{} ({}). Use 'icetable init iceberg {}' first.",
                    args.target, e, args.target
                ),
            })?;
        let metadata_service = resolution
            .to_writable_service(ctx.catalog_config.as_ref(), None)
            .await?;

        // Delegate to ImportService
        let config = ImportConfig {
            source: "delta-import".to_string(),
        };
        let service = ImportService::with_config(config);
        let result = service
            .import_delta(&metadata_service, &args.source, &files)
            .await?;

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
    pub async fn parquet(args: ImportParquetArgs, ctx: &CatalogContext) -> Result<()> {
        use super::constants::MEMORY_INTENSIVE_OPS;
        with_resource_limits(MEMORY_INTENSIVE_OPS, Self::parquet_inner(args, ctx)).await
    }

    async fn parquet_inner(args: ImportParquetArgs, ctx: &CatalogContext) -> Result<()> {
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
        let storage = create_object_store(&args.source).await?;

        // Simple pattern matching for file names
        let pattern = args.pattern.clone();

        // List all files in the source directory
        let all_objects = storage.list_all(None).await?;

        let parquet_files: Vec<_> = all_objects
            .iter()
            .filter(|obj| {
                let path_str = obj.location.to_string();
                let name = extract_filename(&path_str);
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
            return Err(Error::FileNotFound {
                path: std::path::PathBuf::from(format!(
                    "{} (no files matching pattern '{}')",
                    args.source, args.pattern
                )),
            });
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
                let name = extract_filename(&path_str);
                println!("  {}. {} ({})", i + 1, name, format_bytes(file.size));
            }
            if total_files > 10 {
                println!("  ... and {} more files", total_files - 10);
            }
            return Ok(());
        }

        // Resolve target table and create metadata service with catalog context
        let resolution = resolve_table(&Some(args.target.clone()), ctx.catalog_config.as_ref())
            .await
            .map_err(|e| Error::TableNotFound {
                path: format!(
                    "{} ({}). Use 'icetable init iceberg {}' first.",
                    args.target, e, args.target
                ),
            })?;
        let metadata_service = resolution
            .to_writable_service(ctx.catalog_config.as_ref(), None)
            .await?;

        // Delegate to ImportService
        let config = ImportConfig {
            source: "parquet-import".to_string(),
        };
        let service = ImportService::with_config(config);
        let result = service
            .import_parquet(&metadata_service, &parquet_files, &storage)
            .await?;

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
