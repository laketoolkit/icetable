//! Generate command implementation
//!
//! Thin wrapper that delegates to core::operations::generate

use std::sync::Arc;

use colored::Colorize;

use crate::cli::parser::{GenerateArgs, SchemaTemplate as CliSchemaTemplate};
use crate::core::operations::generate::{
    GenerateConfig, GenerateOperation, GenerateResult, SchemaTemplate, parse_schema_string,
};
use crate::utils::core::format_bytes;
use crate::error::Result;
use crate::utils::{temp_dir_with_cleanup, with_resource_limits};

/// Handler for generate command
pub struct GenerateCommand;

impl GenerateCommand {
    /// Execute generate command
    pub async fn execute(args: GenerateArgs) -> Result<()> {
        let schema = Self::resolve_schema(&args)?;

        if args.dry_run {
            return Self::print_dry_run(&args, &schema);
        }

        let config = GenerateConfig {
            path: args.path.clone(),
            schema: Arc::new(schema),
            rows: args.rows,
            files: args.files,
            partition_columns: args.partition_by.clone().unwrap_or_default(),
            seed: args.seed.unwrap_or(42),
            target_file_size: args.target_file_size,
        };

        println!(
            "{} Generating synthetic Iceberg table...",
            "->".cyan().bold()
        );
        println!("  Location: {}", config.path);
        println!("  Schema: {} columns", config.schema.fields().len());
        println!("  Rows: {}", config.rows);
        println!("  Files: {}", config.files);

        // Apply resource limits (timeout, cancellation, memory tracking)
        let estimated_memory = Self::estimate_memory_usage(&config);
        let result = with_resource_limits(estimated_memory, async {
            // Create temp directory for intermediate files (will be cleaned up on cancellation)
            let _temp_dir = temp_dir_with_cleanup()?;
            GenerateOperation::execute(config).await
        })
        .await?;

        Self::print_result(&result, &args.output);

        Ok(())
    }

    fn resolve_schema(args: &GenerateArgs) -> Result<arrow::datatypes::Schema> {
        if let Some(schema_str) = &args.schema {
            parse_schema_string(schema_str)
        } else if let Some(template) = &args.template {
            Ok(Self::cli_template_to_core(*template).to_schema())
        } else {
            // Default: events template
            Ok(SchemaTemplate::Events.to_schema())
        }
    }

    fn cli_template_to_core(cli_template: CliSchemaTemplate) -> SchemaTemplate {
        match cli_template {
            CliSchemaTemplate::Events => SchemaTemplate::Events,
            CliSchemaTemplate::Transactions => SchemaTemplate::Transactions,
            CliSchemaTemplate::Sensors => SchemaTemplate::Sensors,
            CliSchemaTemplate::Users => SchemaTemplate::Users,
            CliSchemaTemplate::WebLogs => SchemaTemplate::WebLogs,
        }
    }

    fn print_dry_run(args: &GenerateArgs, schema: &arrow::datatypes::Schema) -> Result<()> {
        let rows_per_file = (args.rows / args.files as u64).max(1);
        let partition_cols = args.partition_by.clone().unwrap_or_default();

        println!("{} Dry run - no data will be generated", "->".cyan().bold());
        println!();
        println!("Configuration:");
        println!("  Location: {}", args.path);
        println!("  Total rows: {}", args.rows);
        println!("  Files: {}", args.files);
        println!("  Rows per file: ~{}", rows_per_file);
        println!(
            "  Seed: {}",
            args.seed
                .map(|s| s.to_string())
                .unwrap_or_else(|| "42 (default)".to_string())
        );
        println!();
        println!("Schema ({} columns):", schema.fields().len());
        for field in schema.fields() {
            println!(
                "  - {}: {} {}",
                field.name(),
                format!("{:?}", field.data_type()).to_lowercase(),
                if field.is_nullable() {
                    "(nullable)"
                } else {
                    "(required)"
                }
            );
        }

        if !partition_cols.is_empty() {
            println!();
            println!("Partitioning:");
            for col in &partition_cols {
                println!("  - {}", col);
            }
        }

        Ok(())
    }

    fn print_result(result: &GenerateResult, output: &str) {
        for file in &result.data_files {
            println!(
                "  {} Written {} ({} rows, {} bytes)",
                "v".green(),
                file.path.split('/').next_back().unwrap_or(&file.path),
                file.record_count,
                file.size
            );
        }

        println!(
            "  {} Created Iceberg metadata (snapshot {})",
            "v".green(),
            result.snapshot_id
        );

        println!(
            "\n{} Generated table with {} rows in {} files ({})",
            "v".green().bold(),
            result.total_rows,
            result.files_created,
            format_bytes(result.total_bytes)
        );

        if output == "json" {
            let json_result = serde_json::json!({
                "status": "success",
                "location": result.table_path,
                "rows": result.total_rows,
                "files": result.files_created,
                "total_bytes": result.total_bytes,
                "data_files": result.data_files.iter().map(|f| &f.path).collect::<Vec<_>>(),
                "metadata_path": result.metadata_path,
                "snapshot_id": result.snapshot_id
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json_result)
                    .expect("JSON serialization of GenerateResult should never fail")
            );
        }
    }

    /// Estimate memory usage for generation
    fn estimate_memory_usage(config: &GenerateConfig) -> u64 {
        // Estimate based on:
        // 1. Schema metadata: ~100 bytes per column
        // 2. Batch buffers: rows * avg column size
        // 3. Parquet buffers: ~1.5x batch size

        let schema_size = config.schema.fields().len() as u64 * 100;

        // Average column size estimation
        let avg_column_size_bytes = 32; // Conservative estimate for mixed types

        let rows_per_file = (config.rows / config.files as u64).max(1);
        let batch_size =
            rows_per_file * config.schema.fields().len() as u64 * avg_column_size_bytes;

        // Parquet compression buffers
        let parquet_buffer_size = batch_size * 3 / 2;

        // Total for one file at a time (streaming)
        schema_size + batch_size + parquet_buffer_size
    }
}
