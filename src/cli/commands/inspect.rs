//! Inspect command implementation
//!
//! Shows detailed information about table structure, metadata, and statistics.
//! Uses IcebergTableInspector from core::operations for the actual inspection logic.

use colored::Colorize;

use super::common::{print_json, resolve_table_from_context};
use crate::cli::output::format_timestamp_ms;
use crate::cli::output::{Box, BoxItem, BoxLayout, BoxRenderer, BoxSection};
use crate::cli::parser::{CatalogContext, InspectArgs};
use crate::core::operations::inspect::{
    IcebergInspectOptions, IcebergInspectResult, IcebergTableInspector,
};
use crate::core::{format_bytes, format_number};
use crate::error::Result;
use crate::utils::with_resource_limits;

/// Handler for inspect command
pub struct InspectCommand;

impl InspectCommand {
    /// Execute inspect command
    pub async fn execute(args: InspectArgs, ctx: &CatalogContext) -> Result<()> {
        use super::constants::MEMORY_HEAVY_OPS;
        with_resource_limits(MEMORY_HEAVY_OPS, Self::inspect_inner(args, ctx)).await
    }

    async fn inspect_inner(args: InspectArgs, ctx: &CatalogContext) -> Result<()> {
        // Resolve table - get catalog table directly when using catalog
        let resolution = resolve_table_from_context(ctx).await?;

        // Get table using factory method - handles catalog vs path context automatically
        let table = resolution.to_table().await?;

        // Build inspection options from CLI args
        let options = IcebergInspectOptions::from_cli(args.verbose);

        // Execute inspection using the core operation
        let result = IcebergTableInspector::inspect(&table, &options)?;

        // Format and display result based on output format
        if args.output == "json" {
            print_json(&result)?;
        } else {
            let output = Self::format_result(&result, &options);
            println!("{}", output);
        }

        Ok(())
    }

    /// Format inspection result for display
    fn format_result(result: &IcebergInspectResult, options: &IcebergInspectOptions) -> String {
        let layout = BoxLayout::new(100);
        let renderer = BoxRenderer::new(layout);
        let mut container = Box::titled("Iceberg Table Inspection");

        // ═══════════════════════════════════════════════════════════════════════
        // TABLE INFORMATION
        // ═══════════════════════════════════════════════════════════════════════
        let key_width = 18;
        let mut table_info = vec![
            BoxItem::kv_aligned(
                "Format",
                format!("Iceberg v{}", result.format_version),
                key_width,
            ),
            BoxItem::kv_aligned("Location", &result.location, key_width),
            BoxItem::kv_aligned("Table UUID", &result.table_uuid, key_width),
            BoxItem::kv_aligned(
                "Current Snapshot",
                result
                    .current_snapshot_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "None".to_string()),
                key_width,
            ),
            BoxItem::kv_aligned(
                "Snapshot Count",
                result.snapshot_count.to_string(),
                key_width,
            ),
            BoxItem::kv_aligned(
                "Last Updated",
                format_timestamp_ms(result.last_updated_ms),
                key_width,
            ),
        ];

        // Add sequence number in verbose mode
        if options.verbose {
            table_info.push(BoxItem::kv_aligned(
                "Last Sequence",
                result.last_sequence_number.to_string(),
                key_width,
            ));
        }

        container = container.section(BoxSection::titled("Table Information").items(table_info));

        // ═══════════════════════════════════════════════════════════════════════
        // CURRENT STATE (records, files, delete files, size)
        // Always show these fields even if some values are missing
        // ═══════════════════════════════════════════════════════════════════════
        let state = &result.current_state;
        let mut state_items = Vec::new();

        // Total Records
        state_items.push(BoxItem::kv_aligned(
            "Total Records",
            state
                .total_records
                .map(format_number)
                .unwrap_or_else(|| "-".to_string()),
            16,
        ));

