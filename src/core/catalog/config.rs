//! Catalog configuration types

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use crate::utils::credentials::CredentialSource;

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
    /// Credential source (optional, format depends on catalog type)
    #[serde(default)]
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
            credential: None,
            properties: HashMap::new(),
        }
    }

    /// Set warehouse location
    pub fn with_warehouse(mut self, warehouse: impl Into<String>) -> Self {
        self.warehouse = Some(warehouse.into());
        self
    }

    /// Set credential source
    pub fn with_credential(mut self, credential: CredentialSource) -> Self {
        self.credential = Some(credential);
        self
    }

    /// Add a property
    pub fn with_property(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.properties.insert(key.into(), value.into());
        self
    }

    /// Resolve credential to a string token/secret
    pub fn resolve_credential(&self) -> crate::error::Result<Option<String>> {
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
        // Test CatalogType serializes to lowercase
        let catalog_type = CatalogType::Rest;
        let yaml = serde_yaml::to_string(&catalog_type).unwrap();
        assert_eq!(yaml.trim(), "rest");

        // Test deserialize from lowercase string
        let deserialized: CatalogType = serde_yaml::from_str("rest").unwrap();
        assert_eq!(deserialized, CatalogType::Rest);
    }

    #[test]
    fn test_catalog_config_yaml_roundtrip() {
        let config = CatalogConfig::rest("http://nessie:19120/api/v2")
            .with_warehouse("s3://lakehouse/warehouse")
            .with_property("key", "value");

        // Serialize to YAML
        let yaml = serde_yaml::to_string(&config).unwrap();

        // Verify YAML contains expected fields
        assert!(yaml.contains("type: rest"));
        assert!(yaml.contains("uri: http://nessie:19120/api/v2"));
        assert!(yaml.contains("warehouse: s3://lakehouse/warehouse"));

        // Deserialize back
        let deserialized: CatalogConfig = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(deserialized.catalog_type, CatalogType::Rest);
        assert_eq!(deserialized.uri, "http://nessie:19120/api/v2");
        assert_eq!(deserialized.warehouse, Some("s3://lakehouse/warehouse".to_string()));
        assert_eq!(deserialized.properties.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_catalog_config_yaml_with_inline_credential() {
        let config = CatalogConfig::rest("http://localhost:19120")
            .with_credential(CredentialSource::Inline("secret".to_string()));

        let yaml = serde_yaml::to_string(&config).unwrap();

        // Inline credentials serialize as plain string for backward compatibility
        assert!(yaml.contains("credential: secret"));

        let deserialized: CatalogConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(deserialized.credential, Some(CredentialSource::Inline("secret".to_string())));
    }

    #[test]
    fn test_catalog_config_yaml_with_env_credential() {
        let config = CatalogConfig::rest("http://localhost:19120")
            .with_credential(CredentialSource::EnvVar("MY_TOKEN".to_string()));

        let yaml = serde_yaml::to_string(&config).unwrap();

        // EnvVar credentials serialize as object
        assert!(yaml.contains("type: env-var"));
        assert!(yaml.contains("value: MY_TOKEN"));

        let deserialized: CatalogConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(deserialized.credential, Some(CredentialSource::EnvVar("MY_TOKEN".to_string())));
    }

    #[test]
    fn test_catalog_config_yaml_minimal() {
        // Test deserializing minimal YAML (only required fields)
        let yaml = r#"
type: rest
uri: http://localhost:19120
"#;

        let config: CatalogConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.catalog_type, CatalogType::Rest);
        assert_eq!(config.uri, "http://localhost:19120");
        assert!(config.warehouse.is_none());
        assert!(config.credential.is_none());
        assert!(config.properties.is_empty());
    }

    #[test]
    fn test_catalog_type_display() {
        assert_eq!(CatalogType::Rest.to_string(), "rest");
    }
}
