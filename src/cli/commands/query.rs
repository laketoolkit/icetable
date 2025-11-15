//! Query command implementation

use std::path::Path;

use crate::cli::output::formatter::OutputFormatter;
use crate::cli::parser::QueryArgs;
use crate::core::formats::{FormatHandlerRegistry, WriteOptions};
use crate::core::operations::query::QueryOperation;
use crate::core::storage::StorageBackendFactory;
use crate::error::Result;

/// Handler for query command
pub struct QueryCommand;

impl QueryCommand {
    /// Execute query command
    pub async fn execute(args: QueryArgs) -> Result<()> {
        // Create query operation
        let operation = QueryOperation::new();

        // Execute the SQL query
        let result = operation.execute(&args.sql, args.limit).await?;

        // Handle output routing
        if let Some(output_path) = args.file {
            // Save results to file
            Self::save_to_file(&result, &output_path, args.format.as_deref()).await?;

            println!(
                "Query executed successfully. {} rows written to {}",
                result.row_count,
                output_path.display()
            );
        } else {
            // Display results to stdout
            Self::display_results(&result, &args.output)?;

            // Print summary
            println!(
                "\n{} rows in result (from {} tables, {} total rows read)",
                result.row_count, result.tables_accessed, result.rows_read
            );
        }

        Ok(())
    }

    /// Save query results to a file
    async fn save_to_file(
        result: &crate::core::operations::query::QueryResult,
        path: &Path,
        format: Option<&str>,
    ) -> Result<()> {
        // Determine output format
        let _format = if let Some(f) = format {
            f
        } else {
            // Infer from file extension
            path.extension()
                .and_then(|s| s.to_str())
                .ok_or_else(|| {
                    crate::error::Error::InvalidFormat {
                        message: "Could not determine output format from file extension. Please specify --format".to_string(),
                    }
                })?
        };

        // Create storage backend
        let path_str = path
            .to_str()
            .ok_or_else(|| crate::error::Error::InvalidFormat {
                message: "Invalid path: cannot convert to string".to_string(),
            })?;
        let storage = StorageBackendFactory::create_backend(path_str).await?;

        // Create format handler
        let handler = FormatHandlerRegistry::global()
            .create_handler(path, storage)
            .await?;

        // Write the data
        let write_options = WriteOptions::default();
        handler
            .write(result.batches.clone(), &write_options)
            .await?;

        Ok(())
    }

    /// Display query results to stdout
    fn display_results(
        result: &crate::core::operations::query::QueryResult,
        output_format: &str,
    ) -> Result<()> {
        match output_format {
            "table" => {
                // Display as formatted table
                if result.is_empty() {
                    println!("Query returned no rows");
                } else {
                    for batch in &result.batches {
                        let table = OutputFormatter::format_record_batch(batch);
                        println!("{}", table);
                    }
                }
            }
            "json" => {
                // Display as JSON
                Self::display_as_json(result)?;
            }
            _ => {
                return Err(crate::error::Error::InvalidFormat {
                    message: format!(
                        "Unsupported output format '{}'. Use 'table' or 'json'",
                        output_format
                    ),
                });
            }
        }

        Ok(())
    }

    /// Display results as JSON
    fn display_as_json(result: &crate::core::operations::query::QueryResult) -> Result<()> {
        use datafusion::arrow::json::ArrayWriter;

        if result.is_empty() {
            println!("[]");
            return Ok(());
        }

        // Convert batches to JSON
        let mut output = Vec::new();
        {
            let mut writer = ArrayWriter::new(&mut output);
            for batch in &result.batches {
                writer.write(batch)?;
            }
            writer.finish()?;
        }

        // Print JSON output
        let json_str = String::from_utf8(output).map_err(|e| {
            crate::error::Error::General(format!("Failed to convert to UTF-8: {}", e))
        })?;

        println!("{}", json_str);

        Ok(())
    }
}
