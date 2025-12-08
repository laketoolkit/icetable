//! Format handler registry for dynamic format registration
//!
//! Provides a plugin-style system for registering table format handlers.
//! Supports Delta Lake and Iceberg.

use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

use crate::core::storage::Storage;
use crate::error::Result;

use super::FormatHandler;
use super::traits::TimeTravelOptions;

/// Factory function type for creating format handlers
pub type FormatHandlerFactoryFn =
    Arc<dyn Fn(&Path, Storage) -> Result<Box<dyn FormatHandler>> + Send + Sync>;

/// Registry for table format handlers (Delta Lake, Iceberg)
///
/// Format handlers are checked in priority order (highest first) until one
/// successfully handles the given path.
pub struct FormatHandlerRegistry {
    /// Registered handlers: (format_name, priority, factory_fn)
    handlers: RwLock<Vec<(String, i32, FormatHandlerFactoryFn)>>,
}

impl FormatHandlerRegistry {
    /// Get the global singleton instance
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
    /// Higher priority handlers are checked first.
    pub fn register<F>(&self, name: &str, priority: i32, factory: F)
    where
        F: Fn(&Path, Storage) -> Result<Box<dyn FormatHandler>>
            + Send
            + Sync
            + 'static,
    {
        let mut handlers = self.handlers.write().expect("handler registry lock poisoned");
        handlers.push((name.to_string(), priority, Arc::new(factory)));
        // Sort by priority (descending)
        handlers.sort_by(|a, b| b.1.cmp(&a.1));
    }

    /// Try to create a handler by testing registered formats in priority order
    pub async fn create_handler(
        &self,
        path: &Path,
        storage: Storage,
    ) -> Result<Box<dyn FormatHandler>> {
        self.create_handler_with_options(path, storage, TimeTravelOptions::default())
            .await
    }

    /// Try to create a handler with time-travel options
    pub async fn create_handler_with_options(
        &self,
        path: &Path,
        storage: Storage,
        time_travel: TimeTravelOptions,
    ) -> Result<Box<dyn FormatHandler>> {
        // If time-travel options are set, create handlers directly with options
        if time_travel.is_set() {
            return self
                .create_time_travel_handler(path, storage, time_travel)
                .await;
        }

        // Otherwise, use the standard factory-based approach
        // Collect handlers first to avoid holding MutexGuard across await
        let candidate_handlers: Vec<_> = {
            let handlers = self.handlers.read().expect("handler registry lock poisoned");
            handlers
                .iter()
                .filter_map(|(_name, _priority, factory)| factory(path, storage.clone()).ok())
                .collect()
        };

        for handler in candidate_handlers {
            if handler.can_handle(path).await? {
                return Ok(handler);
            }
        }

        Err(crate::error::Error::InvalidFormat {
            message: format!("No table format handler found for: {}", path.display()),
        })
    }

    /// Create a handler with time-travel options (bypasses factory)
    async fn create_time_travel_handler(
        &self,
        path: &Path,
        storage: Storage,
        time_travel: TimeTravelOptions,
    ) -> Result<Box<dyn FormatHandler>> {
        // Try Delta Lake first
        #[cfg(feature = "delta")]
        {
            let handler =
                super::DeltaHandler::with_time_travel(path, storage.clone(), time_travel.clone())?;
            if handler.can_handle(path).await? {
                return Ok(Box::new(handler));
            }
        }

        // Try Iceberg
        {
            #[cfg(not(feature = "delta"))]
            let _ = &time_travel; // Used by Delta above when feature enabled
            let handler = super::IcebergHandler::with_time_travel(path, storage, time_travel)?;
            if handler.can_handle(path).await? {
                return Ok(Box::new(handler));
            }
        }

        Err(crate::error::Error::InvalidFormat {
            message: format!("No table format handler found for: {}", path.display()),
        })
    }

    /// Register built-in table formats
    fn register_builtin_formats(&self) {
        // Delta Lake (feature-gated)
        #[cfg(feature = "delta")]
        self.register("delta", 80, |path, storage| {
            Ok(Box::new(super::DeltaHandler::new(path, storage)?))
        });

        // Iceberg (feature-gated)
        self.register("iceberg", 80, |path, storage| {
            Ok(Box::new(super::IcebergHandler::new(path, storage)?))
        });
    }

    /// Get list of registered format names (for debugging)
    pub fn registered_formats(&self) -> Vec<String> {
        let handlers = self.handlers.read().expect("handler registry lock poisoned");
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
