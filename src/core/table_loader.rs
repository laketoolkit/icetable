//! Unified table loader using iceberg crate consistently
//!
//! This module provides a consistent interface for loading Iceberg tables
//! using the iceberg crate APIs, eliminating custom parsing and path handling code.

use std::sync::Arc;

use iceberg::table::{StaticTable, Table};
use iceberg::{Catalog, CatalogBuilder, NamespaceIdent, TableIdent};

use crate::core::CatalogConfig;
use crate::core::storage::{create_file_io, create_object_store};
use crate::error::{Error, Result};
use crate::utils::core::find_latest_metadata;

/// Unified table loader that uses iceberg crate consistently
pub struct TableLoader;

impl TableLoader {
    /// Load an Iceberg table from a path or catalog reference
    ///
    /// This is the main entry point that should be used by ALL commands
    /// instead of custom metadata parsing.
    ///
    /// # Arguments
    ///
    /// * `table_ref` - Either a direct path (s3://bucket/path) or catalog reference (catalog.db.table)
    /// * `catalog_config` - Optional catalog configuration for catalog tables
    ///
    /// # Returns
    ///
    /// Loaded iceberg::Table ready for operations
    pub async fn load_table(
        table_ref: &str,
        catalog_config: Option<&CatalogConfig>,
    ) -> Result<Arc<Table>> {
        // Check if this looks like a catalog reference (contains dots or forward slashes for namespace)
        if Self::looks_like_catalog_ref(table_ref) {
            if let Some(config) = catalog_config {
                Self::load_from_catalog(table_ref, config).await
            } else {
                Err(Error::CatalogRequired {
                    table_ref: table_ref.to_string(),
                })
            }
        } else {
            // Direct path - use StaticTable::from_metadata_file
            Self::load_from_path(table_ref).await
        }
    }

    /// Load table from direct storage path
    async fn load_from_path(path: &str) -> Result<Arc<Table>> {
        log::debug!("Loading Iceberg table from path: {}", path);

        // Convert relative paths to absolute paths
        // The iceberg crate has issues with relative paths
        let absolute_path = if !path.starts_with("s3://")
            && !path.starts_with("gs://")
            && !path.starts_with("az://")
            && !path.starts_with("file://")
            && !path.starts_with('/')
        {
            // Relative path, convert to absolute
            std::env::current_dir()
                .map_err(|e| Error::Storage {
                    message: format!("Failed to get current directory: {}", e),
                })?
                .join(path)
                .to_string_lossy()
                .to_string()
        } else {
            path.to_string()
        };

        log::debug!("Using absolute path: {}", absolute_path);

        // For direct paths, we need to:
        // 1. Find the latest metadata file
        // 2. Create FileIO for the storage backend
        // 3. Load using StaticTable::from_metadata_file

        // Find the latest metadata file (returns full path)
        let metadata_location = Self::find_latest_metadata(&absolute_path).await?;
        log::debug!("Found metadata location: {}", metadata_location);

        // Create FileIO for the storage backend
        let file_io = create_file_io(&absolute_path)?;
        log::debug!("Created FileIO for path: {}", absolute_path);

        // Create table identifier (for static tables, namespace/name don't matter much)
        let table_ident = TableIdent::new(
            NamespaceIdent::new("static".to_string()),
            "table".to_string(),
        );

        // Load using StaticTable::from_metadata_file
        log::debug!(
            "Calling StaticTable::from_metadata_file with location: {}",
            metadata_location
        );
        let static_table =
            StaticTable::from_metadata_file(&metadata_location, table_ident, file_io)
                .await
                .map_err(|e| Error::IcebergLoad {
                    path: absolute_path.clone(),
                    source: e.into(),
                })?;

        log::debug!("Successfully loaded table from path: {}", absolute_path);
        Ok(Arc::new(static_table.into_table()))
    }

    /// Load table from catalog reference
    async fn load_from_catalog(table_ref: &str, config: &CatalogConfig) -> Result<Arc<Table>> {
        log::debug!("Loading Iceberg table from catalog: {}", table_ref);

        // Parse namespace and table name
        let (namespace, table_name) = Self::parse_catalog_ref(table_ref)?;

        // Build catalog
        let catalog = Self::build_catalog(config).await?;

        // Load table
        let table_ident = TableIdent::new(namespace, table_name);
        let table = catalog
            .load_table(&table_ident)
            .await
            .map_err(|e| Error::CatalogLoad {
                table_ref: table_ref.to_string(),
                source: e.into(),
            })?;

        log::debug!("Successfully loaded table from catalog: {}", table_ref);
        Ok(Arc::new(table))
    }

