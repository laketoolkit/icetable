//! Table and catalog resolution logic
//!
//! This module provides the core logic for resolving table references
//! from various sources (paths, catalogs, config context).
//!
//! # Resolution Flow
//!
//! ```text
//!                          CLI Options
//!                              │
//!                              ▼
//!     ┌────────────────────────────────────────────────┐
//!     │               CatalogContext                    │
//!     │  (table, namespace, catalog, warehouse options) │
//!     └────────────────────────────────────────────────┘
//!                              │
//!           ┌──────────────────┼──────────────────┐
//!           │                  │                  │
//!           ▼                  ▼                  ▼
//!   resolve_table_     resolve_catalog    (direct path)
//!   from_context()                          s3://...
//!           │                  │                  │
//!           ▼                  ▼                  ▼
//!     ┌──────────┐      ┌──────────────┐    ┌──────────┐
//!     │ Table    │      │   Catalog    │    │   Path   │
//!     │Resolution│      │  Resolution  │    │          │
//!     └──────────┘      └──────────────┘    └──────────┘
//!           │                  │
//!           │    ┌─────────────┘
//!           │    │
//!           ▼    ▼
//!     ┌─────────────────────────────────────┐
//!     │         Factory Methods             │
//!     ├─────────────────────────────────────┤
//!     │ to_table()         → IcebergTable   │
//!     │ to_readonly_service() → Reader      │
//!     │ to_writable_service() → Writer      │
//!     └─────────────────────────────────────┘
//! ```
//!
//! # Key Types
//!
//! - [`CatalogContext`] - Input from CLI options, holds unresolved references
//! - [`TableResolution`] - Result of resolving a table (Path or CatalogTable)
//! - [`CatalogResolution`] - Resolved catalog client with namespace context
//!
//! # Usage Example
//!
//! ```ignore
//! // From CLI command handler:
//! let resolution = resolve_table_from_context(&ctx).await?;
//!
//! // For read-only operations:
//! let service = resolution.to_readonly_service().await?;
//! let snapshots = service.list_snapshots(None).await?;
//!
//! // For write operations (requires catalog):
//! let service = resolution.to_writable_service(catalog_config, branch).await?;
//! service.write_snapshot(changes, operation, summary).await?;
//! ```

use std::sync::Arc;

use crate::config::{Config, ResolveTableRef, ResolvedTable};
use crate::core::catalog::RestCatalogClient;
use crate::core::metadata::IcebergMetadataService;
use crate::core::{
    CatalogClient, CatalogConfig, IcebergTable, TableCommitter, TableLoader, TableRef,
};
use crate::error::{Error, Result};

/// Context for catalog and table resolution operations
///
/// This struct holds the necessary context from CLI options for resolving
/// catalog and table references. Used by all commands that need to resolve
/// tables or interact with catalogs.
///
/// # See Also
///
/// - [`TableResolution`] - The result of resolving a `CatalogContext`
#[derive(Debug, Clone, Default)]
pub struct CatalogContext {
    /// Table name or path (from -t option)
    pub table: Option<String>,
    /// Namespace (from -n option)
    pub namespace: Option<String>,
    /// Catalog name (from -c option)
    pub catalog: Option<String>,
    /// Warehouse within catalog (from -w option)
    pub warehouse: Option<String>,
    /// Ad-hoc catalog configuration (from --catalog-uri CLI options)
    pub catalog_config: Option<CatalogConfig>,
}

impl CatalogContext {
    /// Get the full table reference, combining namespace and table if both are present
    ///
    /// If both namespace and table are specified, returns "namespace.table".
    /// If only table is specified, returns the table as-is.
    /// If neither is specified, returns None.
    pub fn table_ref(&self) -> Option<String> {
        match (&self.namespace, &self.table) {
            (Some(ns), Some(t)) => {
                // If table already contains namespace (has '.'), use it as-is
                if t.contains('.')
                    || t.starts_with("s3://")
                    || t.starts_with("gs://")
                    || t.starts_with("az://")
                    || t.starts_with("file://")
                    || t.starts_with("/")
                {
                    Some(t.clone())
                } else {
                    Some(format!("{}.{}", ns, t))
                }
            }
            (None, Some(t)) => Some(t.clone()),
            _ => None,
        }
    }
}

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
                    TableCommitter::with_catalog(config.clone(), namespace.clone(), name.clone());
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
// Catalog Resolution (for ls, create, delete, generate commands)
// =============================================================================

