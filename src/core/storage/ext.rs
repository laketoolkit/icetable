//! Extension traits for object_store
//!
//! Provides convenience methods on top of the standard ObjectStore trait.

use async_trait::async_trait;
use bytes::Bytes;
use futures::TryStreamExt;
use object_store::path::Path;
use object_store::{ObjectMeta, ObjectStore};

use crate::error::{Error, Result};

/// Extension trait adding convenience methods to ObjectStore
///
/// This trait provides a simpler API that matches the old StorageBackend interface,
/// making migration easier.
#[async_trait]
pub trait ObjectStoreExt: ObjectStore {
    /// Check if an object exists at the given path
    async fn exists(&self, path: &Path) -> Result<bool> {
        match self.head(path).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(Error::from(e)),
        }
    }

    /// Check if an object exists (string path version)
    async fn exists_str(&self, path: &str) -> Result<bool> {
        self.exists(&to_path(path)).await
    }

    /// Get the full contents of an object as bytes
    async fn get_bytes(&self, path: &Path) -> Result<Bytes> {
        let result = self.get(path).await.map_err(Error::from)?;
        result.bytes().await.map_err(Error::from)
    }

    /// Get the full contents of an object as bytes (string path version)
    async fn get_bytes_str(&self, path: &str) -> Result<Bytes> {
        self.get_bytes(&to_path(path)).await
    }

    /// Get a range of bytes from an object
    async fn get_range_bytes(&self, path: &Path, start: u64, end: u64) -> Result<Bytes> {
        self.get_range(path, start..end).await.map_err(Error::from)
    }

    /// List all objects with a given prefix, collecting into a Vec
    async fn list_all(&self, prefix: Option<&Path>) -> Result<Vec<ObjectMeta>> {
        let stream = self.list(prefix);
        stream.try_collect().await.map_err(Error::from)
    }

    /// List all objects with a string prefix
    async fn list_prefix(&self, prefix: &str) -> Result<Vec<ObjectMeta>> {
        self.list_all(Some(&to_path(prefix))).await
    }

    /// Write bytes to a path
    async fn put_bytes(&self, path: &Path, data: Bytes) -> Result<()> {
        self.put(path, data.into()).await.map_err(Error::from)?;
        Ok(())
    }

    /// Write bytes to a path (string path version)
    async fn put_bytes_str(&self, path: &str, data: Bytes) -> Result<()> {
        self.put_bytes(&to_path(path), data).await
    }

    /// Delete an object (string path version)
    async fn delete_str(&self, path: &str) -> Result<()> {
        self.delete(&to_path(path)).await.map_err(Error::from)
    }

    /// Copy an object (string path version)
    async fn copy_str(&self, from: &str, to: &str) -> Result<()> {
        self.copy(&to_path(from), &to_path(to))
            .await
            .map_err(Error::from)
    }
}

// Blanket implementation for all ObjectStore implementations
impl<T: ObjectStore> ObjectStoreExt for T {}

/// Convert a string path to an object_store Path
///
/// Handles both relative paths and full URLs (s3://, gs://, az://, etc.).
/// For full URLs, extracts the path component after the bucket.
pub fn to_path(path: &str) -> Path {
    // Handle full URLs (s3://bucket/path, gs://bucket/path, etc.)
    if let Some(rest) = path
        .strip_prefix("s3://")
        .or_else(|| path.strip_prefix("s3a://"))
        .or_else(|| path.strip_prefix("gs://"))
        .or_else(|| path.strip_prefix("az://"))
        .or_else(|| path.strip_prefix("abfs://"))
        .or_else(|| path.strip_prefix("abfss://"))
    {
        // Skip the bucket name and get the rest of the path
        if let Some(slash_pos) = rest.find('/') {
            let relative_path = &rest[slash_pos + 1..];
            return Path::from(relative_path);
        }
    }

    // Strip leading slash for relative paths
    let normalized = path.trim_start_matches('/');
    Path::from(normalized)
}

/// Convert an object_store Path back to a string
pub fn from_path(path: &Path) -> String {
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_path_relative() {
        assert_eq!(to_path("data/file.parquet").as_ref(), "data/file.parquet");
        assert_eq!(to_path("/data/file.parquet").as_ref(), "data/file.parquet");
        assert_eq!(to_path("metadata/v1.json").as_ref(), "metadata/v1.json");
    }

    #[test]
    fn test_from_path() {
        let path = Path::from("data/file.parquet");
        assert_eq!(from_path(&path), "data/file.parquet");
    }
}
