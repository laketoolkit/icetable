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
    /// Uses Value to preserve boolean/number types from JSON input
    #[serde(flatten)]
    config: HashMap<String, serde_json::Value>,
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

    // Check if user specified storageType in config
    // If so, use that; otherwise infer from location
    let mut storage_config = request.storage_config.clone();
    let mut storage_type = storage_config
        .remove("storageType")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| request.inferred_storage_type().to_string());

    // Normalize storage type: S3_COMPATIBLE -> S3 (Polaris only supports S3, GCS, AZURE, FILE)
    if storage_type == "S3_COMPATIBLE" {
        storage_type = "S3".to_string();
    }

    // Normalize field names: s3.endpoint -> endpoint, s3.pathStyleAccess -> pathStyleAccess
    // Polaris expects direct field names in storageConfigInfo, not s3. prefixed
    let field_mappings = [
        ("s3.endpoint", "endpoint"),
        ("s3.pathStyleAccess", "pathStyleAccess"),
        ("s3.region", "region"),
        ("s3.roleArn", "roleArn"),
    ];
    for (old_key, new_key) in field_mappings {
        if let Some(value) = storage_config.remove(old_key) {
            storage_config.insert(new_key.to_string(), value);
        }
    }

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
                storage_type,
                allowed_locations: if request.allowed_locations.is_empty() {
                    vec![request.default_base_location]
                } else {
                    request.allowed_locations
                },
                config: storage_config,
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
        return Err(parse_error_response(
            response,
            &format!("create warehouse '{}'", request.name),
        )
        .await);
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
        return Err(parse_error_response(response, &format!("delete warehouse '{}'", name)).await);
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
    // Polaris uses different formats: {"error": {"message": "..."}} or {"message": "..."}
    let message = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
        // Try nested error.message first
        json.get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .map(|s| s.to_string())
            // Then try direct message field
            .or_else(|| {
                json.get("message")
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
            })
            // Fall back to raw body if we can't parse
            .unwrap_or_else(|| {
                if body.is_empty() {
                    "unknown error".to_string()
                } else {
                    body
                }
            })
    } else if body.is_empty() {
        "unknown error".to_string()
    } else {
        body
    };

    // Clean up common verbose patterns from Polaris
    let clean_message = clean_polaris_message(&message);

    match status.as_u16() {
        401 => Error::AuthenticationFailed {
            provider: "Polaris".to_string(),
            message: format!("run 'icetable auth login' to {}", operation),
        },
        403 => Error::AccessDenied {
            path: operation.to_string(),
            message: clean_message,
        },
        404 => {
            // If clean_message already contains "not found", don't double prefix
            if clean_message.contains("not found") {
                Error::CatalogOperation {
                    message: clean_message,
                }
            } else {
                Error::CatalogOperation {
                    message: format!("not found: {}", clean_message),
                }
            }
        }
        409 => Error::Conflict(clean_message),
        _ => Error::CatalogOperation {
            message: clean_message,
        },
    }
}

/// Clean up verbose Polaris error messages
fn clean_polaris_message(msg: &str) -> String {
    let msg = msg.trim();
    let msg_lower = msg.to_lowercase();

    // Pattern: "Catalog 'X' cannot be dropped, it is not empty"
    if msg_lower.contains("cannot be dropped")
        && msg_lower.contains("not empty")
        && let Some(name) = extract_quoted_name(msg)
    {
        return format!("warehouse '{}' is not empty", name);
    }

    // Pattern: "Cannot create Catalog X. Catalog already exists"
    if msg_lower.contains("already exists")
        && let Some(name) = extract_catalog_name(msg)
    {
        return format!("warehouse '{}' already exists", name);
    }

    // Pattern: "TopLevelEntity of type CATALOG does not exist: X"
    // Must come BEFORE generic "does not exist" check
    if msg_lower.contains("toplevelentity")
        && msg_lower.contains("does not exist")
        && let Some(pos) = msg.rfind(": ")
    {
        let name = msg[pos + 2..].trim();
        if !name.is_empty() {
            return format!("warehouse '{}' not found", name);
        }
    }

    // Pattern: "Unable to find warehouse 'X'" or "does not exist"
    if (msg_lower.contains("unable to find") || msg_lower.contains("does not exist"))
        && let Some(name) = extract_catalog_name(msg)
    {
        return format!("warehouse '{}' not found", name);
    }

    // Default: just lowercase first letter for consistency
    let mut chars = msg.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_lowercase().chain(chars).collect(),
    }
}

/// Extract a quoted name like 'foo' from a string
fn extract_quoted_name(msg: &str) -> Option<&str> {
    let start = msg.find('\'')?;
    let rest = &msg[start + 1..];
    let end = rest.find('\'')?;
    Some(&rest[..end])
}

/// Extract catalog name from messages like "Cannot create Catalog foo. Catalog already exists"
fn extract_catalog_name(msg: &str) -> Option<&str> {
    // Try quoted first
    if let Some(name) = extract_quoted_name(msg) {
        return Some(name);
    }
    // Try "Catalog X." pattern
    if let Some(start) = msg.find("Catalog ") {
        let rest = &msg[start + 8..];
        let end = rest.find(['.', ' '])?;
        return Some(&rest[..end]);
    }
    None
}
