//! Configuration manager
//!
//! Handles loading/saving configuration from ~/.config/icetable/config.yaml

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::{CatalogConfig, ResolvedTable, is_direct_path};
use crate::error::{Error, Result};

/// Parsed context components from the current context string
///
/// Contains the catalog name and optional warehouse, namespace, and table.
/// See [`Config::parse_current_context`] for the parsing logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedContext {
    /// Catalog name (always present)
    pub catalog: String,
    /// Warehouse name (if specified with `@` syntax)
    pub warehouse: Option<String>,
    /// Namespace name
    pub namespace: Option<String>,
    /// Table name
    pub table: Option<String>,
}

impl ParsedContext {
    /// Create a new ParsedContext with just a catalog
    pub fn catalog_only(catalog: String) -> Self {
        Self {
            catalog,
            warehouse: None,
            namespace: None,
            table: None,
        }
    }

    /// Create a new ParsedContext with all components
    pub fn new(
        catalog: String,
        warehouse: Option<String>,
        namespace: Option<String>,
        table: Option<String>,
    ) -> Self {
        Self {
            catalog,
            warehouse,
            namespace,
            table,
        }
    }
}

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
    ///
    /// Automatically validates the configuration after loading.
    /// Returns an error if the configuration contains invalid references.
    pub fn load() -> Result<Self> {
        let yaml_path = Self::config_path()?;

        if yaml_path.exists() {
            let content =
                std::fs::read_to_string(&yaml_path).map_err(|e| Error::Configuration {
                    message: format!("Failed to read config file: {}", e),
                })?;

            let config: Self = serde_yaml_ng::from_str(&content).map_err(|e| Error::Parse {
                message: format!("Failed to parse config file: {}", e),
                source: Some(Box::new(e)),
            })?;

            // Validate the loaded config
            config.validate()?;

            return Ok(config);
        }

        Ok(Self::default())
    }

    /// Validate configuration consistency
    ///
    /// Checks:
    /// - `current_catalog` references an existing catalog
    /// - Catalog URIs are not empty
    /// - Table alias paths are not empty
    pub fn validate(&self) -> Result<()> {
        // Check current_catalog references an existing catalog
        if let Some(ref catalog_name) = self.current_catalog
            && !self.catalogs.contains_key(catalog_name)
        {
            return Err(Error::Configuration {
                message: format!(
                    "current_catalog '{}' does not exist in catalogs. Available: {}",
                    catalog_name,
                    self.catalogs.keys().cloned().collect::<Vec<_>>().join(", ")
                ),
            });
        }

        // Validate catalog configurations
        for (name, catalog) in &self.catalogs {
            if catalog.uri.is_empty() {
                return Err(Error::Configuration {
                    message: format!("Catalog '{}' has empty URI", name),
                });
            }
        }

        // Validate table aliases
        for (name, path) in &self.tables {
            if path.is_empty() {
                return Err(Error::Configuration {
                    message: format!("Table alias '{}' has empty path", name),
                });
            }
        }

        Ok(())
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let config_dir = Self::config_dir()?;
        let config_path = Self::config_path()?;

        // Create config directory if it doesn't exist
        std::fs::create_dir_all(&config_dir).map_err(|e| Error::Configuration {
            message: format!("Failed to create config directory: {}", e),
        })?;

        let content = serde_yaml_ng::to_string(self).map_err(|e| Error::Serialization {
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

    /// Parse the current context into structured components
    ///
    /// Context format: `catalog[@warehouse][.namespace][.table]`
    ///
    /// # Examples
    ///
    /// - `polaris` → `ParsedContext { catalog: "polaris", .. }`
    /// - `polaris@iceberg` → `ParsedContext { catalog: "polaris", warehouse: Some("iceberg"), .. }`
    /// - `polaris@iceberg.demo` → includes namespace "demo"
    /// - `polaris@iceberg.demo.events` → includes table "events"
    /// - `polaris.demo.events` → legacy format without warehouse
    pub fn parse_current_context(&self) -> Option<ParsedContext> {
        let context = self.current_context.as_ref()?;

        // Split on '@' first to extract warehouse
        if let Some(at_pos) = context.find('@') {
            let catalog = context[..at_pos].to_string();
            let after_at = &context[at_pos + 1..];

            // Split the rest on '.' to get warehouse and namespace.table
            let parts: Vec<&str> = after_at.splitn(3, '.').collect();
            match parts.len() {
                1 => Some(ParsedContext::new(
                    catalog,
                    Some(parts[0].to_string()),
                    None,
                    None,
                )),
                2 => Some(ParsedContext::new(
                    catalog,
                    Some(parts[0].to_string()),
                    Some(parts[1].to_string()),
                    None,
                )),
                3 => Some(ParsedContext::new(
                    catalog,
                    Some(parts[0].to_string()),
                    Some(parts[1].to_string()),
                    Some(parts[2].to_string()),
                )),
                _ => None,
            }
        } else {
            // Legacy format without warehouse: catalog[.namespace][.table]
            let parts: Vec<&str> = context.splitn(3, '.').collect();
            match parts.len() {
                1 => Some(ParsedContext::catalog_only(parts[0].to_string())),
                2 => Some(ParsedContext::new(
                    parts[0].to_string(),
                    None,
                    Some(parts[1].to_string()),
                    None,
                )),
                3 => Some(ParsedContext::new(
                    parts[0].to_string(),
                    None,
                    Some(parts[1].to_string()),
                    Some(parts[2].to_string()),
                )),
                _ => None,
            }
        }
    }

    /// Get the current warehouse from context (if any)
    pub fn get_current_warehouse(&self) -> Option<String> {
        self.parse_current_context().and_then(|ctx| ctx.warehouse)
    }

    /// Get the current namespace from context (if any)
    pub fn get_current_namespace(&self) -> Option<String> {
        self.parse_current_context().and_then(|ctx| ctx.namespace)
    }

    /// Get the current table name from context (if any)
    pub fn get_current_table(&self) -> Option<String> {
        self.parse_current_context().and_then(|ctx| ctx.table)
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

        // 4. If we have a current catalog context with namespace, try to use it
        //    e.g., context=polaris.demo + input=events → polaris catalog with demo.events
        if let Some(catalog_name) = self.get_current_catalog()
            && let Some(catalog) = self.catalogs.get(catalog_name)
            && let Some(ns) = self.get_current_namespace()
        {
            // Combine namespace.table
            let table_name = format!("{}.{}", ns, name_or_path);
            return Ok(ResolvedTable::Catalog {
                catalog_name: catalog_name.to_string(),
                catalog_config: Box::new(catalog.clone()),
                table_name,
            });
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

    #[test]
    fn test_validate_empty_config() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_valid_config() {
        let mut config = Config::default();
        config.add_catalog(
            "polaris".to_string(),
            CatalogConfig::rest("http://localhost:8181"),
        );
        config.set_current_catalog("polaris".to_string());
        config.add_table("events".to_string(), "s3://bucket/events".to_string());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_invalid_current_catalog() {
        let mut config = Config::default();
        config.set_current_catalog("nonexistent".to_string());
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn test_validate_empty_catalog_uri() {
        let mut config = Config::default();
        // Create a catalog config with empty URI
        let mut bad_catalog = CatalogConfig::rest("http://localhost:8181");
        bad_catalog.uri = String::new();
        config.catalogs.insert("bad".to_string(), bad_catalog);
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty URI"));
    }

    #[test]
    fn test_validate_empty_table_path() {
        let mut config = Config::default();
        config.tables.insert("bad".to_string(), String::new());
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty path"));
    }
}
