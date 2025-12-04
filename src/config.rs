//! Configuration management for icectl
//!
//! Stores configuration in ~/.config/icectl/config.toml

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{Error, Result};

/// Configuration file name
const CONFIG_DIR: &str = "icectl";
const CONFIG_FILE: &str = "config.toml";

/// icectl configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Current table context (like kubectl current-context)
    #[serde(default)]
    pub current_table: Option<String>,

    /// Named table aliases for quick access
    #[serde(default)]
    pub tables: std::collections::HashMap<String, TableConfig>,
}

/// Configuration for a named table
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableConfig {
    /// Table path (local or s3://, gs://, etc.)
    pub path: String,

    /// Optional description
    #[serde(default)]
    pub description: Option<String>,
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
        let config_path = Self::config_path()?;

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| Error::General(format!("Failed to read config file: {}", e)))?;

        toml::from_str(&content)
            .map_err(|e| Error::General(format!("Failed to parse config file: {}", e)))
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let config_dir = Self::config_dir()?;
        let config_path = Self::config_path()?;

        // Create config directory if it doesn't exist
        std::fs::create_dir_all(&config_dir)
            .map_err(|e| Error::General(format!("Failed to create config directory: {}", e)))?;

        let content = toml::to_string_pretty(self)
            .map_err(|e| Error::General(format!("Failed to serialize config: {}", e)))?;

        std::fs::write(&config_path, content)
            .map_err(|e| Error::General(format!("Failed to write config file: {}", e)))?;

        Ok(())
    }

    /// Set the current table
    pub fn set_current_table(&mut self, path: String) {
        self.current_table = Some(path);
    }

    /// Unset the current table
    pub fn unset_current_table(&mut self) {
        self.current_table = None;
    }

    /// Get the current table path
    pub fn get_current_table(&self) -> Option<&str> {
        self.current_table.as_deref()
    }

    /// Add a named table alias
    pub fn add_table(&mut self, name: String, path: String, description: Option<String>) {
        self.tables.insert(name, TableConfig { path, description });
    }

    /// Remove a named table alias
    pub fn remove_table(&mut self, name: &str) -> bool {
        self.tables.remove(name).is_some()
    }

    /// Get a table by name or return the path as-is
    pub fn resolve_table(&self, name_or_path: &str) -> String {
        if let Some(table) = self.tables.get(name_or_path) {
            table.path.clone()
        } else {
            name_or_path.to_string()
        }
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
        match self {
            Some(path) => Ok(config.resolve_table(path)),
            None => config
                .get_current_table()
                .map(|s| s.to_string())
                .ok_or_else(|| {
                    Error::General(
                        "No table specified. Use -t <path> or set default with 'icectl config use <path>'".to_string()
                    )
                }),
        }
    }
}
