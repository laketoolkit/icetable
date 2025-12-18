//! Catalog management operations
//!
//! This module provides vendor-specific management operations for catalogs,
//! such as creating/deleting warehouses in Polaris, managing roles, etc.
//!
//! These operations are NOT part of the standard Iceberg REST Catalog API,
//! but are specific to each catalog implementation.

pub mod polaris;
mod traits;

pub use traits::{CatalogManagement, UnsupportedManagement};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::config::CatalogConfig;
use crate::error::Result;

// =============================================================================
// Data Types
// =============================================================================

/// Storage type for warehouse locations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum StorageType {
    /// Amazon S3
    #[default]
    S3,
    /// Google Cloud Storage
    Gcs,
    /// Azure Blob Storage
    Azure,
    /// Local filesystem
    File,
}

impl std::fmt::Display for StorageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageType::S3 => write!(f, "S3"),
            StorageType::Gcs => write!(f, "GCS"),
            StorageType::Azure => write!(f, "AZURE"),
            StorageType::File => write!(f, "FILE"),
        }
    }
}

impl std::str::FromStr for StorageType {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "S3" => Ok(StorageType::S3),
            "GCS" => Ok(StorageType::Gcs),
            "AZURE" => Ok(StorageType::Azure),
            "FILE" => Ok(StorageType::File),
            _ => Err(format!("Unknown storage type: {}", s)),
        }
    }
}

/// Warehouse/Catalog information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Warehouse {
    /// Warehouse name (unique identifier)
    pub name: String,
    /// Type of warehouse (INTERNAL or EXTERNAL)
    #[serde(rename = "type")]
    pub warehouse_type: WarehouseType,
    /// Storage type (S3, GCS, Azure, File)
    pub storage_type: StorageType,
    /// Default base location for tables
    pub default_base_location: String,
    /// Allowed storage locations
    #[serde(default)]
    pub allowed_locations: Vec<String>,
    /// Additional properties
    #[serde(default)]
    pub properties: HashMap<String, String>,
}

/// Type of warehouse
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum WarehouseType {
    /// Internal warehouse (managed by the catalog)
    #[default]
    Internal,
    /// External warehouse (references existing data)
    External,
}

impl std::fmt::Display for WarehouseType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WarehouseType::Internal => write!(f, "INTERNAL"),
            WarehouseType::External => write!(f, "EXTERNAL"),
        }
    }
}

/// Request to create a new warehouse
#[derive(Debug, Clone)]
pub struct CreateWarehouseRequest {
    /// Warehouse name
    pub name: String,
    /// Type of warehouse
    pub warehouse_type: WarehouseType,
    /// Storage type (can be inferred from location if not set)
    pub storage_type: Option<StorageType>,
    /// Default base location for tables
    pub default_base_location: String,
    /// Allowed storage locations (optional)
    pub allowed_locations: Vec<String>,
    /// Additional catalog properties
    pub properties: HashMap<String, String>,
    /// Storage configuration (endpoint, credentials, etc.)
    /// Keys are vendor-specific (e.g., "s3.endpoint", "s3.access-key-id")
    /// Uses serde_json::Value to preserve original types (bool, number, string)
    pub storage_config: HashMap<String, serde_json::Value>,
}

impl CreateWarehouseRequest {
    /// Create a new warehouse request with minimal required fields
    pub fn new(name: impl Into<String>, location: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            warehouse_type: WarehouseType::Internal,
            storage_type: None,
            default_base_location: location.into(),
            allowed_locations: Vec::new(),
            properties: HashMap::new(),
            storage_config: HashMap::new(),
        }
    }

    /// Set warehouse type
    pub fn with_type(mut self, warehouse_type: WarehouseType) -> Self {
        self.warehouse_type = warehouse_type;
        self
    }

    /// Set storage type explicitly
    pub fn with_storage_type(mut self, storage_type: StorageType) -> Self {
        self.storage_type = Some(storage_type);
        self
    }

    /// Add allowed locations
    pub fn with_allowed_locations(mut self, locations: Vec<String>) -> Self {
        self.allowed_locations = locations;
        self
    }

    /// Add a catalog property
    pub fn with_property(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.properties.insert(key.into(), value.into());
        self
    }

    /// Add a storage config entry (string value)
    pub fn with_storage_config(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.storage_config
            .insert(key.into(), serde_json::Value::String(value.into()));
        self
    }

    /// Add a storage config entry with a JSON value (preserves type)
    pub fn with_storage_config_value(
        mut self,
        key: impl Into<String>,
        value: serde_json::Value,
    ) -> Self {
        self.storage_config.insert(key.into(), value);
        self
    }

    /// Set all storage config from a HashMap of JSON values
    pub fn with_storage_config_map(mut self, config: HashMap<String, serde_json::Value>) -> Self {
        self.storage_config = config;
        self
    }

    /// Infer storage type from location if not explicitly set
    pub fn inferred_storage_type(&self) -> StorageType {
        self.storage_type.unwrap_or_else(|| {
            let loc = &self.default_base_location;
            if loc.starts_with("s3://") || loc.starts_with("s3a://") {
                StorageType::S3
            } else if loc.starts_with("gs://") {
                StorageType::Gcs
            } else if loc.starts_with("az://")
                || loc.starts_with("abfs://")
                || loc.starts_with("abfss://")
            {
                StorageType::Azure
            } else {
                StorageType::File
            }
        })
    }
}

// =============================================================================
// Factory
// =============================================================================

/// Create a management client based on catalog configuration
///
/// Automatically detects the catalog type and returns the appropriate
/// management implementation. Returns `UnsupportedManagement` for catalogs
/// that don't have management API support.
///
/// If catalog_name is provided, credentials will be loaded from credentials.yaml.
pub async fn create_management_client(
    config: &CatalogConfig,
) -> Result<Box<dyn CatalogManagement>> {
    create_management_client_with_name(config, None).await
}

/// Create a management client with explicit catalog name
///
/// The catalog_name is used to look up credentials from credentials.yaml.
pub async fn create_management_client_with_name(
    config: &CatalogConfig,
    catalog_name: Option<&str>,
) -> Result<Box<dyn CatalogManagement>> {
    if is_polaris(config) {
        Ok(Box::new(
            polaris::PolarisManagement::new(config, catalog_name).await?,
        ))
    } else {
        Ok(Box::new(UnsupportedManagement::new("rest")))
    }
}

/// Detect if a catalog is Polaris based on its configuration
fn is_polaris(config: &CatalogConfig) -> bool {
    // Heuristic: Polaris has /api/catalog in the URI
    config.uri.contains("/api/catalog")
        || config
            .properties
            .get("catalog-impl")
            .is_some_and(|v| v.contains("polaris"))
}
