//! Warehouse management service
//!
//! Provides high-level operations for warehouse management that may involve
//! multiple catalog operations (e.g., force delete with cascade).
//!
//! This service follows the same pattern as AuthService - it coordinates
//! complex operations and returns structured results for the CLI to format.

use crate::config::Config;
use crate::core::catalog::RestCatalogClient;
use crate::error::{Error, Result};

/// Progress of a force delete operation
#[derive(Debug, Clone, Default)]
pub struct DeleteProgress {
    /// Tables that were successfully deleted (namespace, table_name)
    pub tables_deleted: Vec<(String, String)>,
    /// Tables that failed to delete (namespace, table_name, error_message)
    pub tables_failed: Vec<(String, String, String)>,
    /// Namespaces that were successfully deleted
    pub namespaces_deleted: Vec<String>,
    /// Namespaces that failed to delete (namespace, error_message)
    pub namespaces_failed: Vec<(String, String)>,
}

impl DeleteProgress {
    /// Check if there were any failures
    pub fn has_errors(&self) -> bool {
        !self.tables_failed.is_empty() || !self.namespaces_failed.is_empty()
    }

    /// Total items deleted
    pub fn total_deleted(&self) -> usize {
        self.tables_deleted.len() + self.namespaces_deleted.len()
    }
}

/// Service for warehouse operations that require complex logic
pub struct WarehouseService;

impl WarehouseService {
    /// Force delete all contents of a warehouse (namespaces and tables)
    ///
    /// This is a destructive operation that deletes all tables and namespaces
    /// in the warehouse before the warehouse itself can be deleted.
    ///
    /// Returns structured progress so the CLI can format output appropriately.
    pub async fn force_delete_contents(
        catalog_name: &str,
        warehouse_name: &str,
    ) -> Result<DeleteProgress> {
        let mut progress = DeleteProgress::default();

        // Load config and create a catalog client with the warehouse
        let config = Config::load()?;
        let mut catalog_config =
            config
                .catalogs
                .get(catalog_name)
                .cloned()
                .ok_or_else(|| Error::CatalogNotFound {
                    name: catalog_name.to_string(),
                })?;
        catalog_config.warehouse = Some(warehouse_name.to_string());

        let rest_client =
            RestCatalogClient::with_name(&catalog_config, Some(catalog_name)).await?;

        // List all namespaces
        let namespaces = match rest_client.list_namespaces(None).await {
            Ok(ns) => ns,
            Err(_) => return Ok(progress), // No namespaces or error, continue with delete
        };

        // Delete contents of each namespace
        for ns in &namespaces {
            let ns_name = ns.join(".");

            // List and delete tables in namespace
            if let Ok(tables) = rest_client.list_tables(ns).await {
                for table in &tables {
                    match rest_client.delete_table(ns, table, true).await {
                        Ok(_) => {
                            progress
                                .tables_deleted
                                .push((ns_name.clone(), table.clone()));
                        }
                        Err(e) => {
                            progress
                                .tables_failed
                                .push((ns_name.clone(), table.clone(), e.to_string()));
                        }
                    }
                }
            }

            // Delete namespace
            match rest_client.delete_namespace(ns).await {
                Ok(_) => {
                    progress.namespaces_deleted.push(ns_name);
                }
                Err(e) => {
                    progress.namespaces_failed.push((ns_name, e.to_string()));
                }
            }
        }

        Ok(progress)
    }
}
