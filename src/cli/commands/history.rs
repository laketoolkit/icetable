//! History command implementation
//!
//! Shows version history for Iceberg tables.
//! Thin wrapper that delegates to HistoryService in core.

use colored::Colorize;

use super::common::print_json;
use crate::cli::parser::HistoryArgs;
use crate::config::ResolvePath;
use crate::core::operations::{HistoryConfig, HistoryEntry, HistoryService};
use crate::core::TableLoader;
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Handler for history command
pub struct HistoryCommand;

impl HistoryCommand {
    /// Execute history command
    pub async fn execute(args: HistoryArgs) -> Result<()> {
        const ESTIMATED_MEMORY: u64 = 64 * 1024 * 1024; // 64MB for history
        with_resource_limits(ESTIMATED_MEMORY, Self::execute_inner(args)).await
    }

    async fn execute_inner(args: HistoryArgs) -> Result<()> {
        // 1. Resolve path from args or config
        let table_path = args.path.resolve()?;

        // 2. Load table using unified TableLoader
        let table = TableLoader::load_table(&table_path, None).await?;

        // 3. Verify format
        if let Some(ref fmt) = args.format
            && fmt.to_lowercase() != "iceberg"
        {
            return Err(Error::UnsupportedFeature {
                feature: "Only Iceberg tables are supported. Use 'icetable import delta' to convert Delta tables.".to_string(),
            });
        }

        // 4. Build config and delegate to service
        let config = HistoryConfig {
            limit: Some(args.limit),
            all: args.all,
        };

        let entries = HistoryService::get_history(&table, &config)?;

        // 5. Output
        Self::output(&entries, &args.output)
    }

    /// Output history in the requested format
    fn output(entries: &[HistoryEntry], format: &str) -> Result<()> {
        match format {
            "json" => Self::output_json(entries),
            _ => Self::output_table(entries),
        }
    }

    /// Output history as a table
    fn output_table(entries: &[HistoryEntry]) -> Result<()> {
        if entries.is_empty() {
            println!("No history entries found.");
            return Ok(());
        }

        for entry in entries {
            let marker = if entry.is_current {
                "●".yellow().bold()
            } else {
                "○".dimmed()
            };

            let timestamp = entry.timestamp.format("%Y-%m-%d %H:%M:%S");

            let op = match entry.operation.as_str() {
                "Append" => "append".green(),
                "Overwrite" => "overwrite".yellow(),
                "Delete" => "delete".red(),
                "Replace" => "replace".cyan(),
                other => other.normal(),
            };

            println!(
                "{} {} - {} ({})",
                marker,
                entry.version.to_string().cyan().bold(),
                op,
                timestamp.to_string().dimmed()
            );

            // Details line
            let details = HistoryService::format_details(entry);
            if !details.is_empty() {
                println!("  {}", details.join(", ").dimmed());
            }
            println!();
        }

        println!("{} snapshots", entries.len());

        Ok(())
    }

    /// Output history as JSON
    fn output_json(entries: &[HistoryEntry]) -> Result<()> {
        let json_entries: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "snapshot_id": e.version,
                    "timestamp": e.timestamp.to_rfc3339(),
                    "operation": e.operation,
                    "details": e.details,
                    "is_current": e.is_current,
                })
            })
            .collect();

        print_json(&json_entries)?;

        Ok(())
    }
}
