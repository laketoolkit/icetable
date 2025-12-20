//! CatalogContext - input from CLI options for resolution

use crate::config::Config;
use crate::core::CatalogConfig;
use crate::error::{Error, Result};

/// Context for catalog and table resolution operations
///
/// This struct holds the necessary context from CLI options for resolving
/// catalog and table references. Used by all commands that need to resolve
/// tables or interact with catalogs.
///
/// # See Also
///
/// - [`super::TableResolution`] - The result of resolving a `CatalogContext`
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

    /// Resolve catalog name from context or config
    ///
    /// Priority: -c CLI option > config context
    pub fn resolve_catalog_name(&self, config: &Config) -> Result<String> {
        self.catalog
            .clone()
            .or_else(|| config.get_current_catalog().map(String::from))
            .ok_or(Error::NoCatalog)
    }

    /// Resolve catalog name and configuration
    ///
    /// Returns both the catalog name and its configuration.
    /// Priority: -c CLI option > config context
    pub fn resolve_catalog_config(&self, config: &Config) -> Result<(String, CatalogConfig)> {
        let catalog_name = self.resolve_catalog_name(config)?;

        let catalog_config =
            config
                .catalogs
                .get(&catalog_name)
                .cloned()
                .ok_or_else(|| Error::CatalogNotFound {
                    name: catalog_name.clone(),
                })?;

        Ok((catalog_name, catalog_config))
    }
}
