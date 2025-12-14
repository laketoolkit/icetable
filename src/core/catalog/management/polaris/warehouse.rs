//! Polaris warehouse (catalog) operations
//!
//! Implements CRUD operations for Polaris warehouses using the Management API.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::PolarisManagement;
use crate::core::catalog::management::{
    CreateWarehouseRequest, StorageType, Warehouse, WarehouseType,
};
use crate::error::{Error, Result};

// =============================================================================
// Polaris API Types
// =============================================================================

/// Polaris catalog (warehouse) response
#[derive(Debug, Deserialize)]
struct PolarisCatalog {
    name: String,
    #[serde(rename = "type")]
    catalog_type: String,
    properties: HashMap<String, String>,
    #[serde(rename = "storageConfigInfo")]
    storage_config_info: StorageConfigInfo,
}

/// Storage configuration in Polaris response
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StorageConfigInfo {
    storage_type: String,
    #[serde(default)]
    allowed_locations: Vec<String>,
}

/// List catalogs response
#[derive(Debug, Deserialize)]
struct ListCatalogsResponse {
    catalogs: Vec<PolarisCatalog>,
}

/// Create catalog request body
#[derive(Debug, Serialize)]
struct CreateCatalogRequest {
    catalog: CatalogBody,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogBody {
    name: String,
    #[serde(rename = "type")]
    catalog_type: String,
    properties: HashMap<String, String>,
    storage_config_info: StorageConfigInfoRequest,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageConfigInfoRequest {
    storage_type: String,
    allowed_locations: Vec<String>,
    /// Additional storage config (flattened into the JSON)
    #[serde(flatten)]
    config: HashMap<String, String>,
}

// =============================================================================
// Conversion Functions
// =============================================================================

impl From<PolarisCatalog> for Warehouse {
    fn from(catalog: PolarisCatalog) -> Self {
        let warehouse_type = match catalog.catalog_type.as_str() {
            "INTERNAL" => WarehouseType::Internal,
            "EXTERNAL" => WarehouseType::External,
            _ => WarehouseType::Internal,
        };

        let storage_type = match catalog.storage_config_info.storage_type.as_str() {
            "S3" => StorageType::S3,
            "GCS" => StorageType::Gcs,
            "AZURE" => StorageType::Azure,
            "FILE" => StorageType::File,
            _ => StorageType::S3,
        };

        let default_base_location = catalog
            .properties
            .get("default-base-location")
            .cloned()
            .unwrap_or_default();

        Warehouse {
            name: catalog.name,
            warehouse_type,
            storage_type,
            default_base_location,
            allowed_locations: catalog.storage_config_info.allowed_locations,
            properties: catalog.properties,
        }
    }
}

// =============================================================================
// API Operations
// =============================================================================

/// List all warehouses (catalogs) in Polaris
pub async fn list_warehouses(client: &PolarisManagement) -> Result<Vec<Warehouse>> {
    let url = format!("{}/catalogs", client.base_url);

    let response = client
        .authenticated_request(reqwest::Method::GET, &url)
        .await?
        .send()
        .await
        .map_err(|e| Error::Network {
            message: format!("Failed to list warehouses: {}", e),
            source: Some(Box::new(e)),
        })?;

    if !response.status().is_success() {
        return Err(parse_error_response(response, "list warehouses").await);
    }

    let list_response: ListCatalogsResponse = response.json().await.map_err(|e| Error::Parse {
        message: format!("Failed to parse warehouses response: {}", e),
        source: Some(Box::new(e)),
    })?;

    Ok(list_response.catalogs.into_iter().map(Into::into).collect())
}

/// Get a specific warehouse by name
pub async fn get_warehouse(client: &PolarisManagement, name: &str) -> Result<Warehouse> {
    let url = format!("{}/catalogs/{}", client.base_url, name);

    let response = client
        .authenticated_request(reqwest::Method::GET, &url)
        .await?
        .send()
        .await
        .map_err(|e| Error::Network {
            message: format!("Failed to get warehouse '{}': {}", name, e),
            source: Some(Box::new(e)),
        })?;

    if !response.status().is_success() {
        return Err(parse_error_response(response, &format!("get warehouse '{}'", name)).await);
    }

    let catalog: PolarisCatalog = response.json().await.map_err(|e| Error::Parse {
        message: format!("Failed to parse warehouse response: {}", e),
        source: Some(Box::new(e)),
    })?;

    Ok(catalog.into())
}

/// Create a new warehouse
pub async fn create_warehouse(
    client: &PolarisManagement,
    request: CreateWarehouseRequest,
) -> Result<Warehouse> {
    let url = format!("{}/catalogs", client.base_url);

    // Use inferred storage type if not explicitly set (before moving fields)
    let storage_type = request.inferred_storage_type();

    // Build the Polaris-specific request body
    let mut properties = request.properties;
    properties.insert(
        "default-base-location".to_string(),
        request.default_base_location.clone(),
    );

    let create_request = CreateCatalogRequest {
        catalog: CatalogBody {
            name: request.name.clone(),
            catalog_type: request.warehouse_type.to_string(),
            properties,
            storage_config_info: StorageConfigInfoRequest {
                storage_type: storage_type.to_string(),
                allowed_locations: if request.allowed_locations.is_empty() {
                    vec![request.default_base_location]
                } else {
                    request.allowed_locations
                },
                config: request.storage_config,
            },
        },
    };

    let response = client
        .authenticated_request(reqwest::Method::POST, &url)
        .await?
        .json(&create_request)
        .send()
        .await
        .map_err(|e| Error::Network {
            message: format!("Failed to create warehouse '{}': {}", request.name, e),
            source: Some(Box::new(e)),
        })?;

    if !response.status().is_success() {
        return Err(
            parse_error_response(response, &format!("create warehouse '{}'", request.name)).await,
        );
    }

    // Polaris returns the created catalog in the response
    let catalog: PolarisCatalog = response.json().await.map_err(|e| Error::Parse {
        message: format!("Failed to parse create warehouse response: {}", e),
        source: Some(Box::new(e)),
    })?;

    Ok(catalog.into())
}

/// Delete a warehouse by name
pub async fn delete_warehouse(client: &PolarisManagement, name: &str) -> Result<()> {
    let url = format!("{}/catalogs/{}", client.base_url, name);

    let response = client
        .authenticated_request(reqwest::Method::DELETE, &url)
        .await?
        .send()
        .await
        .map_err(|e| Error::Network {
            message: format!("Failed to delete warehouse '{}': {}", name, e),
            source: Some(Box::new(e)),
        })?;

    if !response.status().is_success() {
        return Err(
            parse_error_response(response, &format!("delete warehouse '{}'", name)).await,
        );
    }

    Ok(())
}

// =============================================================================
// Error Handling
// =============================================================================

/// Parse error response from Polaris API
async fn parse_error_response(response: reqwest::Response, operation: &str) -> Error {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    // Try to extract message from JSON error response
    let message = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
        json.get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .map(|s| s.to_string())
            .unwrap_or(body)
    } else {
        body
    };

    match status.as_u16() {
        401 => Error::AuthenticationFailed {
            provider: "Polaris".to_string(),
            message: format!("Authentication required to {}", operation),
        },
        403 => Error::AccessDenied {
            path: operation.to_string(),
            message: format!("Permission denied: {}", message),
        },
        404 => Error::CatalogOperation {
            message: format!("Not found: {}", message),
        },
        409 => Error::Conflict(format!("Conflict while trying to {}: {}", operation, message)),
        _ => Error::CatalogOperation {
            message: format!("Failed to {} ({}): {}", operation, status, message),
        },
    }
}
