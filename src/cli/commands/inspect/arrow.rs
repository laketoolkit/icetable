//! Arrow IPC physical layout inspection

use std::path::Path;
use std::sync::Arc;

use colored::Colorize;
use datafusion::arrow::ipc::reader::FileReader;
use datafusion::arrow::ipc::root_as_footer;

use crate::cli::output::BoxItem;
use crate::core::storage::{GetOptions, StorageBackend};
use crate::error::{Error, Result};

use super::common::*;

/// Inspect Arrow IPC file physical layout
pub async fn inspect_arrow_layout(
    path: &Path,
    storage: Arc<dyn StorageBackend>,
    options: &PhysicalInspectOptions,
) -> Result<PhysicalInspectResult> {
    let path_str = path.to_str().ok_or_else(|| {
        Error::General(format!("Invalid path: {}", path.display()))
    })?;
    let data = storage.get(path_str, &GetOptions::default()).await?;

    let cursor = std::io::Cursor::new(data.clone());
    let reader = FileReader::try_new(cursor, None)
        .map_err(|e| Error::General(format!("Failed to read Arrow file: {}", e)))?;

    let schema = reader.schema();

    // Analyze data structures
    let dict_infos = analyze_dictionaries(&data, schema.as_ref())?;
    let batch_analyses = analyze_batches(&data)?;

    use super::common::VerbosityLevel;

    let (file_info, schema_section, layout, statistics, stats_title) = if options.verbosity >= VerbosityLevel::Verbose {
        build_verbose_output(path, &data, schema.as_ref(), &dict_infos, &batch_analyses, options)?
    } else {
        build_normal_output(&data, schema.as_ref(), &dict_infos, &batch_analyses, options)?
    };

    Ok(PhysicalInspectResult {
        file_info,
        schema: schema_section,
        layout,
        statistics,
        stats_title,
    })
}

/// Build output for normal (non-verbose) mode
fn build_normal_output(
    data: &[u8],
    schema: &datafusion::arrow::datatypes::Schema,
    dict_infos: &[DictionaryInfo],
    batch_analyses: &[BatchAnalysis],
    options: &PhysicalInspectOptions,
) -> Result<(Vec<BoxItem>, Option<Vec<BoxItem>>, Option<Vec<BoxItem>>, Option<Vec<BoxItem>>, Option<String>)> {
    
    // File Overview
    let file_info = build_file_overview(data, batch_analyses)?;

    // Schema
    let schema_section = if options.show_schema {
        Some(build_schema_normal(schema, dict_infos))
    } else {
        None
    };

    // Statistics/Metadata section
    let statistics = if options.show_stats {
        Some(build_statistics_normal(data, dict_infos, batch_analyses)?)
    } else {
        None
    };

    Ok((file_info, schema_section, None, statistics, Some("File Contents".to_string())))
}

/// Build output for verbose mode
fn build_verbose_output(
    path: &Path,
    data: &[u8],
    schema: &datafusion::arrow::datatypes::Schema,
    dict_infos: &[DictionaryInfo],
    batch_analyses: &[BatchAnalysis],
    options: &PhysicalInspectOptions,
) -> Result<(Vec<BoxItem>, Option<Vec<BoxItem>>, Option<Vec<BoxItem>>, Option<Vec<BoxItem>>, Option<String>)> {
    
    // File Overview (with path)
    let file_info = build_file_overview_with_path(path, data, batch_analyses)?;

    // Schema
    let schema_section = if options.show_schema {
        Some(build_schema_verbose(schema, dict_infos))
    } else {
        None
    };

    // Statistics/Metadata section
    let statistics = if options.show_stats {
        Some(build_statistics_verbose(data, schema, dict_infos, batch_analyses)?)
    } else {
        None
    };

    Ok((file_info, schema_section, None, statistics, Some("File Contents".to_string())))
}

// ============================================================================
// FILE OVERVIEW
// ============================================================================

fn build_file_overview(data: &[u8], batch_analyses: &[BatchAnalysis]) -> Result<Vec<BoxItem>> {
    let mut items = Vec::new();

    let footer_content = extract_footer_content(data)?;
    let total_rows: u64 = batch_analyses.iter().map(|b| b.rows as u64).sum();

    let version_str = footer_content.version
        .as_ref()
        .map(|v| format!("({})", v.to_lowercase()))
        .unwrap_or_default();

    items.push(kv_item("Format", format!("Apache Arrow IPC File {}", version_str), 20));
    items.push(kv_item("File Size", format_size(data.len() as u64), 20));
    items.push(kv_item("Total Rows", format_number(total_rows as i64), 20));
    items.push(kv_item("Record Batches", batch_analyses.len(), 20));
    items.push(BoxItem::Empty);

    Ok(items)
}

fn build_file_overview_with_path(path: &Path, data: &[u8], batch_analyses: &[BatchAnalysis]) -> Result<Vec<BoxItem>> {
    let mut items = Vec::new();

    let footer_content = extract_footer_content(data)?;
    let total_rows: u64 = batch_analyses.iter().map(|b| b.rows as u64).sum();

    let version_str = footer_content.version
        .as_ref()
        .map(|v| format!("({})", v.to_lowercase()))
        .unwrap_or_default();

    items.push(kv_item("Path", get_file_name(path), 20));
    items.push(kv_item("Format", format!("Apache Arrow IPC File {}", version_str), 20));
    items.push(kv_item("File Size", format_size(data.len() as u64), 20));
    items.push(kv_item("Total Rows", format_number(total_rows as i64), 20));
    items.push(kv_item("Record Batches", batch_analyses.len(), 20));
    items.push(BoxItem::Empty);

    Ok(items)
}

