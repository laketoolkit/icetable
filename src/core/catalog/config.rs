//! Catalog configuration types

/// Type of catalog backend
#[derive(Debug, Clone, PartialEq)]
pub enum CatalogType {
    /// REST Catalog (Nessie, Polaris, Tabular, etc.)
    Rest,
    // Future: Glue, Hive, etc.
}

/// Configuration for a catalog connection
#[derive(Debug, Clone)]
pub struct CatalogConfig {
    /// Type of catalog
    pub catalog_type: CatalogType,
    /// URI of the catalog service
    pub uri: String,
    /// Warehouse location (optional, some catalogs provide this)
    pub warehouse: Option<String>,
    /// Credentials (format depends on catalog type)
    pub credential: Option<String>,
    /// Additional properties
    pub properties: std::collections::HashMap<String, String>,
}

impl CatalogConfig {
    /// Create a new REST catalog configuration
    pub fn rest(uri: impl Into<String>) -> Self {
        Self {
            catalog_type: CatalogType::Rest,
            uri: uri.into(),
            warehouse: None,
            credential: None,
            properties: std::collections::HashMap::new(),
        }
    }

    /// Set warehouse location
    pub fn with_warehouse(mut self, warehouse: impl Into<String>) -> Self {
        self.warehouse = Some(warehouse.into());
        self
    }

    /// Set credentials
    pub fn with_credential(mut self, credential: impl Into<String>) -> Self {
        self.credential = Some(credential.into());
        self
    }

    /// Add a property
    pub fn with_property(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.properties.insert(key.into(), value.into());
        self
    }
}
