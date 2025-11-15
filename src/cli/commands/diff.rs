//! Diff command implementation

use std::path::Path;
use std::sync::Arc;

use crate::cli::output::OutputFormatter;
use crate::cli::parser::DiffArgs;
use crate::core::formats::FormatHandlerFactory;
use crate::core::operations::diff::{DiffOperation, DiffOptions};
use crate::core::storage::LocalBackend;
use crate::error::{Error, Result};

/// Handler for diff command
pub struct DiffCommand;

impl DiffCommand {
    /// Execute diff command
    pub async fn execute(args: DiffArgs) -> Result<()> {
        let left_path = Path::new(&args.left);
        let right_path = Path::new(&args.right);

        if !left_path.exists() {
            return Err(Error::FileNotFound {
                path: left_path.to_path_buf(),
            });
        }

        if !right_path.exists() {
            return Err(Error::FileNotFound {
                path: right_path.to_path_buf(),
            });
        }

        // Create storage backend
        let storage = Arc::new(LocalBackend::new()?);

        // Get format handlers
        let left_handler = FormatHandlerFactory::create_handler(left_path, storage.clone()).await?;
        let right_handler = FormatHandlerFactory::create_handler(right_path, storage).await?;

        // Convert to Arc for DiffOperation
        let left_arc = Arc::from(left_handler);
        let right_arc = Arc::from(right_handler);

        // Create operation
        let operation = DiffOperation::new(left_arc, right_arc);

        // Build options from args
        let options = DiffOptions {
            verbose: args.verbose,
        };

        // Execute operation with paths
        let result = operation
            .execute(&options, args.left.clone(), args.right.clone())
            .await?;

        // Format output
        let output = match args.format.as_str() {
            "json" => {
                // Create serializable version of the result
                let serializable = serde_json::json!({
                    "left_path": result.left_path,
                    "right_path": result.right_path,
                    "metadata_diff": {
                        "num_rows": result.metadata_diff.num_rows,
                        "compressed_size": result.metadata_diff.compressed_size,
                        "uncompressed_size": result.metadata_diff.uncompressed_size,
                        "compression": result.metadata_diff.compression,
                        "format_version": result.metadata_diff.format_version,
                        "custom_metadata": {
                            "added": result.metadata_diff.custom_metadata.added,
                            "removed": result.metadata_diff.custom_metadata.removed,
                            "modified": result.metadata_diff.custom_metadata.modified,
                        }
                    },
                    "schema_diff": {
                        "columns_added": result.schema_diff.columns_added.iter().map(|c| {
                            serde_json::json!({
                                "name": c.name,
                                "data_type": c.data_type,
                                "nullable": c.nullable,
                            })
                        }).collect::<Vec<_>>(),
                        "columns_removed": result.schema_diff.columns_removed.iter().map(|c| {
                            serde_json::json!({
                                "name": c.name,
                                "data_type": c.data_type,
                                "nullable": c.nullable,
                            })
                        }).collect::<Vec<_>>(),
                        "columns_modified": result.schema_diff.columns_modified.iter().map(|c| {
                            serde_json::json!({
                                "name": c.name,
                                "type_change": c.type_change,
                                "nullability_change": c.nullability_change,
                            })
                        }).collect::<Vec<_>>(),
                        "is_identical": result.schema_diff.is_identical(),
                    },
                    "column_stats_diff": if args.verbose {
                        result.column_stats_diff.iter().map(|s| {
                            serde_json::json!({
                                "name": s.name,
                                "null_count": s.null_count,
                                "distinct_count_approx": s.distinct_count_approx,
                                "min_value": s.min_value,
                                "max_value": s.max_value,
                                "mean": s.mean,
                            })
                        }).collect::<Vec<_>>()
                    } else {
                        Vec::new()
                    },
                });
                serde_json::to_string_pretty(&serializable)
                    .map_err(|e| Error::General(e.to_string()))?
            }
            "text" | _ => OutputFormatter::format_diff_result(&result),
        };

        println!("{}", output);

        Ok(())
    }
}