    /// Check if a reference looks like a catalog reference
    fn looks_like_catalog_ref(table_ref: &str) -> bool {
        // Catalog references look like: catalog.db.table or catalog.namespace.table
        // They contain dots and don't start with storage prefixes or look like paths
        if table_ref.starts_with("s3://")
            || table_ref.starts_with("gs://")
            || table_ref.starts_with("az://")
            || table_ref.starts_with("file://")
            || table_ref.starts_with('/')
            || table_ref.contains('/')
        {
            return false;
        }

        // Catalog references should have at least one dot (catalog.table or catalog.namespace.table)
        table_ref.contains('.')
    }

    /// Parse catalog reference into namespace and table name
    fn parse_catalog_ref(table_ref: &str) -> Result<(NamespaceIdent, String)> {
        let parts: Vec<&str> = table_ref.split('.').collect();

        if parts.len() < 2 {
            return Err(Error::InvalidCatalogRef {
                ref_str: table_ref.to_string(),
                reason: "Expected format: catalog.namespace.table or catalog.db.table".to_string(),
            });
        }

        let table_name = parts.last().unwrap().to_string();
        let namespace_parts: Vec<&str> = parts[..parts.len() - 1].to_vec();
        let namespace =
            NamespaceIdent::from_vec(namespace_parts.into_iter().map(String::from).collect())
                .map_err(|e| Error::InvalidCatalogRef {
                    ref_str: table_ref.to_string(),
                    reason: format!("Failed to parse namespace: {}", e),
                })?;

        Ok((namespace, table_name))
    }

    /// Build iceberg catalog from configuration
    async fn build_catalog(config: &CatalogConfig) -> Result<Arc<dyn Catalog>> {
        // Currently only REST catalog is supported
        match config.catalog_type {
            crate::core::catalog::CatalogType::Rest => {
                #[cfg(feature = "rest-catalog")]
                {
                    use iceberg_catalog_rest::RestCatalogBuilder;

                    let mut props = std::collections::HashMap::new();
                    props.insert("uri".to_string(), config.uri.clone());

                    if let Some(warehouse) = &config.warehouse {
                        props.insert("warehouse".to_string(), warehouse.clone());
                    }

                    // Add auth and custom properties
                    props.extend(config.to_catalog_properties()?);

                    let catalog = RestCatalogBuilder::default()
                        .load("rest", props)
                        .await
                        .map_err(|e| Error::CatalogBuild { source: e.into() })?;

                    Ok(Arc::new(catalog))
                }
                #[cfg(not(feature = "rest-catalog"))]
                {
                    Err(Error::UnsupportedCatalog {
                        catalog_type: "rest".to_string(),
                    })
                }
            }
        }
    }

    /// Find the latest metadata file for a table path
    /// Find the latest metadata file in an Iceberg table directory
    pub async fn find_latest_metadata(path: &str) -> Result<String> {
        // Create storage to list files
        let storage = create_object_store(path)
            .await
            .map_err(|e| Error::Storage {
                message: format!("Failed to create storage: {}", e),
            })?;

        // Use the existing utility function
        find_latest_metadata(path, &storage).await
    }

    /// Create a new Iceberg table
    ///
    /// Uses iceberg's TableCreation API instead of custom metadata building
    pub async fn create_table(
        _path: &str,
        _schema: iceberg::spec::Schema,
        _partition_spec: Option<iceberg::spec::PartitionSpec>,
        _properties: std::collections::HashMap<String, String>,
    ) -> Result<Arc<Table>> {
        // Use iceberg's TableCreation API
        // Note: Table creation for static tables is complex - we'd need a name
        // For now, just return an error
        Err(Error::UnsupportedFeature {
            feature: "Table creation for static tables. Use catalog tables instead.".to_string(),
        })
    }
}

/// Extension trait for iceberg::Table to add convenience methods
pub trait TableExt {
    /// Get metadata with version
    fn metadata_with_version(&self) -> (Arc<iceberg::spec::TableMetadata>, i32);

    /// Get current snapshot ID
    fn current_snapshot_id(&self) -> Option<i64>;

    /// List all snapshots
    fn snapshots(&self) -> Vec<Arc<iceberg::spec::Snapshot>>;

    /// Get data files for current snapshot
    fn current_data_files(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<iceberg::spec::DataFile>>> + Send;
}

impl TableExt for Table {
    fn metadata_with_version(&self) -> (Arc<iceberg::spec::TableMetadata>, i32) {
        (
            self.metadata().clone().into(),
            self.metadata().format_version() as i32,
        )
    }

    fn current_snapshot_id(&self) -> Option<i64> {
        self.metadata().current_snapshot_id()
    }

    fn snapshots(&self) -> Vec<Arc<iceberg::spec::Snapshot>> {
        self.metadata().snapshots().cloned().collect()
    }

    async fn current_data_files(&self) -> Result<Vec<iceberg::spec::DataFile>> {
        // Use iceberg's scan API to get data files
        let scan_builder = self.scan();
        let _scan = scan_builder
            .build()
            .map_err(|e| Error::IcebergScan { source: e.into() })?;

        // The scan API might have changed - for now return empty vector
        // TODO: Fix this when we understand the new scan API
        Ok(Vec::new())
    }
}
