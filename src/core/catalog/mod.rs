//! Catalog abstraction for Iceberg tables
//!
//! Provides a unified interface for accessing tables through:
//! - Direct path (filesystem, S3, etc.) - the default mode
//! - REST Catalog (Nessie, Polaris, Tabular, Unity Catalog, etc.)
//!
//! # Usage
//!
//! ```ignore
//! // Direct path access (default)
//! icetable inspect --table s3://bucket/path/to/table
//!
//! // REST Catalog access
//! icetable --catalog-uri http://nessie:19120/api/v2 inspect analytics.events
//! ```

mod committer;
pub mod management;
mod rest;

pub use committer::TableCommitter;
pub use management::{
    create_management_client, create_management_client_with_name, CatalogManagement,
    CreateWarehouseRequest, StorageType, Warehouse, WarehouseType,
};
pub use rest::RestCatalogClient;

// Re-export from core::config for backward compatibility
pub use super::config::{CatalogAuth, CatalogConfig, CatalogType};

use crate::error::{Error, Result};
use iceberg::table::Table;

/// Resolved table reference - either a direct path or catalog identifier
#[derive(Debug, Clone)]
pub enum TableRef {
    /// Direct path to table (s3://..., gs://..., /local/path)
    Path(String),
    /// Catalog table identifier (namespace.table)
    Catalog {
        /// Namespace path (e.g., ["analytics", "db"])
        namespace: Vec<String>,
        /// Table name
        name: String,
    },
}

impl TableRef {
    /// Parse a table reference string
    ///
    /// If catalog is configured, treats the string as namespace.table
    /// Otherwise treats it as a direct path
    pub fn parse(s: &str, catalog_config: Option<&CatalogConfig>) -> Self {
        if catalog_config.is_some() {
            // Catalog mode: parse as namespace.table
            let parts: Vec<&str> = s.split('.').collect();
            if parts.len() >= 2 {
                // Safe: we checked len >= 2, so last() always exists
                let name = parts.last().expect("checked len >= 2").to_string();
                let namespace = parts[..parts.len() - 1]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                TableRef::Catalog { namespace, name }
            } else {
                // Single name, use default namespace
                TableRef::Catalog {
                    namespace: vec!["default".to_string()],
                    name: s.to_string(),
                }
            }
        } else {
            // Direct path mode
            TableRef::Path(s.to_string())
        }
    }

    /// Get display name for the table
    pub fn display_name(&self) -> String {
        match self {
            TableRef::Path(p) => p.split('/').next_back().unwrap_or(p).to_string(),
            TableRef::Catalog { namespace, name } => {
                format!("{}.{}", namespace.join("."), name)
            }
        }
    }
}

/// Catalog client that can load tables from different sources
pub struct CatalogClient {
    config: Option<CatalogConfig>,
    rest_client: Option<RestCatalogClient>,
}

impl CatalogClient {
    /// Create a new catalog client with optional configuration
    pub async fn new(config: Option<CatalogConfig>) -> Result<Self> {
        let rest_client = if let Some(ref cfg) = config {
            if matches!(cfg.catalog_type, CatalogType::Rest) {
                Some(RestCatalogClient::new(cfg).await?)
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self {
            config,
            rest_client,
        })
    }

    /// Create a client for direct path access (no catalog)
    pub fn direct() -> Self {
        Self {
            config: None,
            rest_client: None,
        }
    }

    /// Check if this client uses a catalog
    pub fn uses_catalog(&self) -> bool {
        self.config.is_some()
    }

    /// Get the catalog configuration
    pub fn config(&self) -> Option<&CatalogConfig> {
        self.config.as_ref()
    }

    /// Load a table by reference
    pub async fn load_table(&self, table_ref: &TableRef) -> Result<Table> {
        match table_ref {
            TableRef::Path(_path) => Err(Error::UnsupportedFeature {
                feature:
                    "Direct path loading through CatalogClient. Use IcebergMetadataService instead."
                        .to_string(),
            }),
            TableRef::Catalog { namespace, name } => {
                let client = self.rest_client.as_ref().ok_or_else(|| Error::NoCatalog)?;
                client.load_table(namespace, name).await
            }
        }
    }

    /// List namespaces in the catalog
    pub async fn list_namespaces(&self, parent: Option<&[String]>) -> Result<Vec<Vec<String>>> {
        let client = self.rest_client.as_ref().ok_or_else(|| Error::NoCatalog)?;
        client.list_namespaces(parent).await
    }

    /// List tables in a namespace
    pub async fn list_tables(&self, namespace: &[String]) -> Result<Vec<String>> {
        let client = self.rest_client.as_ref().ok_or_else(|| Error::NoCatalog)?;
        client.list_tables(namespace).await
    }
}
