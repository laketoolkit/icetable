//! Catalog credentials management
//!
//! Handles loading/saving catalog credentials from ~/.config/icetable/credentials.yaml
//! Credentials are stored separately from config.yaml for security (can have different permissions).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::{CatalogAuth, CredentialSource};
use crate::error::{Error, Result};

/// Credentials file name (in the same directory as config.yaml)
const CREDENTIALS_FILE: &str = "credentials.yaml";

/// Catalog credentials storage
///
/// Stored in ~/.config/icetable/credentials.yaml
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CatalogCredentials {
    /// Authentication configuration per catalog name
    #[serde(default)]
    pub catalogs: HashMap<String, CatalogAuth>,
}

impl CatalogCredentials {
    /// Get the credentials file path
    pub fn credentials_path() -> Result<PathBuf> {
        let config_dir = super::Config::config_dir()?;
        Ok(config_dir.join(CREDENTIALS_FILE))
    }

    /// Load credentials from file
    pub fn load() -> Result<Self> {
        let path = Self::credentials_path()?;

        if path.exists() {
            let content = std::fs::read_to_string(&path).map_err(|e| Error::Configuration {
                message: format!("Failed to read credentials file: {}", e),
            })?;

            return serde_yaml_ng::from_str(&content).map_err(|e| Error::Parse {
                message: format!("Failed to parse credentials file: {}", e),
                source: Some(Box::new(e)),
            });
        }

        Ok(Self::default())
    }

    /// Save credentials to file
    pub fn save(&self) -> Result<()> {
        let config_dir = super::Config::config_dir()?;
        let path = Self::credentials_path()?;

        // Create config directory if it doesn't exist
        std::fs::create_dir_all(&config_dir).map_err(|e| Error::Configuration {
            message: format!("Failed to create config directory: {}", e),
        })?;

        let content = serde_yaml_ng::to_string(self).map_err(|e| Error::Serialization {
            message: format!("Failed to serialize credentials: {}", e),
        })?;

        std::fs::write(&path, content).map_err(|e| Error::Configuration {
            message: format!("Failed to write credentials file: {}", e),
        })?;

        // Set restrictive permissions (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&path, perms).ok(); // Best effort
        }

        Ok(())
    }

    /// Get auth for a specific catalog
    pub fn get(&self, catalog_name: &str) -> Option<&CatalogAuth> {
        self.catalogs.get(catalog_name)
    }

    /// Set auth for a catalog
    pub fn set(&mut self, catalog_name: impl Into<String>, auth: CatalogAuth) {
        self.catalogs.insert(catalog_name.into(), auth);
    }

    /// Remove auth for a catalog
    pub fn remove(&mut self, catalog_name: &str) -> bool {
        self.catalogs.remove(catalog_name).is_some()
    }

    /// Check if credentials exist for a catalog
    pub fn has(&self, catalog_name: &str) -> bool {
        self.catalogs.contains_key(catalog_name)
    }

    /// List all catalog names with stored credentials
    pub fn catalog_names(&self) -> Vec<&String> {
        self.catalogs.keys().collect()
    }
}

/// Login request for OAuth2 authentication
#[derive(Debug, Clone)]
pub struct OAuth2LoginRequest {
    /// OAuth2 client ID
    pub client_id: String,
    /// OAuth2 client secret source
    pub client_secret: CredentialSource,
    /// Token endpoint URL (optional, defaults to catalog's /v1/oauth/tokens)
    pub token_endpoint: Option<String>,
    /// OAuth2 scope
    pub scope: Option<String>,
}

impl OAuth2LoginRequest {
    /// Create a new OAuth2 login request
    pub fn new(client_id: impl Into<String>, client_secret: CredentialSource) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret,
            token_endpoint: None,
            scope: None,
        }
    }

    /// Set the token endpoint URL
    pub fn with_token_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.token_endpoint = Some(endpoint.into());
        self
    }

    /// Set the OAuth2 scope
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = Some(scope.into());
        self
    }

    /// Convert to CatalogAuth
    pub fn into_auth(self) -> CatalogAuth {
        CatalogAuth::OAuth2 {
            client_id: self.client_id,
            client_secret: self.client_secret,
            token_endpoint: self.token_endpoint,
            scope: self.scope,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_credentials_default() {
        let creds = CatalogCredentials::default();
        assert!(creds.catalogs.is_empty());
    }

    #[test]
    fn test_catalog_credentials_set_get() {
        let mut creds = CatalogCredentials::default();

        let auth = CatalogAuth::OAuth2 {
            client_id: "admin".to_string(),
            client_secret: CredentialSource::Inline("secret".to_string()),
            token_endpoint: None,
            scope: Some("PRINCIPAL_ROLE:ALL".to_string()),
        };

        creds.set("polaris", auth.clone());
        assert!(creds.has("polaris"));

        let retrieved = creds.get("polaris").unwrap();
        assert!(matches!(retrieved, CatalogAuth::OAuth2 { client_id, .. } if client_id == "admin"));
    }

    #[test]
    fn test_catalog_credentials_remove() {
        let mut creds = CatalogCredentials::default();
        creds.set("polaris", CatalogAuth::None);

        assert!(creds.remove("polaris"));
        assert!(!creds.has("polaris"));
        assert!(!creds.remove("polaris")); // Already removed
    }

    #[test]
    fn test_yaml_roundtrip() {
        let mut creds = CatalogCredentials::default();

        creds.set(
            "polaris",
            CatalogAuth::OAuth2 {
                client_id: "admin".to_string(),
                client_secret: CredentialSource::EnvVar("POLARIS_SECRET".to_string()),
                token_endpoint: Some(
                    "http://localhost:8181/api/catalog/v1/oauth/tokens".to_string(),
                ),
                scope: Some("PRINCIPAL_ROLE:ALL".to_string()),
            },
        );

        creds.set(
            "nessie",
            CatalogAuth::Bearer {
                token: CredentialSource::Inline("my-token".to_string()),
            },
        );

        let yaml = serde_yaml_ng::to_string(&creds).unwrap();
        let parsed: CatalogCredentials = serde_yaml_ng::from_str(&yaml).unwrap();

        assert!(parsed.has("polaris"));
        assert!(parsed.has("nessie"));
    }
}
