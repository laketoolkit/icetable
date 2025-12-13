//! Configuration manager
//!
//! Handles loading/saving configuration from ~/.config/icetable/config.yaml

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::{CatalogConfig, ResolvedTable, is_direct_path};
use crate::error::{Error, Result};

/// Configuration file name
const CONFIG_DIR: &str = "icetable";
const CONFIG_FILE: &str = "config.yaml";

/// icetable configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Current context (like kubectl current-context)
    /// Can be: table alias, path, or catalog.table
    #[serde(default)]
    pub current_context: Option<String>,

    /// Current catalog (like kubectl current-context for cluster)
    #[serde(default)]
    pub current_catalog: Option<String>,

    /// Named table aliases for quick access (standalone tables)
    #[serde(default)]
    pub tables: HashMap<String, String>,

    /// Catalog configurations
    #[serde(default)]
    pub catalogs: HashMap<String, CatalogConfig>,
}

impl Config {
    /// Get the config directory path
    pub fn config_dir() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| Error::Configuration {
                message: "Could not determine config directory".to_string(),
            })?
            .join(CONFIG_DIR);

        Ok(config_dir)
    }

    /// Get the config file path
    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join(CONFIG_FILE))
    }

    /// Load configuration from file
    pub fn load() -> Result<Self> {
        let yaml_path = Self::config_path()?;

        if yaml_path.exists() {
            let content =
                std::fs::read_to_string(&yaml_path).map_err(|e| Error::Configuration {
                    message: format!("Failed to read config file: {}", e),
                })?;

            return serde_yaml::from_str(&content).map_err(|e| Error::Parse {
                message: format!("Failed to parse config file: {}", e),
                source: Some(Box::new(e)),
            });
        }

        Ok(Self::default())
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let config_dir = Self::config_dir()?;
        let config_path = Self::config_path()?;

        // Create config directory if it doesn't exist
        std::fs::create_dir_all(&config_dir).map_err(|e| Error::Configuration {
            message: format!("Failed to create config directory: {}", e),
        })?;

        let content = serde_yaml::to_string(self).map_err(|e| Error::Serialization {
            message: format!("Failed to serialize config: {}", e),
        })?;

        std::fs::write(&config_path, content).map_err(|e| Error::Configuration {
            message: format!("Failed to write config file: {}", e),
        })?;

        Ok(())
    }

    // --- Context management ---

    /// Set the current context
    pub fn set_current_context(&mut self, context: String) {
        self.current_context = Some(context);
    }

    /// Unset the current context
    pub fn unset_current_context(&mut self) {
        self.current_context = None;
    }

    /// Get the current context
    pub fn get_current_context(&self) -> Option<&str> {
        self.current_context.as_deref()
    }

    // --- Catalog management ---

    /// Set the current catalog
    pub fn set_current_catalog(&mut self, catalog: String) {
        self.current_catalog = Some(catalog);
    }

    /// Unset the current catalog
    pub fn unset_current_catalog(&mut self) {
        self.current_catalog = None;
    }

    /// Get the current catalog
    pub fn get_current_catalog(&self) -> Option<&str> {
        self.current_catalog.as_deref()
    }

    /// Get the current catalog config
    pub fn get_current_catalog_config(&self) -> Option<&CatalogConfig> {
        self.current_catalog
            .as_ref()
            .and_then(|name| self.catalogs.get(name))
    }

    /// Parse the current context into (catalog, namespace, table) parts
    /// Context format: "catalog", "catalog.namespace", or "catalog.namespace.table"
    pub fn parse_current_context(&self) -> Option<(String, Option<String>, Option<String>)> {
        let context = self.current_context.as_ref()?;
        let parts: Vec<&str> = context.splitn(3, '.').collect();

        match parts.len() {
            1 => Some((parts[0].to_string(), None, None)),
            2 => Some((parts[0].to_string(), Some(parts[1].to_string()), None)),
            3 => Some((
                parts[0].to_string(),
                Some(parts[1].to_string()),
                Some(parts[2].to_string()),
            )),
            _ => None,
        }
    }

    /// Get the current table name from context (if any)
    pub fn get_current_table(&self) -> Option<&str> {
        let context = self.current_context.as_ref()?;
        let parts: Vec<&str> = context.splitn(3, '.').collect();
        if parts.len() == 3 {
            Some(parts[2])
        } else {
            None
        }
    }

    /// Get the current namespace from context or catalog config
    pub fn get_current_namespace(&self) -> Option<String> {
        // First try from context
        if let Some(context) = &self.current_context {
            let parts: Vec<&str> = context.splitn(3, '.').collect();
            if parts.len() >= 2 {
                return Some(parts[1].to_string());
            }
        }
        // Fallback to catalog's default namespace
        self.get_current_catalog_config()
            .and_then(|c| c.default_namespace.clone())
    }

    /// Set the default namespace for a catalog
    pub fn set_catalog_namespace(&mut self, catalog_name: &str, namespace: String) -> bool {
        if let Some(catalog) = self.catalogs.get_mut(catalog_name) {
            catalog.default_namespace = Some(namespace);
            true
        } else {
            false
        }
    }

    /// Unset the default namespace for a catalog
    pub fn unset_catalog_namespace(&mut self, catalog_name: &str) -> bool {
        if let Some(catalog) = self.catalogs.get_mut(catalog_name) {
            catalog.default_namespace = None;
            true
        } else {
            false
        }
    }

    /// Add a catalog configuration
    pub fn add_catalog(&mut self, name: String, config: CatalogConfig) {
        self.catalogs.insert(name, config);
    }

    /// Delete a catalog
    pub fn delete_catalog(&mut self, name: &str) -> bool {
        self.catalogs.remove(name).is_some()
    }

    // --- Table alias management ---

    /// Add a named table alias
    pub fn add_table(&mut self, name: String, path: String) {
        self.tables.insert(name, path);
    }

    /// Delete a named table alias
    pub fn delete_table(&mut self, name: &str) -> bool {
        self.tables.remove(name).is_some()
    }

    // --- Table resolution ---

    /// Resolve a table reference to a path or catalog info
    /// Returns: ResolvedTable with either a direct path or catalog + table name
    pub fn resolve_table(&self, name_or_path: &str) -> Result<ResolvedTable> {
        // 1. Direct path (starts with s3://, gs://, file://, /, etc.)
        if is_direct_path(name_or_path) {
            return Ok(ResolvedTable::Path(name_or_path.to_string()));
        }

        // 2. Check if it's a catalog.table reference (catalog.namespace.table or catalog.table)
        if let Some((first, rest)) = name_or_path.split_once('.')
            && let Some(catalog) = self.catalogs.get(first)
        {
            return Ok(ResolvedTable::Catalog {
                catalog_name: first.to_string(),
                catalog_config: Box::new(catalog.clone()),
                table_name: rest.to_string(),
            });
        }

        // 3. Check if it's a table alias
        if let Some(path) = self.tables.get(name_or_path) {
            return Ok(ResolvedTable::Path(path.clone()));
        }

        // 4. If we have a current catalog context, try to use it
        //    e.g., context=polaris.demo + input=events → polaris catalog with demo.events
        if let Some(catalog_name) = self.get_current_catalog()
            && let Some(catalog) = self.catalogs.get(catalog_name)
        {
            // Get namespace from context or catalog default
            let namespace = self
                .get_current_namespace()
                .or_else(|| catalog.default_namespace.clone());

            if let Some(ns) = namespace {
                // Combine namespace.table
                let table_name = format!("{}.{}", ns, name_or_path);
                return Ok(ResolvedTable::Catalog {
                    catalog_name: catalog_name.to_string(),
                    catalog_config: Box::new(catalog.clone()),
                    table_name,
                });
            }
        }

        // 5. Not found
        Err(Error::TableNotFound {
            path: format!(
                "{}. Use a direct path (s3://...), a configured table alias, or catalog.table format",
                name_or_path
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.current_context.is_none());
        assert!(config.current_catalog.is_none());
        assert!(config.tables.is_empty());
        assert!(config.catalogs.is_empty());
    }

    #[test]
    fn test_config_context_management() {
        let mut config = Config::default();

        config.set_current_context("my-table".to_string());
        assert_eq!(config.get_current_context(), Some("my-table"));

        config.unset_current_context();
        assert_eq!(config.get_current_context(), None);
    }

    #[test]
    fn test_config_catalog_management() {
        let mut config = Config::default();

        config.set_current_catalog("nessie".to_string());
        assert_eq!(config.get_current_catalog(), Some("nessie"));

        config.unset_current_catalog();
        assert_eq!(config.get_current_catalog(), None);
    }

    #[test]
    fn test_config_table_alias() {
        let mut config = Config::default();

        config.add_table("events".to_string(), "s3://bucket/events".to_string());
        assert!(config.tables.contains_key("events"));

        assert!(config.delete_table("events"));
        assert!(!config.tables.contains_key("events"));
    }

    #[test]
    fn test_resolve_direct_path() {
        let config = Config::default();

        let resolved = config.resolve_table("s3://bucket/table").unwrap();
        assert!(matches!(resolved, ResolvedTable::Path(p) if p == "s3://bucket/table"));

        let resolved = config.resolve_table("/local/path/table").unwrap();
        assert!(matches!(resolved, ResolvedTable::Path(p) if p == "/local/path/table"));
    }

    #[test]
    fn test_resolve_table_alias() {
        let mut config = Config::default();
        config.add_table("events".to_string(), "s3://bucket/events".to_string());

        let resolved = config.resolve_table("events").unwrap();
        assert!(matches!(resolved, ResolvedTable::Path(p) if p == "s3://bucket/events"));
    }

    #[test]
    fn test_resolve_catalog_table() {
        let mut config = Config::default();
        config.add_catalog(
            "nessie".to_string(),
            CatalogConfig::rest("http://localhost:19120"),
        );

        let resolved = config.resolve_table("nessie.analytics.events").unwrap();
        match resolved {
            ResolvedTable::Catalog {
                catalog_name,
                table_name,
                ..
            } => {
                assert_eq!(catalog_name, "nessie");
                assert_eq!(table_name, "analytics.events");
            }
            _ => panic!("Expected catalog resolution"),
        }
    }
}
