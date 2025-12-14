//! Polaris catalog management implementation
//!
//! Implements the CatalogManagement trait for Apache Polaris catalogs.
//! Uses the Polaris Management API (separate from the Iceberg REST Catalog API).

mod warehouse;

use async_trait::async_trait;
use reqwest::Client;

use super::traits::CatalogManagement;
use super::{CreateWarehouseRequest, Warehouse};
use crate::core::config::{CatalogAuth, CatalogConfig};
use crate::error::{Error, Result};

/// OAuth token response structure
#[derive(Debug, serde::Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
}

/// Polaris management client
///
/// Provides access to Polaris-specific management operations like
/// creating/deleting warehouses (called "catalogs" in Polaris).
pub struct PolarisManagement {
    /// HTTP client
    client: Client,
    /// Base URL for management API (derived from catalog URI)
    base_url: String,
    /// Authentication configuration
    auth: CatalogAuth,
}

impl PolarisManagement {
    /// Create a new Polaris management client from catalog configuration
    ///
    /// If catalog_name is provided, credentials will be loaded from credentials.yaml first.
    pub async fn new(config: &CatalogConfig, catalog_name: Option<&str>) -> Result<Self> {
        // Derive management API base URL from catalog URI
        // e.g., http://localhost:8181/api/catalog -> http://localhost:8181/api/management/v1
        let base_url = Self::derive_management_url(&config.uri);

        // Get effective auth: credentials.yaml > config.yaml
        let auth = if let Some(name) = catalog_name {
            config.effective_auth(name)?
        } else {
            config.auth.clone()
        };

        Ok(Self {
            client: Client::new(),
            base_url,
            auth,
        })
    }

    /// Derive the management API URL from the catalog API URL
    fn derive_management_url(catalog_uri: &str) -> String {
        // Polaris catalog URI is typically: http://host:port/api/catalog
        // Management API is at: http://host:port/api/management/v1
        let base = catalog_uri
            .trim_end_matches('/')
            .trim_end_matches("/api/catalog");
        format!("{}/api/management/v1", base)
    }

    /// Get an access token for the management API
    async fn get_access_token(&self) -> Result<Option<String>> {
        match &self.auth {
            CatalogAuth::None => Ok(None),
            CatalogAuth::Bearer { token } => Ok(token.resolve()?),
            CatalogAuth::OAuth2 {
                client_id,
                client_secret,
                token_endpoint,
                scope,
            } => {
                // Build token endpoint URL
                // Default: derive from base URL
                let endpoint = token_endpoint.clone().unwrap_or_else(|| {
                    // Management base is /api/management/v1, oauth is at /api/catalog/v1/oauth/tokens
                    let base = self
                        .base_url
                        .trim_end_matches("/management/v1")
                        .trim_end_matches('/');
                    format!("{}/catalog/v1/oauth/tokens", base)
                });

                let secret = client_secret.resolve()?.unwrap_or_default();

                // Build form data
                let mut form: Vec<(&str, String)> = vec![
                    ("grant_type", "client_credentials".to_string()),
                    ("client_id", client_id.clone()),
                    ("client_secret", secret),
                ];
                if let Some(s) = scope {
                    form.push(("scope", s.clone()));
                }

                let response = self
                    .client
                    .post(&endpoint)
                    .form(&form)
                    .send()
                    .await
                    .map_err(|e| Error::Network {
                        message: format!("Failed to get OAuth token: {}", e),
                        source: Some(Box::new(e)),
                    })?;

                if !response.status().is_success() {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    return Err(Error::AuthenticationFailed {
                        provider: "Polaris".to_string(),
                        message: format!("OAuth token request failed ({}): {}", status, body),
                    });
                }

                let token_response: OAuthTokenResponse =
                    response.json().await.map_err(|e| Error::Parse {
                        message: format!("Failed to parse OAuth response: {}", e),
                        source: Some(Box::new(e)),
                    })?;

                Ok(Some(token_response.access_token))
            }
            CatalogAuth::SigV4 { .. } => Err(Error::UnsupportedFeature {
                feature: "SigV4 authentication for Polaris management API".to_string(),
            }),
        }
    }

    /// Build a request with authentication
    async fn authenticated_request(
        &self,
        method: reqwest::Method,
        url: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let mut request = self.client.request(method, url);

        if let Some(token) = self.get_access_token().await? {
            request = request.bearer_auth(token);
        }

        Ok(request)
    }
}

#[async_trait]
impl CatalogManagement for PolarisManagement {
    fn catalog_type(&self) -> &'static str {
        "polaris"
    }

    fn supports_management(&self) -> bool {
        true
    }

    async fn list_warehouses(&self) -> Result<Vec<Warehouse>> {
        warehouse::list_warehouses(self).await
    }

    async fn get_warehouse(&self, name: &str) -> Result<Warehouse> {
        warehouse::get_warehouse(self, name).await
    }

    async fn create_warehouse(&self, request: CreateWarehouseRequest) -> Result<Warehouse> {
        warehouse::create_warehouse(self, request).await
    }

    async fn delete_warehouse(&self, name: &str) -> Result<()> {
        warehouse::delete_warehouse(self, name).await
    }
}
