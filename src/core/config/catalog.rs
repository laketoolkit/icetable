//! Catalog configuration types
//!
//! Defines configuration for connecting to Iceberg catalogs (REST, Glue, etc.)
//! and authentication methods (Bearer, OAuth2, SigV4).

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use super::CredentialSource;
use crate::error::Result;

/// Type of catalog backend
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "lowercase")]
pub enum CatalogType {
    /// REST Catalog (Nessie, Polaris, Tabular, etc.)
    #[default]
    Rest,
    // Future: Glue, Hive, etc.
}

impl fmt::Display for CatalogType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogType::Rest => write!(f, "rest"),
        }
    }
}

/// Authentication configuration for REST catalogs
///
/// Supports multiple authentication methods:
/// - Bearer token (static)
/// - OAuth2 client credentials flow
/// - SigV4 (AWS IAM)
/// - No authentication (for local development)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CatalogAuth {
    /// No authentication (default for local catalogs)
    #[default]
    None,

    /// Bearer token authentication
    /// Uses: Authorization: Bearer <token>
    Bearer {
        /// Token source (inline, env var, or file)
        token: CredentialSource,
    },

    /// OAuth2 client credentials flow
    /// Automatically fetches and refreshes tokens
    #[serde(rename = "oauth2")]
    OAuth2 {
        /// Client ID
        client_id: String,
        /// Client secret source
        client_secret: CredentialSource,
        /// OAuth2 token endpoint (optional, defaults to catalog's /v1/oauth/tokens)
        #[serde(default)]
        token_endpoint: Option<String>,
        /// OAuth2 scope (optional)
        #[serde(default)]
        scope: Option<String>,
    },

    /// AWS SigV4 authentication (for AWS Glue, etc.)
    /// Uses IAM credentials from environment/instance metadata
    #[serde(rename = "sigv4")]
    SigV4 {
        /// AWS region
        region: String,
        /// Signing service name (default: "execute-api")
        #[serde(default = "default_signing_name")]
        signing_name: String,
    },
}

fn default_signing_name() -> String {
    "execute-api".to_string()
}

impl CatalogAuth {
    /// Create bearer token auth from a source
    pub fn bearer(token: CredentialSource) -> Self {
        Self::Bearer { token }
    }

    /// Create OAuth2 auth
    pub fn oauth2(
        client_id: impl Into<String>,
        client_secret: CredentialSource,
        token_endpoint: Option<String>,
        scope: Option<String>,
    ) -> Self {
        Self::OAuth2 {
            client_id: client_id.into(),
            client_secret,
            token_endpoint,
            scope,
        }
    }

    /// Create SigV4 auth
    pub fn sigv4(region: impl Into<String>) -> Self {
        Self::SigV4 {
            region: region.into(),
            signing_name: default_signing_name(),
        }
    }

    /// Convert to properties for iceberg-catalog-rest
    pub fn to_properties(&self) -> Result<HashMap<String, String>> {
        let mut props = HashMap::new();

        match self {
            CatalogAuth::None => {}
            CatalogAuth::Bearer { token } => {
                if let Some(t) = token.resolve()? {
                    props.insert("token".to_string(), t);
                }
            }
            CatalogAuth::OAuth2 {
                client_id,
                client_secret,
                token_endpoint,
                scope,
            } => {
                let secret = client_secret.resolve()?.unwrap_or_default();
                // Format: client_id:client_secret
                props.insert(
                    "credential".to_string(),
                    format!("{}:{}", client_id, secret),
                );
                if let Some(endpoint) = token_endpoint {
                    props.insert("oauth2-server-uri".to_string(), endpoint.clone());
                }
                if let Some(s) = scope {
                    props.insert("scope".to_string(), s.clone());
                }
            }
            CatalogAuth::SigV4 {
                region,
                signing_name,
            } => {
                props.insert("rest.sigv4-enabled".to_string(), "true".to_string());
                props.insert("rest.signing-region".to_string(), region.clone());
                props.insert("rest.signing-name".to_string(), signing_name.clone());
            }
        }

        Ok(props)
    }

    /// Get a description of the auth method
    pub fn describe(&self) -> &'static str {
        match self {
            CatalogAuth::None => "none",
            CatalogAuth::Bearer { .. } => "bearer",
            CatalogAuth::OAuth2 { .. } => "oauth2",
            CatalogAuth::SigV4 { .. } => "sigv4",
        }
    }
}

/// Configuration for a catalog connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogConfig {
    /// Type of catalog
    #[serde(rename = "type")]
    pub catalog_type: CatalogType,
    /// URI of the catalog service
    pub uri: String,
    /// Warehouse location (optional, some catalogs provide this)
    #[serde(default)]
    pub warehouse: Option<String>,
    /// Default namespace for this catalog (like kubectl namespace)
    #[serde(default)]
    pub default_namespace: Option<String>,
    /// Authentication configuration
    #[serde(default)]
    pub auth: CatalogAuth,
    /// Legacy credential field (for backwards compatibility)
    /// Prefer using `auth` instead
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<CredentialSource>,
    /// Additional properties
    #[serde(default)]
    pub properties: HashMap<String, String>,
}

