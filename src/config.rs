//! Configuration management for icectl
//!
//! Stores configuration in ~/.config/icectl/config.yaml

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub use crate::core::catalog::CatalogConfig;
use crate::error::{Error, Result};

/// Configuration file name
const CONFIG_DIR: &str = "icectl";
const CONFIG_FILE: &str = "config.yaml";

/// icectl configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Current context (like kubectl current-context)
    /// Can be: table alias, path, or catalog.table
    #[serde(default)]
    pub current_context: Option<String>,

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
            .ok_or_else(|| Error::General("Could not determine config directory".to_string()))?
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
            let content = std::fs::read_to_string(&yaml_path)
                .map_err(|e| Error::General(format!("Failed to read config file: {}", e)))?;

            return serde_yaml::from_str(&content)
                .map_err(|e| Error::General(format!("Failed to parse config file: {}", e)));
        }

        Ok(Self::default())
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let config_dir = Self::config_dir()?;
        let config_path = Self::config_path()?;

        // Create config directory if it doesn't exist
        std::fs::create_dir_all(&config_dir)
            .map_err(|e| Error::General(format!("Failed to create config directory: {}", e)))?;

        let content = serde_yaml::to_string(self)
            .map_err(|e| Error::General(format!("Failed to serialize config: {}", e)))?;

        std::fs::write(&config_path, content)
            .map_err(|e| Error::General(format!("Failed to write config file: {}", e)))?;

        Ok(())
    }

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

    /// Add a named table alias
    pub fn add_table(&mut self, name: String, path: String) {
        self.tables.insert(name, path);
    }

    /// Remove a named table alias
    pub fn remove_table(&mut self, name: &str) -> bool {
        self.tables.remove(name).is_some()
    }

    /// Add a catalog configuration
    pub fn add_catalog(&mut self, name: String, config: CatalogConfig) {
        self.catalogs.insert(name, config);
    }

    /// Remove a catalog
    pub fn remove_catalog(&mut self, name: &str) -> bool {
        self.catalogs.remove(name).is_some()
    }

    /// Resolve a table reference to a path or catalog info
    /// Returns: ResolvedTable with either a direct path or catalog + table name
    pub fn resolve_table(&self, name_or_path: &str) -> Result<ResolvedTable> {
        // 1. Direct path (starts with s3://, gs://, file://, /, etc.)
        if is_direct_path(name_or_path) {
            return Ok(ResolvedTable::Path(name_or_path.to_string()));
        }

        // 2. Check if it's a catalog.table reference
        if let Some((catalog_name, table_name)) = name_or_path.split_once('.')
            && let Some(catalog) = self.catalogs.get(catalog_name)
        {
            return Ok(ResolvedTable::Catalog {
                catalog_name: catalog_name.to_string(),
                catalog_config: catalog.clone(),
                table_name: table_name.to_string(),
            });
        }

        // 3. Check if it's a table alias
        if let Some(path) = self.tables.get(name_or_path) {
            return Ok(ResolvedTable::Path(path.clone()));
        }

        // 4. Not found
        Err(Error::General(format!(
            "Unknown table or catalog reference: '{}'. \
            Use a direct path (s3://...), a configured table alias, or catalog.table format.",
            name_or_path
        )))
    }
}

/// Check if a string looks like a direct path
fn is_direct_path(s: &str) -> bool {
    // Cloud/remote paths
    s.starts_with("s3://")
        || s.starts_with("s3a://")
        || s.starts_with("gs://")
        || s.starts_with("gcs://")
        || s.starts_with("abfs://")
        || s.starts_with("abfss://")
        || s.starts_with("file://")
        // Absolute paths
        || s.starts_with('/')
        // Relative paths (contains path separator or starts with ./)
        || s.contains(std::path::MAIN_SEPARATOR)
        || s.starts_with("./")
        || s.starts_with("../")
}

/// Resolved table reference
#[derive(Debug, Clone)]
pub enum ResolvedTable {
    /// Direct path to table
    Path(String),
    /// Reference via catalog
    Catalog {
        /// Name of the catalog in config
        catalog_name: String,
        /// Configuration for the catalog
        catalog_config: CatalogConfig,
        /// Table identifier within the catalog
        table_name: String,
    },
}

impl ResolvedTable {
    /// Get the path if this is a direct path resolution
    pub fn as_path(&self) -> Option<&str> {
        match self {
            ResolvedTable::Path(p) => Some(p),
            ResolvedTable::Catalog { .. } => None,
        }
    }

    /// Check if this is a catalog reference
    pub fn is_catalog(&self) -> bool {
        matches!(self, ResolvedTable::Catalog { .. })
    }
}

/// Extension trait for resolving table paths from Option<String>
///
/// This provides a clean API: `args.path.resolve()?`
pub trait ResolvePath {
    /// Resolve the path using config defaults if None
    fn resolve(&self) -> Result<String>;
}

impl ResolvePath for Option<String> {
    fn resolve(&self) -> Result<String> {
        let config = Config::load()?;
        let reference = match self {
            Some(path) => path.clone(),
            None => config
                .get_current_context()
                .map(|s| s.to_string())
                .ok_or_else(|| {
                    Error::General(
                        "No table specified. Use -t <path> or set default with 'icectl config use <path>'".to_string()
                    )
                })?,
        };

        match config.resolve_table(&reference)? {
            ResolvedTable::Path(path) => Ok(path),
            ResolvedTable::Catalog {
                catalog_name,
                table_name,
                ..
            } => {
                // For ResolvePath we only support direct paths
                // Use ResolveTableRef for catalog support
                Err(Error::General(format!(
                    "Use inspect command with catalog tables: {}.{}",
                    catalog_name, table_name
                )))
            }
        }
    }
}

/// Extension trait for resolving table references from Option<String>
///
/// This provides a clean API: `args.path.resolve_ref()?`
/// Returns the full ResolvedTable including catalog information
pub trait ResolveTableRef {
    /// Resolve to a full table reference (path or catalog)
    fn resolve_ref(&self) -> Result<ResolvedTable>;
}

impl ResolveTableRef for Option<String> {
    fn resolve_ref(&self) -> Result<ResolvedTable> {
        let config = Config::load()?;
        let reference = match self {
            Some(path) => path.clone(),
            None => config
                .get_current_context()
                .map(|s| s.to_string())
                .ok_or_else(|| {
                    Error::General(
                        "No table specified. Use -t <path> or set default with 'icectl config use <path>'".to_string()
                    )
                })?,
        };

        config.resolve_table(&reference)
    }
}