/// Resolved catalog context for catalog-level operations
///
/// Contains the resolved catalog, namespace, table, and REST client.
/// Used by commands that operate at the catalog level (ls, create, delete, generate).
pub struct CatalogResolution {
    /// Catalog name
    pub catalog_name: String,
    /// Catalog configuration (public for special cases like generate)
    pub catalog_config: CatalogConfig,
    /// Namespace (if specified via -n or config context)
    namespace: Option<String>,
    /// Table name (if specified via -t or config context)
    table: Option<String>,
    /// REST catalog client (encapsulated - use factory methods)
    client: RestCatalogClient,
}

impl CatalogResolution {
    // =========================================================================
    // Accessors
    // =========================================================================

    /// Check if a namespace is set
    pub fn has_namespace(&self) -> bool {
        self.namespace.is_some()
    }

    /// Check if a table is set
    pub fn has_table(&self) -> bool {
        self.table.is_some()
    }

    /// Get the namespace name if set
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    /// Get the table name if set
    pub fn table(&self) -> Option<&str> {
        self.table.as_deref()
    }

    /// Get the full table name (namespace.table) if both are set
    pub fn full_table_name(&self) -> Option<String> {
        match (&self.namespace, &self.table) {
            (Some(ns), Some(t)) => Some(format!("{}.{}", ns, t)),
            _ => None,
        }
    }

    /// Get namespace as `Vec<String>` parts for API calls
    ///
    /// Returns None if no namespace is set.
    pub fn namespace_parts(&self) -> Option<Vec<String>> {
        self.namespace
            .as_ref()
            .map(|ns| ns.split('.').map(String::from).collect())
    }

    // =========================================================================
    // Private helpers
    // =========================================================================

    /// Require namespace to be set, returning error if not
    fn require_namespace(&self) -> Result<Vec<String>> {
        self.namespace_parts().ok_or(Error::NoNamespace)
    }

    // =========================================================================
    // Factory methods for common catalog operations
    // =========================================================================

    /// List all namespaces in the catalog
    pub async fn list_namespaces(&self) -> Result<Vec<Vec<String>>> {
        self.client.list_namespaces(None).await
    }

    /// Check if a namespace exists in the catalog
    pub async fn namespace_exists(&self) -> Result<bool> {
        let ns_parts = self.require_namespace()?;
        let namespaces = self.client.list_namespaces(None).await?;
        Ok(namespaces.contains(&ns_parts))
    }

    /// List tables in the current namespace
    ///
    /// Requires namespace to be set.
    pub async fn list_tables(&self) -> Result<Vec<String>> {
        let ns_parts = self.require_namespace()?;
        self.client.list_tables(&ns_parts).await
    }

    /// Check if a table exists in the current namespace
    pub async fn table_exists(&self, table_name: &str) -> Result<bool> {
        let ns_parts = self.require_namespace()?;
        self.client.table_exists(&ns_parts, table_name).await
    }

    /// Load a table from the current namespace
    pub async fn load_table(&self, table_name: &str) -> Result<IcebergTable> {
        let ns_parts = self.require_namespace()?;
        self.client.load_table(&ns_parts, table_name).await
    }

    /// Create a new namespace
    pub async fn create_namespace(
        &self,
        properties: std::collections::HashMap<String, String>,
    ) -> Result<()> {
        let ns_parts = self.require_namespace()?;
        self.client.create_namespace(&ns_parts, properties).await
    }

    /// Delete a namespace
    pub async fn delete_namespace(&self) -> Result<()> {
        let ns_parts = self.require_namespace()?;
        self.client.delete_namespace(&ns_parts).await
    }

    /// Create a new table in the current namespace
    pub async fn create_table(
        &self,
        table_name: &str,
        schema: iceberg::spec::Schema,
        location: Option<&str>,
        properties: std::collections::HashMap<String, String>,
    ) -> Result<IcebergTable> {
        let ns_parts = self.require_namespace()?;
        self.client
            .create_table(&ns_parts, table_name, schema, location, properties)
            .await
    }

