//! Common utilities for CLI commands

use crate::error::{Error, Result};

// Re-export resolution types from core (business logic belongs there)
pub use crate::core::resolution::{
    CatalogContext, CatalogResolution, TableResolution, no_catalog_error, no_namespace_error,
    no_table_error, resolve_catalog_from_context, resolve_table, resolve_table_path,
};

// Re-export CliTableContext for convenience (used by resolve_table_from_context)
use crate::cli::parser::CliTableContext;

impl From<&CliTableContext> for CatalogContext {
    fn from(ctx: &CliTableContext) -> Self {
        CatalogContext {
            table: ctx.table.clone(),
            namespace: ctx.namespace.clone(),
            catalog: ctx.catalog.clone(),
            warehouse: ctx.warehouse.clone(),
        }
    }
}

/// Resolve catalog context from CLI table context
///
/// Convenience wrapper that converts `CliTableContext` to `CatalogContext`
/// and delegates to `resolve_catalog_from_context`.
pub async fn resolve_catalog(ctx: &CliTableContext) -> crate::error::Result<CatalogResolution> {
    resolve_catalog_from_context(&CatalogContext::from(ctx)).await
}

/// Resolve a table from global context
///
/// This is the primary entry point for resolving tables from CLI commands.
/// Uses the global `-t/--table` and `-n/--namespace` options along with
/// catalog configuration.
pub async fn resolve_table_from_context(ctx: &CliTableContext) -> Result<TableResolution> {
    let table_ref = ctx.table_ref();
    resolve_table(&table_ref, ctx.catalog_config.as_ref()).await
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
