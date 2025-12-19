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

    // Try to extract the error message from JSON if present
    // Pattern: "message":"<actual message>"
    if let Some(json_msg) = extract_json_message(&msg) {
        // Check if it's a warehouse not found error
        if json_msg.contains("Unable to find warehouse") {
            return format_warehouse_not_found_error(&json_msg);
        }
        return json_msg;
    }

    // Check for warehouse not found in raw message
    if msg.contains("Unable to find warehouse") {
        return format_warehouse_not_found_error(&msg);
    }

    // Check for HTTP status codes in the message and provide clearer messages
    if msg.contains("status: 401") || msg.contains("Unauthorized") {
        return "Authentication required - run 'icetable admin auth login'".to_string();
    }
    if msg.contains("status: 403") || msg.contains("Forbidden") {
        return "Access denied - check your credentials and permissions".to_string();
    }
    if msg.contains("status: 404") {
        return "Resource not found".to_string();
    }
    if msg.contains("status: 500") || msg.contains("Internal Server Error") {
        return "Catalog server error - try again later".to_string();
    }
    if msg.contains("status: 502") || msg.contains("Bad Gateway") {
        return "Catalog server unavailable (502)".to_string();
    }
    if msg.contains("status: 503") || msg.contains("Service Unavailable") {
        return "Catalog server unavailable (503)".to_string();
    }
    if msg.contains("Connection refused")
        || msg.contains("connection refused")
        || msg.contains("error sending request")
    {
        return "Cannot connect to catalog - is the server running?".to_string();
    }

    // Pattern: "Unexpected => <message>"
    if let Some(pos) = msg.find(" => ") {
        let clean_msg = &msg[pos + 4..];
        // Capitalize first letter and clean up
        return clean_catalog_message(clean_msg);
    }

    msg
}

/// Format a user-friendly error message when warehouse is not found
fn format_warehouse_not_found_error(msg: &str) -> String {
    // Try to extract warehouse name from message like "Unable to find warehouse 'foo'"
    let warehouse_name = if let Some(start) = msg.find("warehouse") {
        let after = &msg[start + 9..];
        // Look for quoted name or just take the next word
        if let Some(quote_start) = after.find('\'') {
            let after_quote = &after[quote_start + 1..];
            after_quote
                .find('\'')
                .map(|quote_end| &after_quote[..quote_end])
        } else {
            // Try unquoted - take first word
            after.split_whitespace().next()
        }
    } else {
        None
    };

    let wh_display = warehouse_name.unwrap_or("(unknown)");

    format!(
        "Warehouse '{}' not found\n\n\
         Hint:\n  \
         - List warehouses: icetable admin warehouse ls\n  \
         - Create warehouse: icetable admin warehouse create <name> --location s3://...\n  \
         - Use different: icetable -w <warehouse> ls",
        wh_display
    )
}

/// Try to extract the "message" field from a JSON error response embedded in the error string
fn extract_json_message(msg: &str) -> Option<String> {
    // Look for pattern: "message":"<message>"
    let pattern = "\"message\":\"";
    let start = msg.find(pattern)? + pattern.len();
    let end = msg[start..].find('"')? + start;
    let message = &msg[start..end];

    if message.is_empty() {
        return None;
    }

    // Capitalize first letter
    let mut chars = message.chars();
    Some(
        chars
            .next()?
            .to_uppercase()
            .chain(chars)
            .collect::<String>(),
    )
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
    /// Catalog name for error messages (defaults to URI if not set)
    name: String,
}

impl RestCatalogClient {
    /// Create a new REST catalog client from configuration
    pub async fn new(config: &CatalogConfig) -> Result<Self> {
        Self::with_name(config, None).await
    }

    /// Create a new REST catalog client with a specific name for error messages
    pub async fn with_name(config: &CatalogConfig, name: Option<&str>) -> Result<Self> {
        let mut props = HashMap::new();
        props.insert("uri".to_string(), config.uri.clone());

        if let Some(ref warehouse) = config.warehouse {
            props.insert("warehouse".to_string(), warehouse.clone());
        }

        // Add auth and custom properties
        // Use credentials.yaml if name is provided
        let auth_props = if let Some(catalog_name) = name {
            config.to_catalog_properties_with_credentials(catalog_name)?
        } else {
            config.to_catalog_properties()?
        };
        props.extend(auth_props);

        let catalog = RestCatalogBuilder::default()
            .load("rest", props)
            .await
            .map_err(|e| Error::CatalogOperation {
                message: clean_iceberg_error(&e),
            })?;

        Ok(Self {
            catalog: Arc::new(catalog),
            name: name.unwrap_or(&config.uri).to_string(),
        })
    }

