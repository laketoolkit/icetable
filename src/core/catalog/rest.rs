//! REST Catalog client implementation
//!
//! Uses iceberg-catalog-rest crate to interact with REST catalogs
//! like Nessie, Polaris, Tabular, Unity Catalog, etc.

use super::CatalogConfig;
use crate::error::{Error, Result};
use iceberg::spec::Schema;
use iceberg::table::Table;
use iceberg::{Catalog, CatalogBuilder, NamespaceIdent, TableCreation, TableIdent};
use iceberg_catalog_rest::RestCatalogBuilder;
use std::collections::HashMap;
use std::sync::Arc;

/// REST Catalog client wrapper
pub struct RestCatalogClient {
    catalog: Arc<dyn Catalog>,
}

impl RestCatalogClient {
    /// Create a new REST catalog client from configuration
    pub async fn new(config: &CatalogConfig) -> Result<Self> {
        let mut props = HashMap::new();
        props.insert("uri".to_string(), config.uri.clone());

        if let Some(ref warehouse) = config.warehouse {
            props.insert("warehouse".to_string(), warehouse.clone());
        }

        if let Some(credential) = config.resolve_credential()? {
            props.insert("credential".to_string(), credential);
        }

        // Add any additional properties
        for (k, v) in &config.properties {
            props.insert(k.clone(), v.clone());
        }

        let catalog = RestCatalogBuilder::default()
            .load("rest", props)
            .await
            .map_err(|e| Error::General(format!("Failed to create REST catalog: {}", e)))?;

        Ok(Self {
            catalog: Arc::new(catalog),
        })
    }

    /// List namespaces in the catalog
    pub async fn list_namespaces(&self, parent: Option<&[String]>) -> Result<Vec<Vec<String>>> {
        let parent_ident = parent.map(|p| {
            NamespaceIdent::from_vec(p.to_vec()).expect("Invalid namespace")
        });

        let namespaces = self
            .catalog
            .list_namespaces(parent_ident.as_ref())
            .await
            .map_err(|e| Error::General(format!("Failed to list namespaces: {}", e)))?;

        Ok(namespaces
            .into_iter()
            .map(|ns| ns.as_ref().to_vec())
            .collect())
    }

    /// List tables in a namespace
    pub async fn list_tables(&self, namespace: &[String]) -> Result<Vec<String>> {
        let ns_ident = NamespaceIdent::from_vec(namespace.to_vec())
            .map_err(|e| Error::General(format!("Invalid namespace: {}", e)))?;

        let tables = self
            .catalog
            .list_tables(&ns_ident)
            .await
            .map_err(|e| Error::General(format!("Failed to list tables: {}", e)))?;

        Ok(tables.into_iter().map(|t| t.name().to_string()).collect())
    }

    /// Load a table from the catalog
    pub async fn load_table(&self, namespace: &[String], name: &str) -> Result<Table> {
        let ns_ident = NamespaceIdent::from_vec(namespace.to_vec())
            .map_err(|e| Error::General(format!("Invalid namespace: {}", e)))?;

        let table_ident = TableIdent::new(ns_ident, name.to_string());

        self.catalog
            .load_table(&table_ident)
            .await
            .map_err(|e| Error::General(format!("Failed to load table '{}': {}", name, e)))
    }

    /// Check if a table exists
    pub async fn table_exists(&self, namespace: &[String], name: &str) -> Result<bool> {
        let ns_ident = NamespaceIdent::from_vec(namespace.to_vec())
            .map_err(|e| Error::General(format!("Invalid namespace: {}", e)))?;

        let table_ident = TableIdent::new(ns_ident, name.to_string());

        self.catalog
            .table_exists(&table_ident)
            .await
            .map_err(|e| Error::General(format!("Failed to check table existence: {}", e)))
    }

    /// Get the underlying catalog for advanced operations
    pub fn inner(&self) -> &Arc<dyn Catalog> {
        &self.catalog
    }

    /// Create a namespace in the catalog
    pub async fn create_namespace(
        &self,
        namespace: &[String],
        properties: HashMap<String, String>,
    ) -> Result<()> {
        let ns_ident = NamespaceIdent::from_vec(namespace.to_vec())
            .map_err(|e| Error::General(format!("Invalid namespace: {}", e)))?;

        self.catalog
            .create_namespace(&ns_ident, properties)
            .await
            .map_err(|e| Error::General(format!("Failed to create namespace: {}", e)))?;

        Ok(())
    }

    /// Drop a namespace from the catalog
    pub async fn drop_namespace(&self, namespace: &[String]) -> Result<()> {
        let ns_ident = NamespaceIdent::from_vec(namespace.to_vec())
            .map_err(|e| Error::General(format!("Invalid namespace: {}", e)))?;

        self.catalog
            .drop_namespace(&ns_ident)
            .await
            .map_err(|e| Error::General(format!("Failed to drop namespace: {}", e)))
    }

    /// Create a table in the catalog
    pub async fn create_table(
        &self,
        namespace: &[String],
        name: &str,
        schema: Schema,
        location: Option<&str>,
        properties: HashMap<String, String>,
    ) -> Result<Table> {
        let ns_ident = NamespaceIdent::from_vec(namespace.to_vec())
            .map_err(|e| Error::General(format!("Invalid namespace: {}", e)))?;

        let creation = TableCreation::builder()
            .name(name.to_string())
            .schema(schema)
            .location_opt(location.map(|s| s.to_string()))
            .properties(properties)
            .build();

        self.catalog
            .create_table(&ns_ident, creation)
            .await
            .map_err(|e| Error::General(format!("Failed to create table '{}': {}", name, e)))
    }

    /// Drop a table from the catalog
    pub async fn drop_table(&self, namespace: &[String], name: &str, _purge: bool) -> Result<()> {
        let ns_ident = NamespaceIdent::from_vec(namespace.to_vec())
            .map_err(|e| Error::General(format!("Invalid namespace: {}", e)))?;

        let table_ident = TableIdent::new(ns_ident, name.to_string());

        // Note: The Iceberg Rust catalog API doesn't have a separate purge option.
        // The purge behavior is typically handled by the catalog implementation.
        self.catalog
            .drop_table(&table_ident)
            .await
            .map_err(|e| Error::General(format!("Failed to drop table '{}': {}", name, e)))
    }
}