        // Data Files
        state_items.push(BoxItem::kv_aligned(
            "Data Files",
            state
                .total_data_files
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string()),
            16,
        ));

        // Delete Files - always show, highlight if > 0
        let delete_files_str = match state.total_delete_files {
            Some(v) if v > 0 => format!("{} {}", v, "(compaction recommended)".yellow()),
            Some(v) => v.to_string(),
            None => "0".to_string(),
        };
        state_items.push(BoxItem::kv_aligned("Delete Files", delete_files_str, 16));

        // Total Size
        state_items.push(BoxItem::kv_aligned(
            "Total Size",
            state
                .total_files_size
                .map(|v| format_bytes(v as u64))
                .unwrap_or_else(|| "-".to_string()),
            16,
        ));

        container = container.section(BoxSection::titled("Current State").items(state_items));

        // ═══════════════════════════════════════════════════════════════════════
        // SCHEMA
        // ═══════════════════════════════════════════════════════════════════════
        let mut schema_items = vec![
            BoxItem::kv_aligned("Schema ID", result.schema_id.to_string(), 12),
            BoxItem::kv_aligned("Columns", result.fields.len().to_string(), 12),
        ];

        // Show identifier fields if any
        if !result.identifier_field_ids.is_empty() {
            let id_names: Vec<&str> = result
                .fields
                .iter()
                .filter(|f| f.is_identifier)
                .map(|f| f.name.as_str())
                .collect();
            schema_items.push(BoxItem::kv_aligned("Identifier", id_names.join(", "), 12));
        }

        // Add subsection header for fields
        schema_items.push(BoxItem::Empty);
        schema_items.push(BoxItem::text(format!(
            "{} Fields {}",
            "──".white(),
            "─".repeat(80).white()
        )));

        // Calculate max field name width for alignment
        let max_name_width = result
            .fields
            .iter()
            .map(|f| f.name.len())
            .max()
            .unwrap_or(0);

        for field in &result.fields {
            let nullable_str = if field.required { "" } else { " (nullable)" };
            let id_marker = if field.is_identifier { " [ID]" } else { "" };

            if options.verbose {
                // Verbose: show field ID after name
                let name_with_id = format!("{} ({})", field.name, field.field_id);
                let max_verbose_width = max_name_width + 6; // account for " (XX)"
                schema_items.push(BoxItem::text(format!(
                    "  {:<width$}  {}{}{}",
                    name_with_id.white().bold(),
                    field.field_type.cyan(),
                    nullable_str.dimmed(),
                    id_marker.yellow(),
                    width = max_verbose_width
                )));

                // Show doc string if present
                if let Some(ref doc) = field.doc {
                    schema_items.push(BoxItem::text(format!(
                        "    {} {}",
                        "doc:".dimmed(),
                        doc.dimmed()
                    )));
                }
            } else {
                // Normal mode: simpler format
                schema_items.push(BoxItem::text(format!(
                    "  {:<width$}  {}{}{}",
                    field.name.white().bold(),
                    field.field_type.cyan(),
                    nullable_str.dimmed(),
                    id_marker.yellow(),
                    width = max_name_width
                )));
            }
        }

        container = container.section(BoxSection::titled("Schema").items(schema_items));

        // ═══════════════════════════════════════════════════════════════════════
        // PARTITION & SORT
        // ═══════════════════════════════════════════════════════════════════════
        let mut partition_items = Vec::new();

        if result.partition_fields.is_empty() {
            partition_items.push(BoxItem::kv_aligned("Partitioning", "Unpartitioned", 14));
        } else {
            partition_items.push(BoxItem::kv_aligned(
                "Partition Spec",
                format!("ID {}", result.partition_spec_id),
                14,
            ));
            for (i, field) in result.partition_fields.iter().enumerate() {
                let is_last = i == result.partition_fields.len() - 1;
                let prefix = if is_last { "└" } else { "├" };
                partition_items.push(BoxItem::text(format!(
                    "{} {}: {}",
                    prefix.bright_black(),
                    field.name.white().bold(),
                    field.transform.cyan()
                )));
            }
        }

        if result.sort_fields.is_empty() {
            partition_items.push(BoxItem::kv_aligned("Sort Order", "Unsorted", 14));
        } else {
            partition_items.push(BoxItem::kv_aligned(
                "Sort Order",
                format!("ID {}", result.sort_order_id),
                14,
            ));
            for (i, sf) in result.sort_fields.iter().enumerate() {
                let is_last = i == result.sort_fields.len() - 1;
                let prefix = if is_last { "└" } else { "├" };
                partition_items.push(BoxItem::text(format!(
                    "{} field {}: {} {}",
                    prefix.bright_black(),
                    sf.source_id,
                    sf.direction.cyan(),
                    sf.null_order.dimmed()
                )));
            }
        }

        container =
            container.section(BoxSection::titled("Partition & Sort").items(partition_items));

        // ═══════════════════════════════════════════════════════════════════════
        // PROPERTIES
        // Normal mode: key properties only
        // Verbose mode: all properties sorted
        // ═══════════════════════════════════════════════════════════════════════
        if !result.properties.is_empty() {
            let mut prop_items = Vec::new();

            if options.verbose {
                // Show ALL properties sorted alphabetically
                let mut sorted_props: Vec<_> = result.properties.iter().collect();
                sorted_props.sort_by_key(|(k, _)| *k);
                for (key, value) in sorted_props {
                    prop_items.push(BoxItem::text(format!("{} = {}", key.cyan(), value)));
                }
            } else {
                // Show only the most important properties
                let key_properties = [
                    "write.format.default",
                    "write.parquet.compression-codec",
                    "write.target-file-size-bytes",
                    "write.delete.mode",
                    "write.update.mode",
                    "write.merge.mode",
                ];

                for key in &key_properties {
                    if let Some(value) = result.properties.get(*key) {
                        prop_items.push(BoxItem::text(format!("{} = {}", key.cyan(), value)));
                    }
                }
            }

            if !prop_items.is_empty() {
                container = container.section(BoxSection::titled("Properties").items(prop_items));
            }
        }

        // ═══════════════════════════════════════════════════════════════════════
        // REFS (branches and tags) - verbose mode only
        // ═══════════════════════════════════════════════════════════════════════
        if options.verbose && !result.refs.is_empty() {
            let mut ref_items = Vec::new();
            for r in &result.refs {
                let type_str = if r.ref_type == "branch" {
                    "branch".green()
                } else {
                    "tag".blue()
                };
                ref_items.push(BoxItem::text(format!(
                    "{} ({}) → snapshot {}",
                    r.name.white().bold(),
                    type_str,
                    r.snapshot_id
                )));
            }
            container = container.section(BoxSection::titled("Refs").items(ref_items));
        }

        // ═══════════════════════════════════════════════════════════════════════
        // METADATA (verbose mode only)
        // ═══════════════════════════════════════════════════════════════════════
        if options.verbose {
            let mut meta_items = vec![
                BoxItem::kv_aligned("Schema Versions", result.schemas_count.to_string(), 20),
                BoxItem::kv_aligned(
                    "Partition Specs",
                    result.partition_specs_count.to_string(),
                    20,
                ),
                BoxItem::kv_aligned("Sort Orders", result.sort_orders_count.to_string(), 20),
            ];

            if !result.metadata_log.is_empty() {
                meta_items.push(BoxItem::kv_aligned(
                    "Metadata Files",
                    result.metadata_log.len().to_string(),
                    20,
                ));
                // Show current metadata file location
                if let Some(latest) = result.metadata_log.last() {
                    meta_items.push(BoxItem::kv_aligned(
                        "Current Metadata",
                        &latest.metadata_file,
                        20,
                    ));
                }
            }

            container = container.section(BoxSection::titled("Metadata").items(meta_items));
        }

        renderer.render(container)
    }
}