    /// List namespaces in the catalog
    pub async fn list_namespaces(&self, parent: Option<&[String]>) -> Result<Vec<Vec<String>>> {
        let parent_ident = parent
            .map(|p| {
                NamespaceIdent::from_vec(p.to_vec()).map_err(|e| Error::Configuration {
                    message: format!("Invalid namespace: {}", e),
                })
            })
            .transpose()?;

        let namespaces = self
            .catalog
            .list_namespaces(parent_ident.as_ref())
            .await
            .map_err(|e| Error::CatalogOperation {
                message: format!("{} in catalog '{}'", clean_iceberg_error(&e), self.name),
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
                message: e.to_string(),
            })?;

        let tables =
            self.catalog
                .list_tables(&ns_ident)
                .await
                .map_err(|e| Error::CatalogOperation {
                    message: format!(
                        "{} in namespace '{}' (catalog '{}')",
                        clean_iceberg_error(&e),
                        namespace.join("."),
                        self.name
                    ),
                })?;

        Ok(tables.into_iter().map(|t| t.name().to_string()).collect())
    }

    /// Load a table from the catalog
    pub async fn load_table(&self, namespace: &[String], name: &str) -> Result<Table> {
        let ns_ident =
            NamespaceIdent::from_vec(namespace.to_vec()).map_err(|e| Error::InvalidNamespace {
                value: namespace.join("."),
                message: e.to_string(),
            })?;

        let table_ident = TableIdent::new(ns_ident, name.to_string());

        self.catalog
            .load_table(&table_ident)
            .await
            .map_err(|e| Error::TableNotFound {
                path: format!(
                    "'{}' in namespace '{}' (catalog '{}') - {}",
                    name,
                    namespace.join("."),
                    self.name,
                    clean_iceberg_error(&e)
                ),
            })
    }

    /// Check if a table exists
    pub async fn table_exists(&self, namespace: &[String], name: &str) -> Result<bool> {
        let ns_ident =
            NamespaceIdent::from_vec(namespace.to_vec()).map_err(|e| Error::InvalidNamespace {
                value: namespace.join("."),
                message: e.to_string(),
            })?;

        let table_ident = TableIdent::new(ns_ident, name.to_string());

        self.catalog
            .table_exists(&table_ident)
            .await
            .map_err(|e| Error::CatalogOperation {
                message: format!(
                    "{} for '{}' in namespace '{}' (catalog '{}')",
                    clean_iceberg_error(&e),
                    name,
                    namespace.join("."),
                    self.name
                ),
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
                message: e.to_string(),
            })?;

        self.catalog
            .create_namespace(&ns_ident, properties)
            .await
            .map_err(|e| Error::CatalogOperation {
                message: format!(
                    "{} '{}' in catalog '{}'",
                    clean_iceberg_error(&e),
                    namespace.join("."),
                    self.name
                ),
            })?;

        Ok(())
    }

    /// Delete a namespace from the catalog
    pub async fn delete_namespace(&self, namespace: &[String]) -> Result<()> {
        let ns_ident =
            NamespaceIdent::from_vec(namespace.to_vec()).map_err(|e| Error::InvalidNamespace {
                value: namespace.join("."),
                message: e.to_string(),
            })?;

        self.catalog
            .drop_namespace(&ns_ident)
            .await
            .map_err(|e| Error::CatalogOperation {
                message: format!(
                    "{} '{}' in catalog '{}'",
                    clean_iceberg_error(&e),
                    namespace.join("."),
                    self.name
                ),
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
                message: e.to_string(),
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
                message: format!(
                    "{} '{}' in namespace '{}' (catalog '{}')",
                    clean_iceberg_error(&e),
                    name,
                    namespace.join("."),
                    self.name
                ),
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
                message: e.to_string(),
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
                message: format!(
                    "{} '{}' in namespace '{}' (catalog '{}')",
                    clean_iceberg_error(&e),
                    name,
                    namespace.join("."),
                    self.name
                ),
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
