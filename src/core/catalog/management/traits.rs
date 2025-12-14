//! Catalog management trait definition
//!
//! Defines the `CatalogManagement` trait for vendor-specific operations.

use async_trait::async_trait;

use super::{CreateWarehouseRequest, Warehouse};
use crate::error::{Error, Result};

/// Trait for catalog-specific management operations
///
/// This trait defines operations that are NOT part of the standard Iceberg
/// REST Catalog API, but are specific to each catalog implementation
/// (Polaris, Nessie, Unity, etc.).
///
/// # Example
/// ```ignore
/// let client = create_management_client(&config).await?;
/// if client.supports_management() {
///     let warehouses = client.list_warehouses().await?;
///     for wh in warehouses {
///         println!("{}: {}", wh.name, wh.default_base_location);
///     }
/// }
/// ```
#[async_trait]
pub trait CatalogManagement: Send + Sync {
    /// Get the catalog type name (for error messages)
    fn catalog_type(&self) -> &'static str;

    /// Check if this catalog supports management operations
    fn supports_management(&self) -> bool;

    // =========================================================================
    // Warehouse Operations
    // =========================================================================

    /// List all warehouses in the catalog
    async fn list_warehouses(&self) -> Result<Vec<Warehouse>>;

    /// Get a specific warehouse by name
    async fn get_warehouse(&self, name: &str) -> Result<Warehouse>;

    /// Create a new warehouse
    async fn create_warehouse(&self, request: CreateWarehouseRequest) -> Result<Warehouse>;

    /// Delete a warehouse by name
    async fn delete_warehouse(&self, name: &str) -> Result<()>;

    // =========================================================================
    // Future: Role Operations
    // =========================================================================
    // async fn list_roles(&self) -> Result<Vec<Role>>;
    // async fn create_role(&self, request: CreateRoleRequest) -> Result<Role>;
    // async fn delete_role(&self, name: &str) -> Result<()>;

    // =========================================================================
    // Future: Principal Operations
    // =========================================================================
    // async fn list_principals(&self) -> Result<Vec<Principal>>;
    // async fn create_principal(&self, request: CreatePrincipalRequest) -> Result<Principal>;
    // async fn delete_principal(&self, name: &str) -> Result<()>;
}

/// Default implementation for catalogs that don't support management operations
///
/// All operations return `Error::UnsupportedFeature`.
pub struct UnsupportedManagement {
    catalog_type: &'static str,
}

impl UnsupportedManagement {
    /// Create a new UnsupportedManagement for a catalog type
    pub fn new(catalog_type: &'static str) -> Self {
        Self { catalog_type }
    }

    fn unsupported_error(&self, operation: &str) -> Error {
        Error::UnsupportedFeature {
            feature: format!(
                "{} not supported for {} catalogs",
                operation, self.catalog_type
            ),
        }
    }
}

#[async_trait]
impl CatalogManagement for UnsupportedManagement {
    fn catalog_type(&self) -> &'static str {
        self.catalog_type
    }

    fn supports_management(&self) -> bool {
        false
    }

    async fn list_warehouses(&self) -> Result<Vec<Warehouse>> {
        Err(self.unsupported_error("Warehouse management"))
    }

    async fn get_warehouse(&self, _name: &str) -> Result<Warehouse> {
        Err(self.unsupported_error("Warehouse management"))
    }

    async fn create_warehouse(&self, _request: CreateWarehouseRequest) -> Result<Warehouse> {
        Err(self.unsupported_error("Warehouse management"))
    }

    async fn delete_warehouse(&self, _name: &str) -> Result<()> {
        Err(self.unsupported_error("Warehouse management"))
    }
}
