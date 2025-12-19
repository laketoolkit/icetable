//! Common utilities for CLI commands

use crate::error::{Error, Result};

// Re-export resolution types from core (business logic belongs there)
pub use crate::core::resolution::{
    CatalogContext, CatalogResolution, TableResolution, no_catalog_error, no_namespace_error,
    no_table_error, resolve_catalog_from_context, resolve_table, resolve_table_path,
    resolve_table_with_catalog,
};

/// Resolve catalog context from CLI context
///
/// Delegates to `resolve_catalog_from_context` from core.
pub async fn resolve_catalog(ctx: &CatalogContext) -> crate::error::Result<CatalogResolution> {
    resolve_catalog_from_context(ctx).await
}

/// Resolve a table from global context
///
/// This is the primary entry point for resolving tables from CLI commands.
/// Uses the global `-t/--table` and `-n/--namespace` options along with
/// catalog configuration.
///
/// Resolution priority:
/// 1. If `--catalog-uri` is provided, use ad-hoc catalog
/// 2. If `-c/--catalog` is provided, use named catalog from config
/// 3. If current catalog context is set in config, use it
/// 4. Fall back to path/alias resolution
pub async fn resolve_table_from_context(ctx: &CatalogContext) -> Result<TableResolution> {
    // Priority 1: Ad-hoc catalog config (--catalog-uri)
    if ctx.catalog_config.is_some() {
        let table_ref = ctx.table_ref();
        return resolve_table(&table_ref, ctx.catalog_config.as_ref()).await;
    }

    // Priority 2 & 3: Named catalog (-c) or config context
    // Build fully qualified reference: catalog.namespace.table
    let config = crate::config::Config::load()?;
    let catalog_name = ctx
        .catalog
        .as_deref()
        .or_else(|| config.get_current_catalog());

    if let Some(catalog_name) = catalog_name {
        if let Some(base_config) = config.catalogs.get(catalog_name) {
            // Clone config to apply warehouse override
            let mut catalog_config = base_config.clone();

            // Apply warehouse: -w > config context
            let warehouse = ctx
                .warehouse
                .clone()
                .or_else(|| config.get_current_warehouse());
            if let Some(wh) = warehouse {
                catalog_config.warehouse = Some(wh);
            }

            // Get namespace from -n or config context
            let config_ns = config.get_current_namespace();
            let namespace = ctx.namespace.as_deref().or(config_ns.as_deref());

            // Get table from -t or config context
            let config_table = config.get_current_table();
            let table = ctx.table.as_deref().or(config_table.as_deref());

            if let (Some(ns), Some(tbl)) = (namespace, table) {
                // Full catalog.namespace.table resolution with credentials
                let full_table_name = format!("{}.{}", ns, tbl);
                return resolve_table_with_catalog(&full_table_name, &catalog_config, catalog_name)
                    .await;
            }
        }
    }

    // Priority 4: Fall back to path/alias resolution
    let table_ref = ctx.table_ref();
    resolve_table(&table_ref, None).await
}

/// Print JSON to stdout, converting serialization errors to our Error type
pub fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value).map_err(|e| Error::Serialization {
        message: format!("JSON serialization failed: {}", e),
    })?;
    println!("{}", json);
    Ok(())
}

/// Print the standard dry-run header message
///
/// Used consistently across commands that support --dry-run
pub fn print_dry_run_header() {
    use colored::Colorize;
    println!("{}", "DRY RUN - No changes made".yellow().bold());
    println!();
}

/// Print metadata version if present
///
/// Used consistently when operations return an optional new_version
pub fn print_version_if_present(new_version: Option<i64>) {
    if let Some(v) = new_version {
        println!("New metadata version: v{}", v);
    }
}

// Re-export progress utilities from utils
pub use crate::utils::{create_progress_bar, create_spinner};

/// Print dry-run output for ref delete operations (branch/tag)
///
/// Shows what would be deleted without actually deleting
pub fn print_ref_delete_dry_run(ref_type: &str, name: &str, snapshot_id: i64) {
    use colored::Colorize;
    print_dry_run_header();
    println!("Would delete the following:");
    println!("  {}: {} (snapshot {})", ref_type, name.cyan(), snapshot_id);
    println!();
    println!("{}", "Run without --dry-run to apply this change.".dimmed());
}

/// Extract table name from a path
///
/// Returns the last component of the path, or "table" as fallback
pub fn extract_table_name(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("table")
}

/// Output data in the specified format (json/yaml/text)
///
/// Helper that reduces boilerplate for commands that output structured data.
/// For text output, the provided closure is called to format the data.
///
/// # Example
/// ```ignore
/// output_formatted(&args.output, &data, || {
///     println!("Files: {}", data.files);
///     println!("Size: {}", data.size);
/// })
/// ```
pub fn output_formatted<T, F>(format: &str, data: &T, text_formatter: F) -> Result<()>
where
    T: serde::Serialize,
    F: FnOnce(),
{
    match format {
        "json" => print_json(data),
        "yaml" => {
            let yaml = serde_yaml_ng::to_string(data).map_err(|e| Error::Serialization {
                message: format!("YAML serialization failed: {}", e),
            })?;
            print!("{}", yaml);
            Ok(())
        }
        _ => {
            text_formatter();
            Ok(())
        }
    }
}
