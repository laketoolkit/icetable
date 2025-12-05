//! Registry for physical inspectors (Delta Lake, Iceberg)

use super::traits::PhysicalInspector;
use crate::core::storage::StorageBackend;
use crate::error::{Error, Result};
use std::path::Path;
use std::sync::Arc;

/// Factory trait for creating physical inspectors
#[async_trait::async_trait]
pub trait PhysicalInspectorFactory: Send + Sync {
    /// Create an inspector for the given path and storage
    fn create(
        &self,
        path: &Path,
        storage: Arc<dyn StorageBackend>,
    ) -> Result<Box<dyn PhysicalInspector>>;

    /// Check if this factory can handle the given path
    /// For remote storage (S3, etc.), this may need to check for directory existence
    async fn can_handle(&self, path: &Path, storage: &Arc<dyn StorageBackend>) -> bool;

    /// Get the priority of this factory (higher = checked first)
    fn priority(&self) -> i32 {
        0
    }
}

/// Registry for managing physical inspectors (Delta Lake, Iceberg)
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

        // Register Delta Lake inspector (priority 80)
        #[cfg(feature = "delta")]
        registry.register(super::delta::DeltaInspectorFactory);

        // Register Iceberg inspector (priority 75)
        registry.register(super::iceberg::IcebergInspectorFactory);

        registry
    }

    /// Register a new inspector factory
    pub fn register<F: PhysicalInspectorFactory + 'static>(&mut self, factory: F) {
        self.factories.push(Box::new(factory));
        // Sort by priority (highest first)
        self.factories
            .sort_by(|a, b| b.priority().cmp(&a.priority()));
    }

    /// Create an inspector for the given path
    pub async fn create_inspector(
        &self,
        path: &Path,
        storage: Arc<dyn StorageBackend>,
    ) -> Result<Box<dyn PhysicalInspector>> {
        for factory in &self.factories {
            if factory.can_handle(path, &storage).await {
                return factory.create(path, storage);
            }
        }

        Err(Error::InvalidFormat {
            message: format!("No table format inspector found for: {}", path.display()),
        })
    }

    /// Check if any factory can handle the given path
    pub async fn can_handle(&self, path: &Path, storage: &Arc<dyn StorageBackend>) -> bool {
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
