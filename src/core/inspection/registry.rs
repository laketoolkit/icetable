//! Registry for physical inspectors (Iceberg)

use super::traits::PhysicalInspector;
use crate::core::storage::Storage;
use crate::error::{Error, Result};

/// Factory trait for creating physical inspectors
#[async_trait::async_trait]
pub trait PhysicalInspectorFactory: Send + Sync {
    /// Create an inspector for the given path (URL or local path) and storage
    fn create(&self, path: &str, storage: Storage) -> Result<Box<dyn PhysicalInspector>>;

    /// Check if this factory can handle the given path
    /// The storage is already configured for this path, so list operations
    /// should use relative paths (e.g., "metadata/" not "{table_path}/metadata/")
    async fn can_handle(&self, path: &str, storage: &Storage) -> bool;

    /// Get the priority of this factory (higher = checked first)
    fn priority(&self) -> i32 {
        0
    }
}

/// Registry for managing physical inspectors (Iceberg)
pub struct PhysicalInspectorRegistry {
    factories: Vec<Box<dyn PhysicalInspectorFactory>>,
}

impl PhysicalInspectorRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            factories: Vec::new(),
        }
    }

    /// Create a registry with default inspectors for table formats
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();

        // Register Iceberg inspector (priority 75)
        registry.register(super::iceberg::IcebergInspectorFactory);

        registry
    }

    /// Register a new inspector factory
    pub fn register<F: PhysicalInspectorFactory + 'static>(&mut self, factory: F) {
        self.factories.push(Box::new(factory));
        // Sort by priority (highest first)
        self.factories
            .sort_by_key(|f| std::cmp::Reverse(f.priority()));
    }

    /// Create an inspector for the given path (URL or local path)
    pub async fn create_inspector(
        &self,
        path: &str,
        storage: Storage,
    ) -> Result<Box<dyn PhysicalInspector>> {
        for factory in &self.factories {
            if factory.can_handle(path, &storage).await {
                return factory.create(path, storage);
            }
        }

        Err(Error::InvalidFormat {
            message: format!("No table format inspector found for: {}", path),
        })
    }

    /// Check if any factory can handle the given path
    pub async fn can_handle(&self, path: &str, storage: &Storage) -> bool {
        for factory in &self.factories {
            if factory.can_handle(path, storage).await {
                return true;
            }
        }
        false
    }
}

impl Default for PhysicalInspectorRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}