impl CatalogConfig {
    /// Create a new REST catalog configuration
    pub fn rest(uri: impl Into<String>) -> Self {
        Self {
            catalog_type: CatalogType::Rest,
            uri: uri.into(),
            warehouse: None,
            default_namespace: None,
            auth: CatalogAuth::None,
            credential: None,
            properties: HashMap::new(),
        }
    }

    /// Set warehouse location
    pub fn with_warehouse(mut self, warehouse: impl Into<String>) -> Self {
        self.warehouse = Some(warehouse.into());
        self
    }

    /// Set default namespace
    pub fn with_default_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.default_namespace = Some(namespace.into());
        self
    }

    /// Set authentication configuration
    pub fn with_auth(mut self, auth: CatalogAuth) -> Self {
        self.auth = auth;
        self
    }

    /// Set credential source (legacy, prefer with_auth)
    pub fn with_credential(mut self, credential: CredentialSource) -> Self {
        self.credential = Some(credential);
        self
    }

    /// Add a property
    pub fn with_property(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.properties.insert(key.into(), value.into());
        self
    }

    /// Get all properties for the REST catalog client
    /// Combines auth properties with custom properties
    pub fn to_catalog_properties(&self) -> Result<HashMap<String, String>> {
        let mut props = HashMap::new();

        // Add auth properties
        props.extend(self.auth.to_properties()?);

        // Legacy credential support (if auth is None)
        if matches!(self.auth, CatalogAuth::None)
            && let Some(ref cred) = self.credential
            && let Some(token) = cred.resolve()?
        {
            props.insert("credential".to_string(), token);
        }

        // Add custom properties
        props.extend(self.properties.clone());

        Ok(props)
    }

    /// Resolve credential to a string token/secret (legacy)
    pub fn resolve_credential(&self) -> Result<Option<String>> {
        match &self.credential {
            Some(source) => source.resolve(),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_type_yaml_serialization() {
        let catalog_type = CatalogType::Rest;
        let yaml = serde_yaml::to_string(&catalog_type).unwrap();
        assert_eq!(yaml.trim(), "rest");

        let deserialized: CatalogType = serde_yaml::from_str("rest").unwrap();
        assert_eq!(deserialized, CatalogType::Rest);
    }

    #[test]
    fn test_catalog_config_yaml_roundtrip() {
        let config = CatalogConfig::rest("http://nessie:19120/iceberg/")
            .with_warehouse("s3://lakehouse/warehouse")
            .with_property("key", "value");

        let yaml = serde_yaml::to_string(&config).unwrap();

        assert!(yaml.contains("type: rest"));
        assert!(yaml.contains("uri: http://nessie:19120/iceberg/"));
        assert!(yaml.contains("warehouse: s3://lakehouse/warehouse"));

        let deserialized: CatalogConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(deserialized.catalog_type, CatalogType::Rest);
        assert_eq!(deserialized.uri, "http://nessie:19120/iceberg/");
    }

    #[test]
    fn test_catalog_auth_bearer() {
        unsafe {
            std::env::set_var("TEST_BEARER_TOKEN", "my-token");
        }
        let auth = CatalogAuth::bearer(CredentialSource::EnvVar("TEST_BEARER_TOKEN".to_string()));
        let props = auth.to_properties().unwrap();
        assert_eq!(props.get("token"), Some(&"my-token".to_string()));
        unsafe {
            std::env::remove_var("TEST_BEARER_TOKEN");
        }
    }

    #[test]
    fn test_catalog_auth_oauth2() {
        unsafe {
            std::env::set_var("TEST_CLIENT_SECRET", "secret123");
        }
        let auth = CatalogAuth::oauth2(
            "client-id",
            CredentialSource::EnvVar("TEST_CLIENT_SECRET".to_string()),
            Some("https://auth.example.com/token".to_string()),
            Some("catalog".to_string()),
        );
        let props = auth.to_properties().unwrap();
        assert_eq!(
            props.get("credential"),
            Some(&"client-id:secret123".to_string())
        );
        assert_eq!(
            props.get("oauth2-server-uri"),
            Some(&"https://auth.example.com/token".to_string())
        );
        assert_eq!(props.get("scope"), Some(&"catalog".to_string()));
        unsafe {
            std::env::remove_var("TEST_CLIENT_SECRET");
        }
    }

    #[test]
    fn test_catalog_auth_sigv4() {
        let auth = CatalogAuth::sigv4("us-west-2");
        let props = auth.to_properties().unwrap();
        assert_eq!(props.get("rest.sigv4-enabled"), Some(&"true".to_string()));
        assert_eq!(
            props.get("rest.signing-region"),
            Some(&"us-west-2".to_string())
        );
    }

    #[test]
    fn test_catalog_config_yaml_minimal() {
        let yaml = r#"
type: rest
uri: http://localhost:19120
"#;

        let config: CatalogConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.catalog_type, CatalogType::Rest);
        assert_eq!(config.uri, "http://localhost:19120");
        assert!(config.warehouse.is_none());
        assert!(matches!(config.auth, CatalogAuth::None));
    }
}
