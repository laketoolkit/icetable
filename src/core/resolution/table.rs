//! TableResolution - resolved table reference (path or catalog table)

use std::sync::Arc;

use crate::config::{ResolveTableRef, ResolvedTable};
use crate::core::metadata::IcebergMetadataService;
use crate::core::{CatalogConfig, IcebergTable, TableCommitter, TableLoader, TableRef};
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
        /// Catalog configuration (boxed to reduce enum size)
        catalog_config: Box<CatalogConfig>,
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

    // =========================================================================
    // Factory methods for IcebergMetadataService and Table
    // =========================================================================

    /// Get an `Arc<IcebergTable>` from this resolution
    ///
    /// Use this when you need direct access to the iceberg Table object.
    /// For catalog tables, returns the already-loaded table.
    /// For direct paths, loads the table from storage.
    ///
    /// # Example
    /// ```ignore
    /// let resolution = resolve_table_from_context(ctx).await?;
    /// let table = resolution.to_table().await?;
    /// let metadata = table.metadata();
    /// ```
    pub async fn to_table(&self) -> Result<Arc<IcebergTable>> {
        match self {
            TableResolution::CatalogTable { table, .. } => {
                // Return the already-loaded catalog table
                Ok(Arc::new((**table).clone()))
            }
            TableResolution::Path(path) => {
                // Load table from storage
                TableLoader::load_table(path, None).await
            }
        }
    }

    /// Create a read-only IcebergMetadataService from this resolution
    ///
    /// Use this for operations that only read metadata (analyze, inspect, list, lineage).
    /// When the table is from a catalog, uses the catalog's metadata for consistency.
    /// Does NOT create a committer - write operations will fail.
    ///
    /// # Example
    /// ```ignore
    /// let resolution = resolve_table_from_context(ctx).await?;
    /// let service = resolution.to_readonly_service().await?;
    /// let files = service.list_data_files().await?;
    /// ```
    pub async fn to_readonly_service(&self) -> Result<IcebergMetadataService> {
        match self {
            TableResolution::CatalogTable { table, .. } => {
                // Use catalog table's metadata for consistency
                IcebergMetadataService::from_catalog_table_readonly(table).await
            }
            TableResolution::Path(path) => {
                // Direct path - read from storage
                IcebergMetadataService::new_async(path.clone()).await
            }
        }
    }

    /// Create a writable IcebergMetadataService from this resolution
    ///
    /// Use this for operations that modify metadata (optimize, repair, expire, set).
    /// When the table is from a catalog, creates a proper committer for catalog commits.
    /// For direct paths, writes go directly to storage (no catalog tracking).
    ///
    /// # Arguments
    /// * `cli_catalog` - Optional catalog config from CLI (overrides resolution's config)
    /// * `branch` - Optional branch name for the operation
    ///
    /// # Example
    /// ```ignore
    /// let resolution = resolve_table_from_context(ctx).await?;
    /// let service = resolution.to_writable_service(ctx.catalog_config.as_ref(), args.branch.as_deref()).await?;
    /// service.write_snapshot(changes).await?;
    /// ```
    pub async fn to_writable_service(
        &self,
        cli_catalog: Option<&CatalogConfig>,
        branch: Option<&str>,
    ) -> Result<IcebergMetadataService> {
        match self {
            TableResolution::CatalogTable {
                table,
                namespace,
                name,
                catalog_config,
            } => {
                // Use catalog table's metadata for proper UUID/snapshot consistency
                // Prefer CLI catalog config if provided
                let config = cli_catalog.unwrap_or(catalog_config);
                let committer =
                    TableCommitter::with_catalog(config.clone(), namespace.clone(), name.clone())?;
                IcebergMetadataService::from_catalog_table(
                    table,
                    branch.map(|s| s.to_string()),
                    committer,
                )
                .await
            }
            TableResolution::Path(path) => {
                // Direct path - create service with optional branch
                // Note: writes go directly to storage without catalog tracking
                if branch.is_some() {
                    IcebergMetadataService::new_with_branch(
                        path.clone(),
                        branch.map(|s| s.to_string()),
                    )
                    .await
                } else {
                    IcebergMetadataService::new_async(path.clone()).await
                }
            }
        }
    }
}

// =============================================================================
// Resolution Functions
// =============================================================================

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
        let table_input = table_ref.as_ref().ok_or_else(|| Error::MissingArgument {
            argument: "table".to_string(),
            description:
                "Table identifier required when using --catalog-uri (e.g., namespace.table)"
                    .to_string(),
        })?;

        // Ad-hoc catalog - no name for credentials lookup
        return resolve_from_catalog(table_input, catalog, None).await;
    }

    // Priority 2: Resolve from config
    let resolved = table_ref.resolve_ref()?;

    match resolved {
        ResolvedTable::Path(path) => Ok(TableResolution::Path(path)),
        ResolvedTable::Catalog {
            catalog_name,
            catalog_config,
            table_name,
        } => resolve_from_catalog(&table_name, &catalog_config, Some(&catalog_name)).await,
    }
}

/// Resolve a table with explicit catalog name (for credentials lookup)
///
/// Used by CLI commands that resolve catalog from config context.
pub async fn resolve_table_with_catalog(
    table_ref: &str,
    catalog_config: &CatalogConfig,
    catalog_name: &str,
) -> Result<TableResolution> {
    resolve_from_catalog(table_ref, catalog_config, Some(catalog_name)).await
}

/// Resolve a table from a catalog
///
/// # Arguments
/// * `table_input` - Table reference (namespace.table format)
/// * `catalog` - Catalog configuration
/// * `catalog_name` - Optional catalog name for credentials lookup
async fn resolve_from_catalog(
    table_input: &str,
    catalog: &CatalogConfig,
    catalog_name: Option<&str>,
) -> Result<TableResolution> {
    use crate::core::catalog::RestCatalogClient;

    // Parse namespace.table
    let table_ref = TableRef::parse(table_input, Some(catalog));

    let (namespace, name) = match &table_ref {
        TableRef::Catalog { namespace, name } => (namespace.clone(), name.clone()),
        TableRef::Path(_) => {
            return Err(Error::InvalidCatalogRef {
                ref_str: table_input.to_string(),
                message: "Expected catalog table reference (namespace.table), got path".to_string(),
            });
        }
    };

    // Load table from catalog - use with_name to get credentials from credentials.yaml
    let client = RestCatalogClient::with_name(catalog, catalog_name).await?;
    let table = client.load_table(&namespace, &name).await?;

    // Build catalog config with effective auth for later use (e.g., commits)
    // This ensures the committer uses credentials from credentials.yaml
    let mut effective_config = catalog.clone();
    if let Some(name) = catalog_name {
        if let Ok(auth) = catalog.effective_auth(name) {
            effective_config.auth = auth;
        }
    }

    Ok(TableResolution::CatalogTable {
        table: Box::new(table),
        namespace,
        name,
        catalog_config: Box::new(effective_config),
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