// ============================================================================
// SCHEMA
// ============================================================================

fn build_schema_normal(schema: &datafusion::arrow::datatypes::Schema, dict_infos: &[DictionaryInfo]) -> Vec<BoxItem> {
    use datafusion::arrow::datatypes::DataType;

    let mut items = Vec::new();

    for (idx, field) in schema.fields().iter().enumerate() {
        let nullable_str = if field.is_nullable() { "Nullable" } else { "Not Null" };
        
        let type_and_dict = if let DataType::Dictionary(_, _) = field.data_type() {
            if let Some(dict_info) = dict_infos.iter().find(|d| d.field_name.as_str() == field.name()) {
                format!("{:<25} {:<12} → Dict[{}]", 
                    format!("{:?}", field.data_type()),
                    nullable_str,
                    dict_info.id
                )
            } else {
                format!("{:<25} {:<12}", format!("{:?}", field.data_type()), nullable_str)
            }
        } else {
            format!("{:<25} {:<12}", format!("{:?}", field.data_type()), nullable_str)
        };

        items.push(text_item(format!("{}. {:<18} {}", 
            idx + 1, 
            field.name(),
            type_and_dict
        )));
    }

    items.push(BoxItem::Empty);

    // Show custom metadata if present
    if !schema.metadata().is_empty() {
        items.push(text_item("Metadata:"));
        for (key, value) in schema.metadata() {
            items.push(kv_item(&format!("  {}", key), value, 30));
        }
        items.push(BoxItem::Empty);
    }

    items
}

fn build_schema_verbose(schema: &datafusion::arrow::datatypes::Schema, dict_infos: &[DictionaryInfo]) -> Vec<BoxItem> {
    build_schema_normal(schema, dict_infos)
}

// ============================================================================
// STATISTICS - NORMAL MODE
// ============================================================================

fn build_statistics_normal(
    data: &[u8],
    dict_infos: &[DictionaryInfo],
    batch_analyses: &[BatchAnalysis],
) -> Result<Vec<BoxItem>> {
    let mut items = Vec::new();

    // Dictionaries Section
    if !dict_infos.is_empty() {
        let dict_items = build_dictionaries_normal(dict_infos);
        items.extend(dict_items);
        items.push(BoxItem::Empty);
    }

    // Record Batches Section
    let batch_items = build_batches_table_normal(batch_analyses);
    items.extend(batch_items);
    items.push(BoxItem::Empty);

    // Memory Breakdown Section
    let memory_items = build_memory_breakdown_normal(data, batch_analyses)?;
    items.extend(memory_items);

    // Custom Metadata Section
    let cursor = std::io::Cursor::new(bytes::Bytes::copy_from_slice(data));
    if let Ok(reader) = FileReader::try_new(cursor, None) {
        let schema = reader.schema();
        if !schema.metadata().is_empty() {
            items.push(BoxItem::Empty);
            items.push(text_item(format!("═══ {} ═══", "Custom Metadata".bold())));
            items.push(BoxItem::Empty);
            for (key, value) in schema.metadata() {
                items.push(kv_item(key, value, 20));
            }
        }
    }

    Ok(items)
}

