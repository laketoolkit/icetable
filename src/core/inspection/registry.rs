//! Registry for physical inspectors

use super::traits::PhysicalInspector;
use crate::core::storage::StorageBackend;
use crate::error::{Error, Result};
use std::path::Path;
use std::sync::Arc;

/// Factory trait for creating physical inspectors
pub trait PhysicalInspectorFactory: Send + Sync {
    /// Create an inspector for the given path and storage
    fn create(
        &self,
        path: &Path,
        storage: Arc<dyn StorageBackend>,
    ) -> Result<Box<dyn PhysicalInspector>>;

    /// Check if this factory can handle the given path
    fn can_handle(&self, path: &Path) -> bool;

    /// Get the priority of this factory (higher = checked first)
    fn priority(&self) -> i32 {
        0
    }
}

/// Registry for managing physical inspectors
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

    /// Create a registry with default inspectors
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();

        // Register Parquet inspector
        registry.register(super::parquet::ParquetInspectorFactory);

        registry
    }

    /// Register a new inspector factory
    pub fn register<F: PhysicalInspectorFactory + 'static>(&mut self, factory: F) {
        self.factories.push(Box::new(factory));
        // Sort by priority (highest first)
        self.factories.sort_by(|a, b| b.priority().cmp(&a.priority()));
    }

    /// Create an inspector for the given path
    pub fn create_inspector(
        &self,
        path: &Path,
        storage: Arc<dyn StorageBackend>,
    ) -> Result<Box<dyn PhysicalInspector>> {
        for factory in &self.factories {
            if factory.can_handle(path) {
                return factory.create(path, storage);
            }
        }

        Err(Error::InvalidFormat {
            message: format!(
                "No physical inspector found for: {}",
                path.display()
            ),
        })
    }

    /// Check if any factory can handle the given path
    pub fn can_handle(&self, path: &Path) -> bool {
        self.factories.iter().any(|f| f.can_handle(path))
    }
}

impl Default for PhysicalInspectorRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}
