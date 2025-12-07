//! Common utilities for CLI commands

use crate::config::{ResolveTableRef, ResolvedTable};
use crate::core::{CatalogClient, CatalogConfig, TableRef};
use crate::error::{Error, Result};
use iceberg::table::Table;

/// Resolved table that can be either a direct path or a catalog table
#[allow(clippy::large_enum_variant)]
pub enum TableResolution {
    /// Direct path to table on storage
    Path(String),
    /// Table loaded from catalog
    CatalogTable {
        /// The loaded iceberg Table
        table: Table,
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
            TableResolution::CatalogTable { table, .. } => {
                table.metadata().location().to_string()
            }
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
    pub fn as_catalog_table(&self) -> Option<&Table> {
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
        } => {
            resolve_from_catalog(&table_name, &catalog_config).await
        }
    }
}

/// Resolve a table from a catalog
async fn resolve_from_catalog(table_input: &str, catalog: &CatalogConfig) -> Result<TableResolution> {
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
        table,
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
