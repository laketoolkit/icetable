//! Format handler registry for dynamic format registration
//!
//! Provides a plugin-style system for registering format handlers without
//! modifying core code. Handlers are checked in priority order.

use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

use crate::core::storage::StorageBackend;
use crate::error::Result;

use super::FormatHandler;

/// Factory function type for creating format handlers
pub type FormatHandlerFactoryFn =
    Arc<dyn Fn(&Path, Arc<dyn StorageBackend>) -> Result<Box<dyn FormatHandler>> + Send + Sync>;

/// Registry for format handlers with priority-based detection
///
/// Format handlers are checked in priority order (highest first) until one
/// successfully handles the given path. This allows for:
/// - Custom formats to override built-in detection
/// - Efficient format detection (check fast formats first)
/// - Plugin-style extensibility
///
/// # Example
///
/// ```ignore
/// use tabletools::core::formats::FormatHandlerRegistry;
///
/// // Register a custom format
/// FormatHandlerRegistry::global().register("xml", 75, |path, storage| {
///     Ok(Box::new(XmlHandler::new(path, storage)?))
/// });
/// ```
pub struct FormatHandlerRegistry {
    /// Registered handlers: (format_name, priority, factory_fn)
    handlers: RwLock<Vec<(String, i32, FormatHandlerFactoryFn)>>,
}

impl FormatHandlerRegistry {
    /// Get the global singleton instance
    ///
    /// The registry is initialized once with built-in formats. Additional
    /// formats can be registered at any time.
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<FormatHandlerRegistry> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            let registry = FormatHandlerRegistry::new();
            registry.register_builtin_formats();
            registry
        })
    }

    /// Create a new empty registry
    fn new() -> Self {
        Self {
            handlers: RwLock::new(Vec::new()),
        }
    }

    /// Register a format handler with priority
    ///
    /// Higher priority handlers are checked first. Built-in formats use:
    /// - Parquet: 100 (most common, fastest detection)
    /// - Arrow IPC: 90
    /// - Delta/Iceberg: 80 (table formats)
    /// - CSV: 50 (ambiguous, slower detection)
    /// - JSON: 50
    ///
    /// Custom formats should use priorities between 0-200 based on specificity.
    ///
    /// # Arguments
    ///
    /// * `name` - Format name for debugging/logging
    /// * `priority` - Detection priority (higher = checked first)
    /// * `factory` - Function to create handler instances
    pub fn register<F>(&self, name: &str, priority: i32, factory: F)
    where
        F: Fn(&Path, Arc<dyn StorageBackend>) -> Result<Box<dyn FormatHandler>>
            + Send
            + Sync
            + 'static,
    {
        let mut handlers = self.handlers.write().unwrap();
        handlers.push((name.to_string(), priority, Arc::new(factory)));
        // Sort by priority (descending)
        handlers.sort_by(|a, b| b.1.cmp(&a.1));
    }

    /// Try to create a handler by testing registered formats in priority order
    ///
    /// Each registered factory is called and its `can_handle` method is checked
    /// until a handler succeeds. Returns an error if no handler can process the path.
    pub async fn create_handler(
        &self,
        path: &Path,
        storage: Arc<dyn StorageBackend>,
    ) -> Result<Box<dyn FormatHandler>> {
        let handlers = self.handlers.read().unwrap();

        for (_name, _priority, factory) in handlers.iter() {
            match factory(path, storage.clone()) {
                Ok(handler) => {
                    if handler.can_handle(path).await? {
                        return Ok(handler);
                    }
                }
                Err(_) => continue, // Try next handler
            }
        }

        Err(crate::error::Error::InvalidFormat {
            message: format!("No handler found for path: {}", path.display()),
        })
    }

    /// Register built-in formats with default priorities
    fn register_builtin_formats(&self) {
        use super::{ArrowHandler, CsvHandler, JsonHandler, ParquetHandler};

        // Parquet gets highest priority (100) - most common in data engineering
        self.register("parquet", 100, |path, storage| {
            Ok(Box::new(ParquetHandler::new(path, storage)?))
        });

        // Arrow IPC
        self.register("arrow", 90, |path, storage| {
            Ok(Box::new(ArrowHandler::new(path, storage)?))
        });

        // CSV (lower priority due to ambiguous detection)
        self.register("csv", 50, |path, storage| {
            Ok(Box::new(CsvHandler::new(path, storage)?))
        });

        // JSON/NDJSON
        self.register("json", 50, |path, storage| {
            Ok(Box::new(JsonHandler::new(path, storage)?))
        });

        // Delta Lake (feature-gated)
        #[cfg(feature = "delta")]
        self.register("delta", 80, |path, storage| {
            Ok(Box::new(super::DeltaHandler::new(path, storage)?))
        });

        // Iceberg (feature-gated)
        #[cfg(feature = "iceberg")]
        self.register("iceberg", 80, |path, storage| {
            Ok(Box::new(super::IcebergHandler::new(path, storage)?))
        });
    }

    /// Get list of registered format names (for debugging)
    pub fn registered_formats(&self) -> Vec<String> {
        let handlers = self.handlers.read().unwrap();
        handlers.iter().map(|(name, _, _)| name.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = FormatHandlerRegistry::new();
        let formats = registry.registered_formats();
        assert_eq!(formats.len(), 0);
    }

    #[test]
    fn test_global_registry_has_builtin_formats() {
        let registry = FormatHandlerRegistry::global();
        let formats = registry.registered_formats();

        // Should have at least: parquet, arrow, csv, json
        assert!(formats.contains(&"parquet".to_string()));
        assert!(formats.contains(&"arrow".to_string()));
        assert!(formats.contains(&"csv".to_string()));
        assert!(formats.contains(&"json".to_string()));
    }

    #[test]
    fn test_priority_ordering() {
        let registry = FormatHandlerRegistry::new();

        // Register in random order
        registry.register("low", 10, |_, _| {
            Err(crate::error::Error::General("test".to_string()))
        });
        registry.register("high", 100, |_, _| {
            Err(crate::error::Error::General("test".to_string()))
        });
        registry.register("medium", 50, |_, _| {
            Err(crate::error::Error::General("test".to_string()))
        });

        let formats = registry.registered_formats();
        // Should be sorted by priority: high, medium, low
        assert_eq!(formats[0], "high");
        assert_eq!(formats[1], "medium");
        assert_eq!(formats[2], "low");
    }
}
