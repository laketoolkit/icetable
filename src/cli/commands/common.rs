//! Common utilities for CLI commands

use crate::config::{ResolveTableRef, ResolvedTable};
use crate::core::maintenance::RefService;
use crate::core::{CatalogClient, CatalogConfig, IcebergTable, TableCommitter, TableContext, TableRef};
use crate::error::{Error, Result};

/// Resolved table that can be either a direct path or a catalog table
pub enum TableResolution {
    /// Direct path to table on storage
    Path(String),
    /// Table loaded from catalog
    CatalogTable {
        /// The loaded iceberg Table (boxed to reduce enum size)
        table: Box<IcebergTable>,
        /// Namespace path
        namespace: Vec<String>,
        /// Table name
        name: String,
    },
}

impl TableResolution {
    /// Get the table location (path for direct, location from metadata for catalog)
    pub fn location(&self) -> String {
        match self {
            TableResolution::Path(p) => p.clone(),
            TableResolution::CatalogTable { table, .. } => table.metadata().location().to_string(),
        }
    }

    /// Check if this is a catalog table
    pub fn is_catalog(&self) -> bool {
        matches!(self, TableResolution::CatalogTable { .. })
    }

    /// Get the path if direct
    pub fn as_path(&self) -> Option<&str> {
        match self {
            TableResolution::Path(p) => Some(p),
            TableResolution::CatalogTable { .. } => None,
        }
    }

    /// Get the catalog table if available
    pub fn as_catalog_table(&self) -> Option<&IcebergTable> {
        match self {
            TableResolution::Path(_) => None,
            TableResolution::CatalogTable { table, .. } => Some(table),
        }
    }
}

/// Resolve a table reference, supporting both CLI catalog args and config catalogs
///
/// Priority:
/// 1. If `cli_catalog` is provided (--catalog-uri), use it
/// 2. Otherwise, resolve from config (which may return direct path or catalog reference)
pub async fn resolve_table(
    table_ref: &Option<String>,
    cli_catalog: Option<&CatalogConfig>,
) -> Result<TableResolution> {
    // Priority 1: CLI catalog argument takes precedence
    if let Some(catalog) = cli_catalog {
        let table_input = table_ref.as_ref().ok_or_else(|| {
            Error::General(
                "Table identifier required when using --catalog-uri (e.g., namespace.table)"
                    .to_string(),
            )
        })?;

        return resolve_from_catalog(table_input, catalog).await;
    }

    // Priority 2: Resolve from config
    let resolved = table_ref.resolve_ref()?;

    match resolved {
        ResolvedTable::Path(path) => Ok(TableResolution::Path(path)),
        ResolvedTable::Catalog {
            catalog_config,
            table_name,
            ..
        } => resolve_from_catalog(&table_name, &catalog_config).await,
    }
}

/// Resolve a table from a catalog
async fn resolve_from_catalog(
    table_input: &str,
    catalog: &CatalogConfig,
) -> Result<TableResolution> {
    // Parse namespace.table
    let table_ref = TableRef::parse(table_input, Some(catalog));

    let (namespace, name) = match &table_ref {
        TableRef::Catalog { namespace, name } => (namespace.clone(), name.clone()),
        TableRef::Path(_) => {
            return Err(Error::General(format!(
                "Expected catalog table reference, got path: {}",
                table_input
            )));
        }
    };

    // Load table from catalog
    let client = CatalogClient::new(Some(catalog.clone())).await?;
    let table = client.load_table(&table_ref).await?;

    Ok(TableResolution::CatalogTable {
        table: Box::new(table),
        namespace,
        name,
    })
}

/// Helper to get just the path (resolves catalog tables to their location)
pub async fn resolve_table_path(
    table_ref: &Option<String>,
    cli_catalog: Option<&CatalogConfig>,
) -> Result<String> {
    let resolution = resolve_table(table_ref, cli_catalog).await?;
    Ok(resolution.location())
}

/// Print JSON to stdout, converting serialization errors to our Error type
pub fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| Error::General(format!("JSON serialization failed: {}", e)))?;
    println!("{}", json);
    Ok(())
}

/// Create a styled comfy_table::Table with standard icetable appearance
///
/// Uses UTF8_FULL preset and Dynamic content arrangement
pub fn create_table() -> comfy_table::Table {
    use comfy_table::{ContentArrangement, presets::UTF8_FULL};
    let mut table = comfy_table::Table::new();
    table.load_preset(UTF8_FULL);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table
}

/// Create a TableCommitter if the table was resolved from a catalog
///
/// Returns None if the table is a direct path (not from catalog)
pub fn create_committer(
    catalog_config: Option<&CatalogConfig>,
    resolution: &TableResolution,
) -> Option<TableCommitter> {
    match (catalog_config, resolution) {
        (
            Some(config),
            TableResolution::CatalogTable {
                namespace, name, ..
            },
        ) => Some(TableCommitter::with_catalog(
            config.clone(),
            namespace.clone(),
            name.clone(),
        )),
        _ => None,
    }
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

/// Create a spinner progress bar for long-running operations
///
/// Returns a spinner with the given message that ticks every 100ms
pub fn create_spinner(message: &str) -> indicatif::ProgressBar {
    use indicatif::{ProgressBar, ProgressStyle};
    use std::time::Duration;

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template(&format!("{{spinner:.cyan}} {}...", message))
            .expect("hardcoded progress template is valid"),
    );
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

/// Print dry-run output for ref delete operations (branch/tag)
///
/// Shows what would be deleted without actually deleting
pub fn print_ref_delete_dry_run(ref_type: &str, name: &str, snapshot_id: i64) {
    use colored::Colorize;
    print_dry_run_header();
    println!("Would delete the following:");
    println!(
        "  {}: {} (snapshot {})",
        ref_type,
        name.cyan(),
        snapshot_id
    );
    println!();
    println!("{}", "Run without --dry-run to apply this change.".dimmed());
}

/// Format timestamp in milliseconds to human-readable string
///
/// Returns "unknown" if the timestamp is invalid
pub fn format_timestamp(timestamp_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "unknown".to_string())
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

/// Resolved context for an Iceberg table operation
///
/// Combines table resolution with TableContext, ensuring the table is Iceberg format
pub struct IcebergContext {
    /// The table resolution (path or catalog)
    pub resolution: TableResolution,
    /// The table context for operations
    pub ctx: TableContext,
}

/// Resolve a table reference and create an Iceberg context
///
/// This is a convenience function that combines:
/// 1. Resolving the table reference (path or catalog)
/// 2. Creating a TableContext from the resolved location
/// 3. Validating that the table is Iceberg format
///
/// Returns an IcebergContext with both the resolution and context,
/// useful when you need the resolution for creating a committer.
pub async fn resolve_iceberg_context(
    table_ref: &Option<String>,
    cli_catalog: Option<&CatalogConfig>,
) -> Result<IcebergContext> {
    let resolution = resolve_table(table_ref, cli_catalog).await?;
    let ctx = TableContext::from_path(Some(resolution.location())).await?;
    ctx.require_iceberg()?;
    Ok(IcebergContext { resolution, ctx })
}

/// Extract filename from a path string
///
/// Returns the last component after the final '/', or the whole string if no '/' present
pub fn extract_filename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Create a RefService with an optional committer
///
/// Convenience helper to avoid repeating the match pattern in branch/tag commands
pub fn create_ref_service(committer: Option<TableCommitter>) -> RefService {
    match committer {
        Some(c) => RefService::with_committer(c),
        None => RefService::new(),
    }
}
