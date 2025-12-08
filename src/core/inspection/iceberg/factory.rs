//! Factory for creating Iceberg inspectors

use async_trait::async_trait;
use std::path::Path;

use super::IcebergInspector;
use crate::core::inspection::PhysicalInspectorFactory;
use crate::core::inspection::traits::PhysicalInspector;
use crate::core::storage::Storage;
use crate::error::Result;

/// Factory for creating Iceberg inspectors
pub struct IcebergInspectorFactory;

#[async_trait]
impl PhysicalInspectorFactory for IcebergInspectorFactory {
    fn create(
        &self,
        path: &Path,
        storage: Storage,
    ) -> Result<Box<dyn PhysicalInspector>> {
        Ok(Box::new(IcebergInspector::new(path.to_path_buf(), storage)))
    }

    async fn can_handle(&self, path: &Path, storage: &Storage) -> bool {
        // Check for metadata directory by trying to list files in it
        let path_str = path.to_str().unwrap_or("");

        // Strip the scheme (s3://, file://, etc.) if present, then strip bucket/container
        let clean_path = if let Some(pos) = path_str.find("://") {
            let after_scheme = &path_str[pos + 3..];
            // For cloud storage, strip the bucket/container name (first path segment)
            if let Some(slash_pos) = after_scheme.find('/') {
                &after_scheme[slash_pos + 1..]
            } else {
                // Just the bucket name, no path
                ""
            }
        } else {
            path_str
        };

        let metadata_prefix = if clean_path.is_empty() {
            "metadata/".to_string()
        } else {
            format!("{}/metadata/", clean_path)
        };

        use futures::TryStreamExt;

        let prefix_path = crate::core::storage::to_path(&metadata_prefix);
        let mut stream = storage.list(Some(&prefix_path));

        // Check if we can get at least one item
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