    /// Delete a table from the current namespace
    pub async fn delete_table(&self, table_name: &str, purge: bool) -> Result<()> {
        let ns_parts = self.require_namespace()?;
        self.client.delete_table(&ns_parts, table_name, purge).await
    }

    /// Get the underlying catalog for advanced operations (e.g., commits)
    pub fn catalog(&self) -> &dyn iceberg::Catalog {
        self.client.catalog()
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
            return Err(Error::InvalidCatalogRef {
                ref_str: table_input.to_string(),
                message: "Expected catalog table reference (namespace.table), got path".to_string(),
            });
        }
    };

    // Load table from catalog
    let client = CatalogClient::new(Some(catalog.clone())).await?;
    let table = client.load_table(&table_ref).await?;

    Ok(TableResolution::CatalogTable {
        table: Box::new(table),
        namespace,
        name,
        catalog_config: Box::new(catalog.clone()),
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

/// Resolve catalog context from global options
///
/// This is the primary entry point for catalog-level commands (ls, create, delete, generate).
/// Resolves catalog name, config, namespace, and table from:
/// 1. Global CLI options (-c, -w, -n, -t)
/// 2. Config context (current catalog/warehouse/namespace/table)
///
/// Priority for each field: CLI option > config context
///
/// # Arguments
/// * `ctx` - CatalogContext from global CLI options
///
/// # Returns
/// * `Ok(CatalogResolution)` with resolved context and ready-to-use client
/// * `Err` if no catalog is configured or catalog not found
pub async fn resolve_catalog_from_context(ctx: &CatalogContext) -> Result<CatalogResolution> {
    let config = Config::load()?;

    // Resolve catalog name: -c > config context
    let catalog_name = ctx
        .catalog
        .clone()
        .or_else(|| config.get_current_catalog().map(String::from))
        .ok_or(Error::NoCatalog)?;

    // Get catalog config
    let mut catalog_config =
        config
            .catalogs
            .get(&catalog_name)
            .cloned()
            .ok_or_else(|| Error::CatalogNotFound {
                name: catalog_name.clone(),
            })?;

    // Resolve warehouse: -w > config context
    let warehouse = ctx
        .warehouse
        .clone()
        .or_else(|| config.get_current_warehouse());

    if let Some(wh) = warehouse {
        catalog_config.warehouse = Some(wh);
    }

    // Check if catalog requires warehouse (e.g., Polaris)
    if catalog_config.warehouse.is_none() && is_polaris_catalog(&catalog_config) {
        return Err(Error::Configuration {
            message: format!(
                "Catalog '{}' requires a warehouse\n\n\
                 Hint:\n  \
                 - List warehouses: icetable admin warehouse ls\n  \
                 - Use flag: icetable -w <warehouse> ls\n  \
                 - Set context: icetable config use {}@<warehouse>",
                catalog_name, catalog_name
            ),
        });
    }

    // Resolve namespace: -n > config context
    let namespace = ctx
        .namespace
        .clone()
        .or_else(|| config.get_current_namespace());

    // Resolve table: -t > config context
    let table = ctx.table.clone().or_else(|| config.get_current_table());

    // Create REST client with catalog name for better error messages
    let client = RestCatalogClient::with_name(&catalog_config, Some(&catalog_name)).await?;

    Ok(CatalogResolution {
        catalog_name,
        catalog_config,
        namespace,
        table,
        client,
    })
}

/// Check if a catalog is Polaris (requires warehouse)
fn is_polaris_catalog(config: &CatalogConfig) -> bool {
    // Heuristic: Polaris has /api/catalog in the URI
    config.uri.contains("/api/catalog")
        || config
            .properties
            .get("catalog-impl")
            .is_some_and(|v| v.contains("polaris"))
}

// =============================================================================
// Error helpers
// =============================================================================

/// Create error for "no catalog specified"
///
/// Returns an error with helpful message. Use with `?` to propagate.
#[inline]
pub fn no_catalog_error() -> Error {
    Error::NoCatalog
}

/// Create error for "no namespace specified"
///
/// Returns an error with helpful message. Use with `?` to propagate.
#[inline]
pub fn no_namespace_error() -> Error {
    Error::NoNamespace
}

/// Create error for "no table specified"
///
/// Returns an error with helpful message. Use with `?` to propagate.
#[inline]
pub fn no_table_error() -> Error {
    Error::NoTable
}
