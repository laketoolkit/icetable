//! Diff command implementation

use std::path::Path;
use std::sync::Arc;

use crate::cli::output::OutputFormatter;
use crate::cli::parser::DiffArgs;
use crate::core::formats::FormatHandlerFactory;
use crate::core::operations::diff::{DiffOperation, DiffOptions};
use crate::core::storage::StorageBackendFactory;
use crate::error::{Error, Result};

/// Handler for diff command
pub struct DiffCommand;

impl DiffCommand {
    /// Execute diff command
    pub async fn execute(args: DiffArgs) -> Result<()> {
        let left_path = Path::new(&args.left);
        let right_path = Path::new(&args.right);

        // Create storage backends for each file (supports local and cloud)
        let left_storage = StorageBackendFactory::create_backend(&args.left).await?;
        let right_storage = StorageBackendFactory::create_backend(&args.right).await?;

        // Get format handlers
        let left_handler = FormatHandlerFactory::create_handler(left_path, left_storage).await?;
        let right_handler = FormatHandlerFactory::create_handler(right_path, right_storage).await?;

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
        let output = match args.output.as_str() {
            "json" => {
                // Calculate row deltas
                let (rows_added, rows_removed, rows_delta, rows_delta_pct) =
                    if let Some((left, right)) = result.metadata_diff.num_rows {
                        let delta = right - left;
                        let delta_pct = if left > 0 {
                            ((delta as f64) / (left as f64)) * 100.0
                        } else {
                            0.0
                        };
                        if delta > 0 {
                            (delta, 0, delta, delta_pct)
                        } else {
                            (0, delta.abs(), delta, delta_pct)
                        }
                    } else {
                        (0, 0, 0, 0.0)
                    };

                // Create JSON matching visual structure
                let serializable = serde_json::json!({
                    "files": {
                        "left": result.left_path,
                        "right": result.right_path,
                    },
                    "rows": {
                        "total": {
                            "left": result.metadata_diff.num_rows.map(|(l, _)| l),
                            "right": result.metadata_diff.num_rows.map(|(_, r)| r),
                            "delta": rows_delta,
                            "delta_percent": rows_delta_pct,
                        },
                        "added": rows_added,
                        "removed": rows_removed,
                    },
                    "schema": {
                        "is_identical": result.schema_diff.is_identical(),
                        "added_columns": result.schema_diff.columns_added.iter().map(|c| {
                            serde_json::json!({
                                "name": c.name,
                                "data_type": c.data_type,
                                "nullable": c.nullable,
                            })
                        }).collect::<Vec<_>>(),
                        "removed_columns": result.schema_diff.columns_removed.iter().map(|c| {
                            serde_json::json!({
                                "name": c.name,
                                "data_type": c.data_type,
                                "nullable": c.nullable,
                            })
                        }).collect::<Vec<_>>(),
                        "modified_columns": result.schema_diff.columns_modified.iter().map(|c| {
                            serde_json::json!({
                                "name": c.name,
                                "type_change": c.type_change,
                                "nullability_change": c.nullability_change,
                            })
                        }).collect::<Vec<_>>(),
                    },
                    "metadata": {
                        "file_properties": {
                            "rows": result.metadata_diff.num_rows,
                            "size": {
                                "compressed": result.metadata_diff.compressed_size,
                                "uncompressed": result.metadata_diff.uncompressed_size,
                            },
                            "compression": result.metadata_diff.compression,
                            "version": result.metadata_diff.format_version,
                        },
                        "custom_metadata": {
                            "added": result.metadata_diff.custom_metadata.added,
                            "removed": result.metadata_diff.custom_metadata.removed,
                            "modified": result.metadata_diff.custom_metadata.modified,
                        }
                    },
                    "column_statistics": if args.verbose {
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
