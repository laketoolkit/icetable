//! CatalogResolution - resolved catalog context for catalog-level operations

use crate::config::Config;
use crate::core::catalog::RestCatalogClient;
use crate::core::{CatalogConfig, IcebergTable};
use crate::error::{Error, Result};

use super::CatalogContext;

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
// Resolution Function
// =============================================================================

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
    let (catalog_name, mut catalog_config) = ctx.resolve_catalog_config(&config)?;

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
                 - List warehouses: icetable warehouse ls\n  \
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_polaris_catalog_by_uri() {
        let config = CatalogConfig::rest("http://localhost:8181/api/catalog");
        assert!(is_polaris_catalog(&config));
    }

    #[test]
    fn test_is_polaris_catalog_by_property() {
        let mut config = CatalogConfig::rest("http://localhost:8181");
        config
            .properties
            .insert("catalog-impl".to_string(), "org.apache.polaris".to_string());
        assert!(is_polaris_catalog(&config));
    }

    #[test]
    fn test_is_not_polaris_catalog() {
        let config = CatalogConfig::rest("http://localhost:19120/api/v1");
        assert!(!is_polaris_catalog(&config));
    }
}