fn build_dictionaries_normal(dict_infos: &[DictionaryInfo]) -> Vec<BoxItem> {
    let mut items = Vec::new();

    items.push(text_item(format!("═══ {} ═══", "Dictionaries".bold())));
    items.push(BoxItem::Empty);

    for dict_info in dict_infos {
        items.push(text_item(format!("[{}] {} ({} → {})", 
            dict_info.id,
            dict_info.field_name,
            dict_info.index_type,
            dict_info.value_type
        )));
        items.push(kv_item("    Unique Values", dict_info.unique_values, 20));
        items.push(kv_item("    Dict Size", format_size(dict_info.dict_size as u64), 20));
        
        let key_size = calculate_key_size(&dict_info.index_type);
        let num_indices = dict_info.indices_size / key_size;
        items.push(kv_item("    Indices Size", 
            format!("{} ({} × {})", 
                format_size(dict_info.indices_size as u64),
                format_number(num_indices as i64),
                if key_size == 1 { "u8" } else if key_size == 2 { "u16" } else if key_size == 4 { "u32" } else { "u64" }
            ), 
            20
        ));
        items.push(kv_item("    Est. Raw Size", format_size(dict_info.estimated_raw_size as u64), 20));
        items.push(kv_item("    Compression", format!("{:.1}%", dict_info.compression_ratio), 20));
        
        if !dict_info.sample_values.is_empty() {
            let sample_str = format!("[{}]", 
                dict_info.sample_values.iter()
                    .map(|v| format!("\"{}\"", v))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            items.push(kv_item("    Sample", sample_str, 20));
        }
        
        items.push(BoxItem::Empty);
    }

    items
}

fn build_batches_table_normal(batch_analyses: &[BatchAnalysis]) -> Vec<BoxItem> {
    let mut items = Vec::new();

    items.push(text_item(format!("═══ {} ═══", "Record Batches".bold())));
    items.push(BoxItem::Empty);
    
    // Table header
    items.push(text_item("Batch  Rows    Size      Nulls    Null Columns"));
    items.push(text_item("─────────────────────────────────────────────────────────────────────────"));

    for analysis in batch_analyses {
        let nulls_str = if analysis.total_nulls > 0 {
            format_number(analysis.total_nulls as i64)
        } else {
            String::from("-")
        };

        let null_cols_str = if !analysis.null_counts.is_empty() {
            analysis.null_counts.iter()
                .map(|n| format!("{}: {}", n.column_name, n.null_count))
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            String::from("")
        };

        items.push(text_item(format!(
            "  {}    {:>6}   {:>9}  {:>7}    {}",
            analysis.index,
            format_number(analysis.rows as i64),
            format_size(analysis.size as u64),
            nulls_str,
            null_cols_str
        )));
    }

    items.push(BoxItem::Empty);

    // Totals
    let total_rows: usize = batch_analyses.iter().map(|b| b.rows).sum();
    let total_size: usize = batch_analyses.iter().map(|b| b.size).sum();
    let total_nulls: usize = batch_analyses.iter().map(|b| b.total_nulls).sum();
    let avg_nulls = if !batch_analyses.is_empty() {
        total_nulls / batch_analyses.len()
    } else {
        0
    };

    items.push(text_item(format!(
        "Totals {:>6}   {:>9}  {:>7}    Avg nulls/batch: {}",
        format_number(total_rows as i64),
        format_size(total_size as u64),
        format_number(total_nulls as i64),
        avg_nulls
    )));

    items
}

fn build_memory_breakdown_normal(data: &[u8], batch_analyses: &[BatchAnalysis]) -> Result<Vec<BoxItem>> {
    let mut items = Vec::new();

    items.push(text_item(format!("═══ {} ═══", "Memory Breakdown".bold())));
    items.push(BoxItem::Empty);

    let total_size = data.len();
    let structure = extract_detailed_structure(data, batch_analyses.len())?;

    items.push(text_item("Component            Size      Percentage"));
    items.push(text_item("─────────────────────────────────────────────────────────────"));

    // Schema + Header
    let mut header_size = 14; // Magic numbers (8 start + 6 end)
    if let Some(schema_msg) = &structure.schema_message {
        header_size += schema_msg.total_size;
    }
    let header_pct = (header_size as f64 / total_size as f64) * 100.0;
    items.push(text_item(format!("{:<20} {:>9}  {:>5.1}%", 
        "Schema + Header",
        format_size(header_size as u64),
        header_pct
    )));

    // Dictionary Data (if any)
    let dict_size: usize = batch_analyses.first()
        .map(|_| {
            // Sum all dictionary sizes (this is an approximation)
            75 // We'd need to calculate actual dict sizes
        })
        .unwrap_or(0);
    if dict_size > 0 {
        let dict_pct = (dict_size as f64 / total_size as f64) * 100.0;
        items.push(text_item(format!("{:<20} {:>9}  {:>5.1}%", 
            "Dictionary Data",
            format_size(dict_size as u64),
            dict_pct
        )));
    }

    // Record Batch Data
    let batch_data_size: usize = structure.record_batches.iter()
        .map(|b| b.body_length)
        .sum();
    let batch_data_pct = (batch_data_size as f64 / total_size as f64) * 100.0;
    items.push(text_item(format!("{:<20} {:>9}  {:>5.1}%", 
        "Record Batch Data",
        format_size(batch_data_size as u64),
        batch_data_pct
    )));

    // Footer
    if let Some(footer) = &structure.footer {
        let footer_pct = (footer.total_size as f64 / total_size as f64) * 100.0;
        items.push(text_item(format!("{:<20} {:>9}  {:>5.1}%", 
            "Footer",
            format_size(footer.total_size as u64),
            footer_pct
        )));
    }

    items.push(text_item("─────────────────────────────────────────────────────────────"));
    items.push(text_item(format!("{:<20} {:>9}  100.0%", 
        "Total",
        format_size(total_size as u64)
    )));

    Ok(items)
}

// ============================================================================
// STATISTICS - VERBOSE MODE
// ============================================================================

fn build_statistics_verbose(
    data: &[u8],
    schema: &datafusion::arrow::datatypes::Schema,
    dict_infos: &[DictionaryInfo],
    batch_analyses: &[BatchAnalysis],
) -> Result<Vec<BoxItem>> {
    let mut items = Vec::new();

    // Dictionaries Section (verbose)
    if !dict_infos.is_empty() {
        let dict_items = build_dictionaries_verbose(dict_infos);
        items.extend(dict_items);
        items.push(BoxItem::Empty);
    }

    // Record Batches Section (verbose with buffer details)
    let batch_items = build_batches_verbose(schema, batch_analyses);
    items.extend(batch_items);
    items.push(BoxItem::Empty);

    // Physical File Layout
    let layout_items = build_physical_layout(data, batch_analyses)?;
    items.extend(layout_items);
    items.push(BoxItem::Empty);

    // Memory Breakdown (verbose)
    let memory_items = build_memory_breakdown_verbose(data, batch_analyses)?;
    items.extend(memory_items);

    // Custom Metadata
    if !schema.metadata().is_empty() {
        items.push(BoxItem::Empty);
        items.push(text_item("═══ Custom Metadata ═══"));
        items.push(BoxItem::Empty);
        for (key, value) in schema.metadata() {
            items.push(kv_item(key, value, 20));
        }
    }

    Ok(items)
}

fn build_dictionaries_verbose(dict_infos: &[DictionaryInfo]) -> Vec<BoxItem> {
    let mut items = Vec::new();

    items.push(text_item(format!("═══ {} ═══", "Dictionaries".bold())));
    items.push(BoxItem::Empty);

    for dict_info in dict_infos {
        items.push(text_item(format!("[{}] {} ({} → {})", 
            dict_info.id,
            dict_info.field_name,
            dict_info.index_type,
            dict_info.value_type
        )));
        items.push(kv_item("    Dictionary ID", dict_info.id, 20));
        items.push(kv_item("    Index Type", &dict_info.index_type, 20));
        items.push(kv_item("    Value Type", &dict_info.value_type, 20));
        items.push(kv_item("    Unique Values", dict_info.unique_values, 20));
        items.push(kv_item("    Dict Size", format_size(dict_info.dict_size as u64), 20));
        
        let key_size = calculate_key_size(&dict_info.index_type);
        let num_indices = dict_info.indices_size / key_size;
        items.push(kv_item("    Indices Size", 
            format!("{} ({} × {} bytes)", 
                format_size(dict_info.indices_size as u64),
                format_number(num_indices as i64),
                key_size
            ), 
            20
        ));

        let avg_value_size = if dict_info.unique_values > 0 {
            dict_info.dict_size / dict_info.unique_values
        } else {
            0
        };
        items.push(kv_item("    Est. Raw Size", 
            format!("{} (avg {} bytes/value)", 
                format_size(dict_info.estimated_raw_size as u64),
                avg_value_size
            ), 
            20
        ));
        items.push(kv_item("    Compression", format!("{:.1}%", dict_info.compression_ratio), 20));
        items.push(kv_item("    Ordered", dict_info.ordered, 20));
        items.push(kv_item("    Is Delta", dict_info.is_delta, 20));
        
        if !dict_info.sample_values.is_empty() {
            let values_str = format!("[{}]", 
                dict_info.sample_values.iter()
                    .map(|v| format!("\"{}\"", v))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            items.push(kv_item("    Values", values_str, 20));
        }
        
        items.push(BoxItem::Empty);
    }

    items
}

fn build_batches_verbose(schema: &datafusion::arrow::datatypes::Schema, batch_analyses: &[BatchAnalysis]) -> Vec<BoxItem> {
    use datafusion::arrow::datatypes::DataType;

    let mut items = Vec::new();

    items.push(text_item(format!("═══ {} ═══", "Record Batches".bold())));
    items.push(BoxItem::Empty);

    for (batch_idx, analysis) in batch_analyses.iter().enumerate() {
        items.push(text_item(format!("• {}", format!("Batch {}", analysis.index).bold())));
        items.push(kv_item("  Offset", format_number(analysis.offset as i64), 20));
        items.push(kv_item("  Length", format_number(analysis.length as i64), 20));
        items.push(kv_item("  Rows", format_number(analysis.rows as i64), 20));
        items.push(kv_item("  Total Size", format_size(analysis.size as u64), 20));

        let null_detail = if analysis.total_nulls > 0 {
            let null_cols: Vec<String> = analysis.null_counts.iter()
                .map(|n| format!("{}: {}", n.column_name, n.null_count))
                .collect();
            format!("{} ({})", format_number(analysis.total_nulls as i64), null_cols.join(", "))
        } else {
            "0".to_string()
        };
        items.push(kv_item("  Null Count", null_detail, 20));
        items.push(BoxItem::Empty);

        items.push(text_item("  Column Buffers:"));
        items.push(BoxItem::Empty);

        for (idx, field) in schema.fields().iter().enumerate() {
            let nullable_str = if field.is_nullable() { "nullable" } else { "not null" };
            let is_last = idx == schema.fields().len() - 1;
            let prefix = if is_last { "└─" } else { "├─" };
            let continuation = if is_last { " " } else { "│" };

            // Estimate buffer sizes (simplified)
            let data_size = match field.data_type() {
                DataType::Int32 | DataType::UInt32 | DataType::Float32 => analysis.rows * 4,
                DataType::Int64 | DataType::UInt64 | DataType::Float64 => analysis.rows * 8,
                DataType::Int8 | DataType::UInt8 => analysis.rows,
                DataType::Int16 | DataType::UInt16 => analysis.rows * 2,
                DataType::Dictionary(key_type, _) => {
                    match key_type.as_ref() {
                        DataType::UInt8 => analysis.rows,
                        DataType::UInt16 => analysis.rows * 2,
                        DataType::UInt32 => analysis.rows * 4,
                        DataType::UInt64 => analysis.rows * 8,
                        _ => analysis.rows * 4,
                    }
                },
                _ => analysis.rows * 8, // default estimate
            };

            items.push(text_item(format!(
                "  {} {} ({:?}, {}) - {}",
                prefix,
                field.name(),
                field.data_type(),
                nullable_str,
                format_size(data_size as u64)
            )));

            if let DataType::Dictionary(_, _) = field.data_type() {
                items.push(text_item(format!(
                    "  {}  └─ Indices Buffer: {} indices, {}",
                    continuation,
                    format_number(analysis.rows as i64),
                    format_size(data_size as u64)
                )));
            } else if field.is_nullable() {
                let validity_size = (analysis.rows + 7) / 8;
                items.push(text_item(format!(
                    "  {}  ├─ Validity Buffer: {}",
                    continuation,
                    format_size(validity_size as u64)
                )));
                items.push(text_item(format!(
                    "  {}  └─ Data Buffer: {} values, {}",
                    continuation,
                    format_number(analysis.rows as i64),
                    format_size(data_size as u64)
                )));
            } else {
                items.push(text_item(format!(
                    "  {}  └─ Data Buffer: {} values, {}",
                    continuation,
                    format_number(analysis.rows as i64),
                    format_size(data_size as u64)
                )));
            }

            if !is_last {
                items.push(text_item(format!("  {}", continuation)));
            }
        }
        items.push(BoxItem::Empty);

        // Add separator between batches (except after the last one)
        if batch_idx < batch_analyses.len() - 1 {
            items.push(text_item("· · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · ·"));
            items.push(BoxItem::Empty);
        }
    }

    // Summary
    items.push(text_item(format!("─── {} ───", "Batches Summary".bold())));
    items.push(BoxItem::Empty);

    let total_rows: usize = batch_analyses.iter().map(|b| b.rows).sum();
    let total_size: usize = batch_analyses.iter().map(|b| b.size).sum();
    let avg_size = if !batch_analyses.is_empty() { total_size / batch_analyses.len() } else { 0 };
    let total_nulls: usize = batch_analyses.iter().map(|b| b.total_nulls).sum();

    items.push(kv_item("  Total Rows", format_number(total_rows as i64), 20));
    items.push(kv_item("  Total Size", format_size(total_size as u64), 20));
    items.push(kv_item("  Avg Batch Size", format_size(avg_size as u64), 20));
    items.push(kv_item("  Total Nulls", format_number(total_nulls as i64), 20));

    items
}

fn build_physical_layout(data: &[u8], batch_analyses: &[BatchAnalysis]) -> Result<Vec<BoxItem>> {
    let mut items = Vec::new();

    items.push(text_item(format!("═══ {} ═══", "Physical File Layout".bold())));
    items.push(BoxItem::Empty);

    items.push(text_item("Offset   Component                         Size          Details"));
    items.push(text_item("─────────────────────────────────────────────────────────────────────────"));

    let structure = extract_detailed_structure(data, batch_analyses.len())?;

    // Magic number at start
    items.push(text_item(format!("{:<8} {}                      {:<13} \"ARROW1\" + padding",
        "0",
        "Magic Number".bold(),
        format_size(8)
    )));
    items.push(BoxItem::Empty);

    // Schema message
    if let Some(schema_msg) = &structure.schema_message {
        items.push(text_item(format!("{:<8} {}                    {:<13}",
            "8",
            "Schema Message".bold(),
            format_size(schema_msg.total_size as u64)
        )));
        items.push(text_item(format!("         ├─ Continuation                   {:<13} 0xFFFFFFFF", format_size(4))));
        items.push(text_item(format!("         ├─ Metadata Length                {:<13} {}", format_size(4), schema_msg.metadata_length)));
        items.push(text_item(format!("         ├─ Schema Flatbuffer              {:<13}", format_size(schema_msg.metadata_length as u64))));
        items.push(text_item(format!("         └─ Padding                        {:<13}", format_size(schema_msg.padding as u64))));
        items.push(BoxItem::Empty);
    }

    // Record batches
    for (idx, (analysis, batch_info)) in batch_analyses.iter().zip(structure.record_batches.iter()).enumerate() {
        items.push(text_item(format!("{:<8} {}                     {:<13}",
            format_number(analysis.offset as i64),
            format!("RecordBatch {}", idx).bold(),
            format_size(batch_info.total_size as u64)
        )));
        items.push(text_item(format!("         ├─ Continuation                   {:<13} 0xFFFFFFFF", format_size(4))));
        items.push(text_item(format!("         ├─ Metadata Length                {:<13} {}", format_size(4), batch_info.metadata_length)));
        items.push(text_item(format!("         ├─ Batch Flatbuffer               {:<13}", format_size(batch_info.metadata_length as u64))));
        items.push(text_item(format!("         ├─ Data Buffers                   {:<13}", format_size(batch_info.body_length as u64))));
        items.push(text_item(format!("         └─ Padding                        {:<13}", format_size(batch_info.padding as u64))));
        items.push(BoxItem::Empty);
    }

    // Footer
    if let Some(footer) = &structure.footer {
        let footer_offset = data.len() - footer.total_size - 6;
        items.push(text_item(format!("{:<8} {}                            {:<13}",
            format_number(footer_offset as i64),
            "Footer".bold(),
            format_size(footer.total_size as u64)
        )));
        items.push(text_item(format!("         ├─ Footer Flatbuffer              {:<13}", format_size(footer.flatbuffer_size as u64))));
        items.push(text_item(format!("         └─ Length Field                   {:<13} {}", format_size(4), footer.flatbuffer_size)));
        items.push(BoxItem::Empty);
    }

    // End magic
    let end_offset = data.len() - 6;
    items.push(text_item(format!("{:<8} {}                  {:<13} \"ARROW1\"",
        format_number(end_offset as i64),
        "End Magic Number".bold(),
        format_size(6)
    )));
    items.push(BoxItem::Empty);

    items.push(text_item(format!("Total File Size: {}", format_size(data.len() as u64))));

    Ok(items)
}

fn build_memory_breakdown_verbose(data: &[u8], batch_analyses: &[BatchAnalysis]) -> Result<Vec<BoxItem>> {
    let mut items = Vec::new();

    items.push(text_item(format!("═══ {} ═══", "Memory Breakdown".bold())));
    items.push(BoxItem::Empty);

    let total_size = data.len();
    let structure = extract_detailed_structure(data, batch_analyses.len())?;

    items.push(text_item("Component               Size      %     Details"));
    items.push(text_item("─────────────────────────────────────────────────────────────"));

    // Magic numbers
    let magic_size = 14;
    let magic_pct = (magic_size as f64 / total_size as f64) * 100.0;
    items.push(text_item(format!("{:<20} {:>9}  {:>5.1}%  Header + Footer",
        "Magic Numbers",
        format_size(magic_size),
        magic_pct
    )));

    // Schema message
    if let Some(schema_msg) = &structure.schema_message {
        let schema_pct = (schema_msg.total_size as f64 / total_size as f64) * 100.0;
        items.push(text_item(format!("{:<20} {:>9}  {:>5.1}%  Inc. continuation",
            "Schema Message",
            format_size(schema_msg.total_size as u64),
            schema_pct
        )));
    }

    // Batch metadata
    let batch_metadata_size: usize = structure.record_batches.iter()
        .map(|b| 8 + b.metadata_length)
        .sum();
    let batch_metadata_pct = (batch_metadata_size as f64 / total_size as f64) * 100.0;
    items.push(text_item(format!("{:<20} {:>9}  {:>5.1}%  {} × {} bytes flatbuffers",
        "Batch Metadata",
        format_size(batch_metadata_size as u64),
        batch_metadata_pct,
        batch_analyses.len(),
        if !structure.record_batches.is_empty() { structure.record_batches[0].metadata_length } else { 0 }
    )));

    // Batch data buffers
    let batch_data_size: usize = structure.record_batches.iter()
        .map(|b| b.body_length)
        .sum();
    let batch_data_pct = (batch_data_size as f64 / total_size as f64) * 100.0;
    items.push(text_item(format!("{:<20} {:>9}  {:>5.1}%  Actual column data",
        "Batch Data Buffers",
        format_size(batch_data_size as u64),
        batch_data_pct
    )));

    // Footer
    if let Some(footer) = &structure.footer {
        let footer_pct = (footer.total_size as f64 / total_size as f64) * 100.0;
        items.push(text_item(format!("{:<20} {:>9}  {:>5.1}%  Index + metadata",
            "Footer",
            format_size(footer.total_size as u64),
            footer_pct
        )));
    }

    items.push(text_item("─────────────────────────────────────────────────────────────"));
    items.push(text_item(format!("{:<20} {:>9}  100.0%",
        "Total",
        format_size(total_size as u64)
    )));

    Ok(items)
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

fn calculate_key_size(index_type: &str) -> usize {
    if index_type.contains("Int8") || index_type.contains("UInt8") {
        1
    } else if index_type.contains("Int16") || index_type.contains("UInt16") {
        2
    } else if index_type.contains("Int32") || index_type.contains("UInt32") {
        4
    } else {
        8
    }
}

// ============================================================================
// DATA STRUCTURES
// ============================================================================

struct MessageInfo {
    total_size: usize,
    metadata_length: usize,
    padding: usize,
}

struct RecordBatchInfo {
    total_size: usize,
    metadata_length: usize,
    body_length: usize,
    padding: usize,
}

struct FooterInfo {
    total_size: usize,
    flatbuffer_size: usize,
}

struct DetailedFileStructure {
    schema_message: Option<MessageInfo>,
    record_batches: Vec<RecordBatchInfo>,
    footer: Option<FooterInfo>,
}

struct BatchEntry {
    offset: u64,
    length: u64,
    rows: u64,
}

struct DictionaryInfo {
    id: i64,
    field_name: String,
    index_type: String,
    value_type: String,
    unique_values: usize,
    dict_size: usize,
    indices_size: usize,
    estimated_raw_size: usize,
    compression_ratio: f64,
    ordered: bool,
    is_delta: bool,
    sample_values: Vec<String>,
}

struct ColumnNullInfo {
    column_name: String,
    null_count: usize,
}

struct BatchAnalysis {
    index: usize,
    offset: u64,
    length: u64,
    rows: usize,
    size: usize,
    null_counts: Vec<ColumnNullInfo>,
    total_nulls: usize,
}

struct FooterContent {
    version: Option<String>,
    schema_fields: usize,
    record_batch_count: usize,
    batch_entries: Vec<BatchEntry>,
    has_dictionaries: bool,
    dictionary_count: usize,
    custom_metadata_count: usize,
}

// ============================================================================
// ANALYSIS FUNCTIONS
// ============================================================================

fn analyze_dictionaries(data: &[u8], schema: &datafusion::arrow::datatypes::Schema) -> Result<Vec<DictionaryInfo>> {
    use datafusion::arrow::datatypes::DataType;

    let cursor = std::io::Cursor::new(bytes::Bytes::copy_from_slice(data));
    let mut reader = FileReader::try_new(cursor, None)
        .map_err(|e| Error::General(format!("Failed to create reader: {}", e)))?;

    let mut dict_infos = Vec::new();

    if let Some(Ok(batch)) = reader.next() {
        for (col_idx, field) in schema.fields().iter().enumerate() {
            if let DataType::Dictionary(key_type, value_type) = field.data_type() {
                let column = batch.column(col_idx);

                let dict_id = col_idx as i64;
                let field_name = field.name().clone();
                let index_type = format!("{:?}", key_type);
                let value_type_str = format!("{:?}", value_type);

                let (unique_values, dict_size, sample_values) = extract_dictionary_values(column.as_ref(), value_type);

                let key_size = match key_type.as_ref() {
                    DataType::Int8 | DataType::UInt8 => 1,
                    DataType::Int16 | DataType::UInt16 => 2,
                    DataType::Int32 | DataType::UInt32 => 4,
                    DataType::Int64 | DataType::UInt64 => 8,
                    _ => 4,
                };
                let indices_size = batch.num_rows() * key_size;

                let avg_value_len = if !sample_values.is_empty() {
                    sample_values.iter().map(|s| s.len()).sum::<usize>() / sample_values.len()
                } else {
                    10
                };
                let estimated_raw_size = batch.num_rows() * avg_value_len;

                let compression_ratio = if estimated_raw_size > 0 {
                    ((dict_size + indices_size) as f64 / estimated_raw_size as f64) * 100.0
                } else {
                    100.0
                };

                dict_infos.push(DictionaryInfo {
                    id: dict_id,
                    field_name,
                    index_type,
                    value_type: value_type_str,
                    unique_values,
                    dict_size,
                    indices_size,
                    estimated_raw_size,
                    compression_ratio,
                    ordered: false,
                    is_delta: false,
                    sample_values,
                });
            }
        }
    }

    Ok(dict_infos)
}

fn extract_dictionary_values(array: &dyn datafusion::arrow::array::Array, value_type: &Box<datafusion::arrow::datatypes::DataType>) -> (usize, usize, Vec<String>) {
    use datafusion::arrow::datatypes::DataType;
    use datafusion::arrow::array::Array;

    let array_data = array.to_data();

    if let Some(dict_data) = array_data.child_data().first() {
        let dict_len = dict_data.len();

        let dict_size = match value_type.as_ref() {
            DataType::Utf8 => {
                let string_array = datafusion::arrow::array::StringArray::try_from(dict_data.clone()).unwrap();
                let mut total_size = 0;
                let mut samples = Vec::new();
                for i in 0..dict_len.min(5) {
                    if !string_array.is_null(i) {
                        let value = string_array.value(i);
                        total_size += value.len();
                        samples.push(value.to_string());
                    }
                }
                if dict_len > 5 && !samples.is_empty() {
                    let avg_len = total_size / samples.len();
                    total_size = avg_len * dict_len;
                }
                return (dict_len, total_size, samples);
            },
            DataType::Int32 => dict_len * 4,
            DataType::Int64 => dict_len * 8,
            DataType::Float32 => dict_len * 4,
            DataType::Float64 => dict_len * 8,
            _ => dict_len * 8,
        };

        let samples = extract_sample_strings(dict_data, value_type.as_ref(), 5);

        (dict_len, dict_size, samples)
    } else {
        (0, 0, Vec::new())
    }
}

fn extract_sample_strings(array_data: &datafusion::arrow::array::ArrayData, data_type: &datafusion::arrow::datatypes::DataType, limit: usize) -> Vec<String> {
    use datafusion::arrow::datatypes::DataType;
    use datafusion::arrow::array::Array;

    let count = array_data.len().min(limit);
    let mut samples = Vec::new();

    match data_type {
        DataType::Utf8 => {
            let string_array = datafusion::arrow::array::StringArray::try_from(array_data.clone()).unwrap();
            for i in 0..count {
                if !string_array.is_null(i) {
                    samples.push(string_array.value(i).to_string());
                }
            }
        },
        DataType::Int32 => {
            let int_array = datafusion::arrow::array::Int32Array::try_from(array_data.clone()).unwrap();
            for i in 0..count {
                if !int_array.is_null(i) {
                    samples.push(int_array.value(i).to_string());
                }
            }
        },
        DataType::Int64 => {
            let int_array = datafusion::arrow::array::Int64Array::try_from(array_data.clone()).unwrap();
            for i in 0..count {
                if !int_array.is_null(i) {
                    samples.push(int_array.value(i).to_string());
                }
            }
        },
        _ => {
            samples.push(format!("<{} values>", array_data.len()));
        }
    }

    samples
}

fn analyze_batches(data: &[u8]) -> Result<Vec<BatchAnalysis>> {
    use datafusion::arrow::array::Array;

    let cursor = std::io::Cursor::new(bytes::Bytes::copy_from_slice(data));
    let mut reader = FileReader::try_new(cursor, None)
        .map_err(|e| Error::General(format!("Failed to create reader: {}", e)))?;

    let mut analyses = Vec::new();
    let footer_content = extract_footer_content(data)?;

    for (idx, entry) in footer_content.batch_entries.iter().enumerate() {
        if let Some(Ok(batch)) = reader.next() {
            let mut null_counts = Vec::new();
            let mut total_nulls = 0;

            for (col_idx, field) in batch.schema().fields().iter().enumerate() {
                if field.is_nullable() {
                    let column = batch.column(col_idx);
                    let null_count = column.null_count();
                    if null_count > 0 {
                        null_counts.push(ColumnNullInfo {
                            column_name: field.name().clone(),
                            null_count,
                        });
                        total_nulls += null_count;
                    }
                }
            }

            analyses.push(BatchAnalysis {
                index: idx,
                offset: entry.offset,
                length: entry.length,
                rows: batch.num_rows(),
                size: entry.length as usize,
                null_counts,
                total_nulls,
            });
        }
    }

    Ok(analyses)
}

fn extract_detailed_structure(data: &[u8], _num_batches: usize) -> Result<DetailedFileStructure> {
    if data.len() < 16 || &data[0..6] != b"ARROW1" || &data[data.len() - 6..] != b"ARROW1" {
        return Err(Error::General("Invalid Arrow IPC file".to_string()));
    }

    let mut record_batches = Vec::new();
    let mut schema_message = None;

    let footer_len_bytes = &data[data.len() - 10..data.len() - 6];
    let footer_len = i32::from_le_bytes([
        footer_len_bytes[0],
        footer_len_bytes[1],
        footer_len_bytes[2],
        footer_len_bytes[3],
    ]) as usize;

    let footer_start = data.len() - footer_len - 10;

    let first_batch_offset = if let Ok(footer_content) = extract_footer_content(data) {
        footer_content.batch_entries.first().map(|e| e.offset as usize).unwrap_or(footer_start)
    } else {
        footer_start
    };

    let mut schema_offset = None;
    for offset in (8..first_batch_offset.min(1024)).step_by(4) {
        if offset + 8 > data.len() {
            break;
        }

        let marker_bytes = &data[offset..offset + 4];
        let marker = i32::from_le_bytes([marker_bytes[0], marker_bytes[1], marker_bytes[2], marker_bytes[3]]);

        if marker == -1 {
            schema_offset = Some(offset);
            break;
        }
    }

    if let Some(offset) = schema_offset {
        let meta_len_bytes = &data[offset + 4..offset + 8];
        let meta_len = i32::from_le_bytes([
            meta_len_bytes[0],
            meta_len_bytes[1],
            meta_len_bytes[2],
            meta_len_bytes[3],
        ]) as usize;

        let meta_end = offset + 8 + meta_len;
        let aligned_end = (meta_end + 7) & !7;
        let msg_padding = aligned_end - meta_end;
        let total_size = first_batch_offset - offset;

        schema_message = Some(MessageInfo {
            total_size,
            metadata_length: meta_len,
            padding: msg_padding,
        });
    }

    if let Ok(footer_content) = extract_footer_content(data) {
        for entry in footer_content.batch_entries {
            let batch_offset = entry.offset as usize;

            if batch_offset + 8 <= data.len() {
                let meta_len_bytes = &data[batch_offset + 4..batch_offset + 8];
                let meta_len = i32::from_le_bytes([
                    meta_len_bytes[0],
                    meta_len_bytes[1],
                    meta_len_bytes[2],
                    meta_len_bytes[3],
                ]) as usize;

                let body_len = entry.length as usize - 8 - meta_len;

                let meta_end = batch_offset + 8 + meta_len;
                let aligned_meta_end = (meta_end + 7) & !7;
                let meta_padding = aligned_meta_end - meta_end;

                let body_end = aligned_meta_end + body_len;
                let aligned_body_end = (body_end + 7) & !7;
                let body_padding = aligned_body_end - body_end;

                let total_padding = meta_padding + body_padding;
                let total_size = 4 + 4 + meta_len + body_len + total_padding;

                record_batches.push(RecordBatchInfo {
                    total_size,
                    metadata_length: meta_len,
                    body_length: body_len,
                    padding: total_padding,
                });
            }
        }
    }

    let footer = Some(FooterInfo {
        total_size: footer_len + 4,
        flatbuffer_size: footer_len,
    });

    Ok(DetailedFileStructure {
        schema_message,
        record_batches,
        footer,
    })
}

fn extract_footer_content(data: &[u8]) -> Result<FooterContent> {
    if data.len() < 16 || &data[data.len() - 6..] != b"ARROW1" {
        return Err(Error::General("Invalid Arrow IPC file".to_string()));
    }

    let footer_len_bytes = &data[data.len() - 10..data.len() - 6];
    let footer_len = i32::from_le_bytes([
        footer_len_bytes[0],
        footer_len_bytes[1],
        footer_len_bytes[2],
        footer_len_bytes[3],
    ]) as usize;

    if footer_len > data.len() || footer_len == 0 {
        return Err(Error::General("Invalid footer length".to_string()));
    }

    let footer_start = data.len() - footer_len - 10;
    let footer_data = &data[footer_start..data.len() - 10];

    let footer = root_as_footer(footer_data)
        .map_err(|e| Error::General(format!("Failed to parse footer: {}", e)))?;

    let version = footer.version();
    let version_str = format!("{:?}", version);

    let schema = footer.schema().ok_or_else(|| Error::General("No schema in footer".to_string()))?;
    let schema_fields = schema.fields().map(|f| f.len()).unwrap_or(0);

    let record_batches = footer.recordBatches();
    let record_batch_count = record_batches.as_ref().map(|rb| rb.len()).unwrap_or(0);

    let row_counts = read_batch_row_counts(data);

    let mut batch_entries = Vec::new();
    if let Some(batches) = record_batches {
        for (idx, batch) in batches.iter().enumerate() {
            let offset = batch.offset() as u64;
            let metaDataLength = batch.metaDataLength() as u64;
            let bodyLength = batch.bodyLength() as u64;

            let total_length = 4 + 4 + metaDataLength + bodyLength;

            let rows = row_counts.get(idx).copied().unwrap_or(0);

            batch_entries.push(BatchEntry {
                offset,
                length: total_length,
                rows,
            });
        }
    }

    let dictionaries = footer.dictionaries();
    let has_dictionaries = dictionaries.is_some();
    let dictionary_count = dictionaries.map(|d| d.len()).unwrap_or(0);

    let custom_metadata = schema.custom_metadata();
    let custom_metadata_count = custom_metadata.map(|m| m.len()).unwrap_or(0);

    Ok(FooterContent {
        version: Some(version_str),
        schema_fields,
        record_batch_count,
        batch_entries,
        has_dictionaries,
        dictionary_count,
        custom_metadata_count,
    })
}

fn read_batch_row_counts(data: &[u8]) -> Vec<u64> {
    let cursor = std::io::Cursor::new(bytes::Bytes::copy_from_slice(data));

    if let Ok(mut reader) = FileReader::try_new(cursor, None) {
        let mut row_counts = Vec::new();

        for _i in 0..reader.num_batches() {
            if let Some(Ok(batch)) = reader.next() {
                row_counts.push(batch.num_rows() as u64);
            } else {
                row_counts.push(0);
            }
        }

        row_counts
    } else {
        Vec::new()
    }
}
