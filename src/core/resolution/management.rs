//! Management client resolution
//!
//! Resolves catalog management clients from CLI context.

use crate::config::Config;
use crate::core::catalog::{CatalogManagement, create_management_client_with_name};
use crate::error::Result;

use super::CatalogContext;

/// Resolved management client with catalog name
pub struct ManagementResolution {
    /// Resolved catalog name
    pub catalog_name: String,
    /// Management client
    pub client: Box<dyn CatalogManagement>,
}

impl ManagementResolution {
    /// Get the catalog name
    pub fn catalog_name(&self) -> &str {
        &self.catalog_name
    }

    /// Get the management client
    pub fn client(&self) -> &dyn CatalogManagement {
        self.client.as_ref()
    }

    /// Check if management is supported
    pub fn supports_management(&self) -> bool {
        self.client.supports_management()
    }

    /// Get catalog type for error messages
    pub fn catalog_type(&self) -> &'static str {
        self.client.catalog_type()
    }
}

/// Resolve catalog name and create management client from CLI context
///
/// Uses the global -c/--catalog option or falls back to config context.
pub async fn resolve_management_from_context(ctx: &CatalogContext) -> Result<ManagementResolution> {
    let config = Config::load()?;
    let (catalog_name, catalog_config) = ctx.resolve_catalog_config(&config)?;

    // Create management client with catalog name for credentials lookup
    let client = create_management_client_with_name(&catalog_config, Some(&catalog_name)).await?;

    Ok(ManagementResolution {
        catalog_name,
        client,
    })
}

/// Get the provider for the current catalog from context
pub fn get_catalog_provider_from_context(
    ctx: &CatalogContext,
) -> Result<crate::config::CatalogProvider> {
    let config = Config::load()?;
    let (_, catalog_config) = ctx.resolve_catalog_config(&config)?;

    Ok(catalog_config.provider())
}
