//! Inspect command implementation
//!
//! Shows detailed information about table structure, metadata, and statistics.
//! Uses unified TableLoader for consistent table loading.

use colored::Colorize;

use crate::cli::parser::InspectArgs;
use crate::core::{CatalogConfig, TableExt, TableLoader};
use crate::error::Result;
use crate::utils::{track_memory_usage, with_cancellation, with_timeout};

/// Handler for inspect command
pub struct InspectCommand;

impl InspectCommand {
    /// Execute inspect command
    pub async fn execute(args: InspectArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        // Apply timeout and cancellation from global resource limits
        with_timeout(async {
            with_cancellation(async {
                // Estimate memory usage: depends on --deep flag and rows
                let estimated_memory = if args.deep {
                    512 * 1024 * 1024 // 512MB for deep inspection
                } else {
                    128 * 1024 * 1024 // 128MB for regular inspection
                };
                track_memory_usage(estimated_memory)?;

                Self::inspect_inner(args, catalog_config).await
            })
            .await
        })
        .await
    }

    async fn inspect_inner(args: InspectArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        // Get table reference from args
        let table_input = args.path.as_ref().ok_or_else(|| {
            crate::error::Error::General("Table path or identifier required".to_string())
        })?;

        // Load table using unified TableLoader
        let table = TableLoader::load_table(table_input, catalog_config.as_ref()).await?;

        // Execute inspection
        Self::execute_inspect(&table, args).await
    }

    /// Execute inspection on a loaded table
    async fn execute_inspect(
        table: &std::sync::Arc<iceberg::table::Table>,
        args: InspectArgs,
    ) -> Result<()> {
        // Get metadata
        let (metadata, version) = table.metadata_with_version();
        
        // Build output
        let mut output = String::new();
        
        // Header
        output.push_str(&format!("{}", "Iceberg Table Inspection".cyan().bold()));
        output.push_str("\n\n");
        
        // Basic table info
        output.push_str(&format!("{}: Iceberg v{}\n", "Format".bold(), version));
        output.push_str(&format!("{}: {}\n", "Location".bold(), metadata.location()));
        // Note: table_uuid is private in iceberg crate
        // Skipping UUID display for now
        
        if let Some(current_snapshot_id) = metadata.current_snapshot_id() {
            output.push_str(&format!("{}: {}\n", "Current Snapshot".bold(), current_snapshot_id));
        } else {
            output.push_str(&format!("{}: None\n", "Current Snapshot".bold()));
        }
        
        output.push_str(&format!("{}: {}\n", "Snapshot Count".bold(), metadata.snapshots().len()));
        // Note: last_updated_ms and last_column_id are private in iceberg crate
        // Using available public information
        output.push_str(&format!("{}: {}\n", "Format Version".bold(), metadata.format_version()));
        
        // Schema if requested or default
        if args.schema || (!args.metadata && !args.stats && !args.preview && !args.layout) {
            output.push_str("\n");
            output.push_str(&format!("{}", "Schema:".cyan().bold()));
            output.push_str("\n");
            
            let schema = metadata.current_schema();
            output.push_str(&format!("  Schema ID: {}\n", schema.schema_id()));
            
            let struct_type = schema.as_struct();
            output.push_str(&format!("  Fields: {}\n", struct_type.fields().len()));
            
            for field in struct_type.fields() {
                let nullable = if field.required { "" } else { " (nullable)" };
                output.push_str(&format!("  - {}: {:?}{}\n", field.name, field.field_type, nullable));
            }
        }
        
        // Partition spec if layout requested or default
        if args.layout || (!args.metadata && !args.stats && !args.preview && !args.schema) {
            output.push_str("\n");
            output.push_str(&format!("{}", "Partition Spec:".cyan().bold()));
            output.push_str("\n");
            
            let partition_spec = metadata.default_partition_spec();
            // Note: spec_id and fields are private in iceberg crate
            // Using available public information
            output.push_str(&format!("  Partition Spec ID: {}\n", partition_spec.spec_id()));
            output.push_str("  (Partition details require accessing private fields)\n");
            
            output.push_str(&format!("  Sort Order ID: {}\n", metadata.default_sort_order_id()));
        }
        
        // Stats if requested
        if args.stats {
            output.push_str("\n");
            output.push_str(&format!("{}", "Statistics:".cyan().bold()));
            output.push_str("\n");
            
            if let Some(current_snapshot_id) = metadata.current_snapshot_id() {
                if let Some(snapshot) = metadata.snapshot_by_id(current_snapshot_id) {
                    let summary = snapshot.summary();
                    output.push_str(&format!("  Operation: {:?}\n", summary.operation));
                    
                    for (key, value) in &summary.additional_properties {
                        if key.starts_with("added-") || key.starts_with("total-") || key.starts_with("deleted-") {
                            output.push_str(&format!("  {}: {}\n", key, value));
                        }
                    }
                }
            }
        }
        
        // Metadata if requested
        if args.metadata {
            output.push_str("\n");
            output.push_str(&format!("{}", "Metadata:".cyan().bold()));
            output.push_str("\n");
            
            // Note: properties is private in iceberg crate
            output.push_str("  Properties: (requires accessing private field)\n");
            
            output.push_str(&format!("  Current Schema ID: {}\n", metadata.current_schema_id()));
            output.push_str(&format!("  Schemas: {}\n", metadata.schemas_iter().count()));
            output.push_str(&format!("  Partition Specs: {}\n", metadata.partition_specs_iter().count()));
            output.push_str(&format!("  Sort Orders: {}\n", metadata.sort_orders_iter().count()));
        }
        
        // Preview if requested
        if args.preview {
            output.push_str("\n");
            output.push_str(&format!("{}", "Data Preview:".cyan().bold()));
            output.push_str("\n");
            output.push_str("  (Data preview requires scan implementation)\n");
            // TODO: Implement data preview using table.scan()
        }
        
        // Snapshots if verbose
        if args.verbose {
            output.push_str("\n");
            output.push_str(&format!("{}", "Snapshots:".cyan().bold()));
            output.push_str("\n");
            
            for snapshot in metadata.snapshots() {
                let timestamp = chrono::DateTime::from_timestamp_millis(snapshot.timestamp_ms())
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                
                output.push_str(&format!("  - ID: {}, Timestamp: {}, Operation: {:?}\n",
                    snapshot.snapshot_id(), timestamp, snapshot.summary().operation));
            }
        }
        
        println!("{}", output);
        Ok(())
    }
}

// Re-export common module
pub mod common;