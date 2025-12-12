//! Table resolution
//!
//! Resolves table references (aliases, paths, catalog identifiers) to concrete locations.

use super::{CatalogConfig, Config};
use crate::error::{Error, Result};

/// Resolved table reference
#[derive(Debug, Clone)]
pub enum ResolvedTable {
    /// Direct path to table
    Path(String),
    /// Reference via catalog (boxed to reduce enum size)
    Catalog {
        /// Name of the catalog in config
        catalog_name: String,
        /// Configuration for the catalog (boxed to reduce variant size)
        catalog_config: Box<CatalogConfig>,
        /// Table identifier within the catalog (namespace.table)
        table_name: String,
    },
}

impl ResolvedTable {
    /// Get the path if this is a direct path resolution
    pub fn as_path(&self) -> Option<&str> {
        match self {
            ResolvedTable::Path(p) => Some(p),
            ResolvedTable::Catalog { .. } => None,
        }
    }

    /// Check if this is a catalog reference
    pub fn is_catalog(&self) -> bool {
        matches!(self, ResolvedTable::Catalog { .. })
    }

    /// Get display name for the table
    pub fn display_name(&self) -> String {
        match self {
            ResolvedTable::Path(p) => p.split('/').next_back().unwrap_or(p).to_string(),
            ResolvedTable::Catalog {
                catalog_name,
                table_name,
                ..
            } => format!("{}.{}", catalog_name, table_name),
        }
    }
}

/// Extension trait for resolving table paths from Option<String>
///
/// This provides a clean API: `args.path.resolve()?`
pub trait ResolvePath {
    /// Resolve the path using config defaults if None
    fn resolve(&self) -> Result<String>;
}

impl ResolvePath for Option<String> {
    fn resolve(&self) -> Result<String> {
        let config = Config::load()?;
        let reference = match self {
            Some(path) => path.clone(),
            None => config
                .get_current_context()
                .map(|s| s.to_string())
                .ok_or(Error::NoTable)?,
        };

        match config.resolve_table(&reference)? {
            ResolvedTable::Path(path) => Ok(path),
            ResolvedTable::Catalog {
                catalog_name,
                table_name,
                ..
            } => {
                // For ResolvePath we only support direct paths
                // Use ResolveTableRef for catalog support
                Err(Error::UnsupportedFeature {
                    feature: format!(
                        "Direct path resolution for catalog tables ({}.{}). Use ResolveTableRef or inspect command instead",
                        catalog_name, table_name
                    ),
                })
            }
        }
    }
}

/// Extension trait for resolving table references from Option<String>
///
/// This provides a clean API: `args.path.resolve_ref()?`
/// Returns the full ResolvedTable including catalog information
pub trait ResolveTableRef {
    /// Resolve to a full table reference (path or catalog)
    fn resolve_ref(&self) -> Result<ResolvedTable>;
}

impl ResolveTableRef for Option<String> {
    fn resolve_ref(&self) -> Result<ResolvedTable> {
        let config = Config::load()?;

        // If explicit table reference provided, use it
        if let Some(path) = self {
            return config.resolve_table(path);
        }

        // No explicit table - check if context includes a table
        let context = config.get_current_context().ok_or(Error::NoTable)?;

        // Parse context: catalog, catalog.namespace, or catalog.namespace.table
        let parts: Vec<&str> = context.splitn(3, '.').collect();

        match parts.len() {
            1 | 2 => {
                // Just catalog name or catalog.namespace - no table specified
                Err(Error::NoTable)
            }
            3 => {
                // catalog.namespace.table - full context, resolve it
                config.resolve_table(context)
            }
            _ => Err(Error::Parse {
                message: format!("Invalid context format: '{}'", context),
                source: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolved_table_display_name() {
        let path = ResolvedTable::Path("s3://bucket/db/events".to_string());
        assert_eq!(path.display_name(), "events");

        let catalog = ResolvedTable::Catalog {
            catalog_name: "nessie".to_string(),
            catalog_config: Box::new(CatalogConfig::rest("http://localhost")),
            table_name: "analytics.events".to_string(),
        };
        assert_eq!(catalog.display_name(), "nessie.analytics.events");
    }

    #[test]
    fn test_resolved_table_as_path() {
        let path = ResolvedTable::Path("s3://bucket/table".to_string());
        assert_eq!(path.as_path(), Some("s3://bucket/table"));

        let catalog = ResolvedTable::Catalog {
            catalog_name: "test".to_string(),
            catalog_config: Box::new(CatalogConfig::rest("http://localhost")),
            table_name: "table".to_string(),
        };
        assert_eq!(catalog.as_path(), None);
    }

    #[test]
    fn test_resolved_table_is_catalog() {
        let path = ResolvedTable::Path("s3://bucket/table".to_string());
        assert!(!path.is_catalog());

        let catalog = ResolvedTable::Catalog {
            catalog_name: "test".to_string(),
            catalog_config: Box::new(CatalogConfig::rest("http://localhost")),
            table_name: "table".to_string(),
        };
        assert!(catalog.is_catalog());
    }
}
