//! REST Catalog client implementation
//!
//! Uses iceberg-catalog-rest crate to interact with REST catalogs
//! like Nessie, Polaris, Tabular, Unity Catalog, etc.

use crate::core::config::CatalogConfig;
use crate::error::{Error, Result};
use iceberg::spec::Schema;
use iceberg::table::Table;
use iceberg::{Catalog, CatalogBuilder, NamespaceIdent, TableCreation, TableIdent};
use iceberg_catalog_rest::RestCatalogBuilder;
use std::collections::HashMap;
use std::sync::Arc;

/// Extract a clean error message from iceberg errors
/// Converts verbose errors like "Unexpected => Tried to create a namespace that already exists"
/// to cleaner messages like "Namespace already exists"
fn clean_iceberg_error(error: &iceberg::Error) -> String {
    let msg = error.to_string();

    // Pattern: "Unexpected => <message>"
    if let Some(pos) = msg.find(" => ") {
        let clean_msg = &msg[pos + 4..];
        // Capitalize first letter and clean up
        return clean_catalog_message(clean_msg);
    }

    msg
}

/// Clean up catalog error messages to be more natural
fn clean_catalog_message(msg: &str) -> String {
    let msg = msg.trim();

    // Common patterns to simplify
    let simplified = msg
        .replace(
            "Tried to create a namespace that already exists",
            "Namespace already exists",
        )
        .replace(
            "Tried to create a table under a namespace that does not exist",
            "Namespace does not exist",
        )
        .replace(
            "Tried to create a table that already exists",
            "Table already exists",
        )
        .replace(
            "Tried to drop a namespace that is not empty",
            "Namespace is not empty",
        )
        .replace(
            "Tried to drop a namespace that does not exist",
            "Namespace does not exist",
        )
        .replace(
            "Tried to drop a table that does not exist",
            "Table does not exist",
        )
        .replace(
            "Tried to load a table that does not exist",
            "Table does not exist",
        );

    // Capitalize first letter if needed
    let mut chars = simplified.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().chain(chars).collect(),
    }
}

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

        // Add auth and custom properties
        props.extend(config.to_catalog_properties()?);

        let catalog = RestCatalogBuilder::default()
            .load("rest", props)
            .await
            .map_err(|e| Error::CatalogOperation {
                message: clean_iceberg_error(&e),
            })?;

        Ok(Self {
            catalog: Arc::new(catalog),
        })
    }

    /// List namespaces in the catalog
    pub async fn list_namespaces(&self, parent: Option<&[String]>) -> Result<Vec<Vec<String>>> {
        let parent_ident =
            parent.map(|p| NamespaceIdent::from_vec(p.to_vec()).expect("Invalid namespace"));

        let namespaces = self
            .catalog
            .list_namespaces(parent_ident.as_ref())
            .await
            .map_err(|e| Error::CatalogOperation {
                message: clean_iceberg_error(&e),
            })?;

        Ok(namespaces
            .into_iter()
            .map(|ns| ns.as_ref().to_vec())
            .collect())
    }

    /// List tables in a namespace
    pub async fn list_tables(&self, namespace: &[String]) -> Result<Vec<String>> {
        let ns_ident =
            NamespaceIdent::from_vec(namespace.to_vec()).map_err(|e| Error::InvalidNamespace {
                value: namespace.join("."),
                reason: e.to_string(),
            })?;

        let tables =
            self.catalog
                .list_tables(&ns_ident)
                .await
                .map_err(|e| Error::CatalogOperation {
                    message: clean_iceberg_error(&e),
                })?;

        Ok(tables.into_iter().map(|t| t.name().to_string()).collect())
    }

    /// Load a table from the catalog
    pub async fn load_table(&self, namespace: &[String], name: &str) -> Result<Table> {
        let ns_ident =
            NamespaceIdent::from_vec(namespace.to_vec()).map_err(|e| Error::InvalidNamespace {
                value: namespace.join("."),
                reason: e.to_string(),
            })?;

        let table_ident = TableIdent::new(ns_ident, name.to_string());

        self.catalog
            .load_table(&table_ident)
            .await
            .map_err(|e| Error::TableNotFound {
                path: format!(
                    "{}.{} ({})",
                    namespace.join("."),
                    name,
                    clean_iceberg_error(&e)
                ),
            })
    }

    /// Check if a table exists
    pub async fn table_exists(&self, namespace: &[String], name: &str) -> Result<bool> {
        let ns_ident =
            NamespaceIdent::from_vec(namespace.to_vec()).map_err(|e| Error::InvalidNamespace {
                value: namespace.join("."),
                reason: e.to_string(),
            })?;

        let table_ident = TableIdent::new(ns_ident, name.to_string());

        self.catalog
            .table_exists(&table_ident)
            .await
            .map_err(|e| Error::CatalogOperation {
                message: clean_iceberg_error(&e),
            })
    }

    /// Get the underlying catalog for advanced operations
    pub fn inner(&self) -> &Arc<dyn Catalog> {
        &self.catalog
    }

    /// Get the underlying catalog as a reference to dyn Catalog
    pub fn catalog(&self) -> &dyn Catalog {
        self.catalog.as_ref()
    }

    /// Create a namespace in the catalog
    pub async fn create_namespace(
        &self,
        namespace: &[String],
        properties: HashMap<String, String>,
    ) -> Result<()> {
        let ns_ident =
            NamespaceIdent::from_vec(namespace.to_vec()).map_err(|e| Error::InvalidNamespace {
                value: namespace.join("."),
                reason: e.to_string(),
            })?;

        self.catalog
            .create_namespace(&ns_ident, properties)
            .await
            .map_err(|e| Error::CatalogOperation {
                message: clean_iceberg_error(&e),
            })?;

        Ok(())
    }

    /// Delete a namespace from the catalog
    pub async fn delete_namespace(&self, namespace: &[String]) -> Result<()> {
        let ns_ident =
            NamespaceIdent::from_vec(namespace.to_vec()).map_err(|e| Error::InvalidNamespace {
                value: namespace.join("."),
                reason: e.to_string(),
            })?;

        self.catalog
            .drop_namespace(&ns_ident)
            .await
            .map_err(|e| Error::CatalogOperation {
                message: clean_iceberg_error(&e),
            })
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
        let ns_ident =
            NamespaceIdent::from_vec(namespace.to_vec()).map_err(|e| Error::InvalidNamespace {
                value: namespace.join("."),
                reason: e.to_string(),
            })?;

        let creation = TableCreation::builder()
            .name(name.to_string())
            .schema(schema)
            .location_opt(location.map(|s| s.to_string()))
            .properties(properties)
            .build();

        self.catalog
            .create_table(&ns_ident, creation)
            .await
            .map_err(|e| Error::CatalogOperation {
                message: clean_iceberg_error(&e),
            })
    }

    /// Delete a table from the catalog
    ///
    /// If `purge` is true, also deletes all data files and metadata from storage.
    /// Note: iceberg-rs doesn't support purge via the catalog API, so we implement
    /// it by loading the table location first, then dropping from catalog, then
    /// deleting the storage location.
    pub async fn delete_table(&self, namespace: &[String], name: &str, purge: bool) -> Result<()> {
        let ns_ident =
            NamespaceIdent::from_vec(namespace.to_vec()).map_err(|e| Error::InvalidNamespace {
                value: namespace.join("."),
                reason: e.to_string(),
            })?;

        let table_ident = TableIdent::new(ns_ident, name.to_string());

        // If purge requested, get the table location first
        let table_location = if purge {
            match self.catalog.load_table(&table_ident).await {
                Ok(table) => Some(table.metadata().location().to_string()),
                Err(_) => None, // Table might not exist or be accessible
            }
        } else {
            None
        };

        // Drop from catalog
        self.catalog
            .drop_table(&table_ident)
            .await
            .map_err(|e| Error::CatalogOperation {
                message: clean_iceberg_error(&e),
            })?;

        // If purge requested and we have a location, delete storage
        if let Some(location) = table_location
            && let Err(e) = Self::purge_table_storage(&location).await
        {
            // Log warning but don't fail - catalog drop succeeded
            eprintln!("Warning: Failed to purge storage at {}: {}", location, e);
        }

        Ok(())
    }

    /// Delete all files at a table location (for purge)
    async fn purge_table_storage(location: &str) -> Result<()> {
        use crate::core::storage::{ObjectStoreExt, create_object_store, to_path};

        let store = create_object_store(location).await?;
        let prefix = to_path(location);

        // List all objects under the table location
        let objects = store.list_all(Some(&prefix)).await?;

        if objects.is_empty() {
            return Ok(());
        }

        // Delete all objects
        for meta in objects {
            store
                .delete(&meta.location)
                .await
                .map_err(|e| Error::CloudStorage {
                    provider: "object_store".to_string(),
                    message: format!("Failed to delete {}: {}", meta.location, e),
                    error_code: None,
                    http_status: None,
                })?;
        }

        Ok(())
    }
}
