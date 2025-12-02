//! High-level service for physical inspection

use super::registry::PhysicalInspectorRegistry;
use super::traits::PhysicalInspectOptions;
use super::view_builder::{InspectionView, InspectionViewBuilder};
use crate::core::storage::StorageBackendFactory;
use crate::error::Result;
use std::path::Path;

/// High-level service for physical inspection
pub struct PhysicalInspectionService {
    registry: PhysicalInspectorRegistry,
}

impl PhysicalInspectionService {
    /// Create a new inspection service
    pub fn new() -> Self {
        Self {
            registry: PhysicalInspectorRegistry::with_defaults(),
        }
    }

    /// Create a new inspection service with custom registry
    pub fn with_registry(registry: PhysicalInspectorRegistry) -> Self {
        Self {
            registry,
        }
    }

    /// Inspect a path and return view ready for rendering
    pub async fn inspect(
        &self,
        path: &Path,
        options: PhysicalInspectOptions,
    ) -> Result<InspectionView> {
        // 1. Create storage backend
        let path_str = path.to_str().ok_or_else(|| {
            crate::error::Error::General(
                "Invalid path: contains non-UTF8 characters".to_string()
            )
        })?;
        let storage = StorageBackendFactory::create_backend(path_str).await?;

        // 2. Detect format and create inspector
        let inspector = self.registry.create_inspector(path, storage).await?;

        // 3. Extract metadata
        let metadata = inspector.extract_metadata(&options).await?;

        // 4. Build view
        let mut view_builder = InspectionViewBuilder::new()
            .with_file_info(&metadata.file_info);

        if let Some(schema) = &metadata.schema {
            view_builder = view_builder.with_schema(schema);
        }

        if let Some(layout) = &metadata.layout {
            view_builder = view_builder.with_layout(layout);
        }

        if let Some(stats) = &metadata.statistics {
            view_builder = view_builder.with_statistics(stats);
        }

        Ok(view_builder.build())
    }
}

impl Default for PhysicalInspectionService {
    fn default() -> Self {
        Self::new()
    }
}
