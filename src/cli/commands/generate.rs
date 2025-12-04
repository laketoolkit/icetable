//! Generate command implementation
//!
//! Generates table metadata like statistics for Iceberg tables.

use colored::Colorize;
use std::sync::Arc;

use crate::cli::parser::{GenerateArgs, GenerateCommands, GenerateStatsArgs};
use crate::config::ResolvePath;
use crate::core::storage::{StorageBackend, StorageBackendFactory};
use crate::error::{Error, Result};

/// Handler for generate command
pub struct GenerateCommand;

impl GenerateCommand {
    /// Execute generate command
    pub async fn execute(args: GenerateArgs) -> Result<()> {
        match args.command {
            GenerateCommands::Manifest(a) => {
                let table_path = a.path.resolve()?;
                Self::generate_manifest_cmd(&table_path).await
            }
            GenerateCommands::Stats(a) => {
                let table_path = a.path.resolve()?;
                Self::generate_stats_cmd(&table_path, &a).await
            }
        }
    }

    async fn generate_manifest_cmd(table_path: &str) -> Result<()> {
        let storage = StorageBackendFactory::create_backend(table_path).await?;

        if !Self::is_iceberg_table(table_path, &storage).await {
            return Err(Error::General(format!(
                "Path '{}' is not an Iceberg table",
                table_path
            )));
        }

        // Iceberg manages manifests automatically
        println!(
            "{}",
            "Iceberg manages manifests automatically - no generation needed".yellow()
        );
        println!(
            "{}",
            "Use 'icectl repair --sync-metadata' to fix manifest issues".dimmed()
        );
        println!(
            "{}",
            "Use 'icectl optimize manifests' to compact manifests".dimmed()
        );
        Ok(())
    }

    async fn generate_stats_cmd(table_path: &str, args: &GenerateStatsArgs) -> Result<()> {
        let storage = StorageBackendFactory::create_backend(table_path).await?;

        if !Self::is_iceberg_table(table_path, &storage).await {
            return Err(Error::General(format!(
                "Path '{}' is not an Iceberg table",
                table_path
            )));
        }

        Self::generate_iceberg_stats(table_path, args).await
    }

    /// Check if path is an Iceberg table
    async fn is_iceberg_table(path: &str, storage: &Arc<dyn StorageBackend>) -> bool {
        use crate::core::storage::traits::ListOptions;

        let iceberg_prefix = format!("{}/metadata/", path.trim_end_matches('/'));
        let list_opts = ListOptions {
            prefix: Some(iceberg_prefix),
            delimiter: None,
            max_results: Some(1),
            continuation_token: None,
        };

        matches!(storage.list(&list_opts).await, Ok(result) if !result.objects.is_empty())
    }

    async fn generate_iceberg_stats(path: &str, args: &GenerateStatsArgs) -> Result<()> {
        use crate::core::metadata::IcebergMetadataService;

        println!("{} Iceberg statistics at {}", "Generating".green(), path);

        let service = IcebergMetadataService::new_async(path.to_string()).await?;
        let (metadata, _) = service.load_metadata().await?;

        // Get schema for column info
        let schema = metadata.current_schema();
        let columns: Vec<String> = if let Some(cols) = &args.columns {
            cols.clone()
        } else {
            schema
                .as_struct()
                .fields()
                .iter()
                .map(|f| f.name.clone())
                .collect()
        };

        println!();
        println!("Columns to compute: {}", columns.join(", "));

        if !args.force {
            println!();
            println!("{}", "Iceberg stores statistics in manifest files".yellow());
            println!(
                "{}",
                "Use --force to recompute by reading all data files".dimmed()
            );
            return Ok(());
        }

        // TODO: Implement statistics recomputation using Puffin files
        println!();
        println!(
            "{}",
            "Statistics recomputation requires reading data files".yellow()
        );
        println!(
            "{}",
            "This is a planned feature - Iceberg computes stats during writes".dimmed()
        );

        Ok(())
    }
}
