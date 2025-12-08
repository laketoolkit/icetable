//! Factory for creating Iceberg inspectors

use async_trait::async_trait;
use std::path::PathBuf;

use super::IcebergInspector;
use crate::core::inspection::PhysicalInspectorFactory;
use crate::core::inspection::traits::PhysicalInspector;
use crate::core::storage::Storage;
use crate::error::Result;

/// Factory for creating Iceberg inspectors
pub struct IcebergInspectorFactory;

#[async_trait]
impl PhysicalInspectorFactory for IcebergInspectorFactory {
    fn create(&self, path: &str, storage: Storage) -> Result<Box<dyn PhysicalInspector>> {
        Ok(Box::new(IcebergInspector::new(
            PathBuf::from(path),
            storage,
        )))
    }

    async fn can_handle(&self, _path: &str, storage: &Storage) -> bool {
        // The storage is already configured with the table path as prefix.
        // We just need to check if there's a metadata/ directory.
        use futures::TryStreamExt;

        let prefix_path = crate::core::storage::to_path("metadata/");
        let mut stream = storage.list(Some(&prefix_path));

        // Check if we can get at least one item in metadata/
        matches!(stream.try_next().await, Ok(Some(_)))
    }

    fn priority(&self) -> i32 {
        75
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iceberg_factory_priority() {
        let factory = IcebergInspectorFactory;
        assert_eq!(factory.priority(), 75);
    }
}
