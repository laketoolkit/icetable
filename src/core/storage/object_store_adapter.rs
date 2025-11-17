//! ObjectStore adapter for DataFusion
//!
//! This module bridges our StorageBackend trait with DataFusion's ObjectStore trait,
//! allowing DataFusion to read files through our storage abstraction layer.

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use futures::stream::BoxStream;
use object_store::path::Path as ObjectPath;
use object_store::{
    GetOptions as OSGetOptions, GetResult, ListResult as OSListResult, MultipartUpload, ObjectMeta,
    ObjectStore, PutMultipartOptions, PutOptions as OSPutOptions, PutPayload, PutResult,
    Result as OSResult,
};
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use super::traits::{GetOptions, ListOptions, ObjectMetadata, PutOptions, StorageBackend};
use crate::error::Error;

#[cfg(test)]
use crate::error::Result;

/// Adapter that implements object_store::ObjectStore using our StorageBackend
///
/// This allows DataFusion to use our storage abstraction for streaming reads.
/// Currently supports local filesystem with plans to extend to S3, GCS, and Azure.
#[derive(Clone)]
pub struct ObjectStoreAdapter {
    backend: Arc<dyn StorageBackend>,
    base_path: String,
}

impl fmt::Debug for ObjectStoreAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObjectStoreAdapter")
            .field("storage_type", &self.backend.storage_type())
            .field("base_path", &self.base_path)
            .finish()
    }
}

impl ObjectStoreAdapter {
    /// Create a new ObjectStore adapter from a StorageBackend
    ///
    /// # Arguments
    ///
    /// * `backend` - The storage backend to adapt
    /// * `base_path` - Base path for resolving relative paths (e.g., "/tmp" or "s3://bucket")
    pub fn new(backend: Arc<dyn StorageBackend>, base_path: String) -> Self {
        Self { backend, base_path }
    }

    /// Convert ObjectStore path to our storage path format
    fn to_storage_path(&self, location: &ObjectPath) -> String {
        let path_str = location.as_ref();

        log::debug!("[ObjectStoreAdapter] to_storage_path called");
        log::debug!("[ObjectStoreAdapter]   incoming location.as_ref(): '{}'", path_str);
        log::debug!("[ObjectStoreAdapter]   self.base_path: '{}'", self.base_path);

        // Local filesystem: DataFusion may pass paths with or without leading slash
        // Example: location = "home/user/data/file.parquet" (slash removed by DataFusion)
        // We need to ensure it's an absolute path
        let result = if self.base_path.starts_with('/') {
            log::debug!("[ObjectStoreAdapter]   branch: local filesystem");
            if path_str.starts_with('/') {
                // Path is already absolute
                log::debug!("[ObjectStoreAdapter]   sub-branch: path already absolute");
                path_str.to_string()
            } else {
                // DataFusion removed leading slash - check if it's actually an absolute path
                let base_path_no_slash = self.base_path.trim_start_matches('/');
                if path_str.starts_with(base_path_no_slash) {
                    // It's an absolute path without leading slash - just add it back
                    log::debug!("[ObjectStoreAdapter]   sub-branch: absolute path without leading slash, adding it back");
                    format!("/{}", path_str)
                } else {
                    // Truly relative path - concatenate with base_path
                    log::debug!("[ObjectStoreAdapter]   sub-branch: truly relative path, concatenating with base_path");
                    format!("{}/{}", self.base_path.trim_end_matches('/'), path_str)
                }
            }
        }
        // Cloud storage (S3/GCS/Azure): DataFusion passes paths relative to bucket
        // Example: base_path = "s3://bucket", location = "path/file.parquet"
        // We concatenate them: "s3://bucket/path/file.parquet"
        else if self.base_path.starts_with("s3://")
            || self.base_path.starts_with("gs://")
            || self.base_path.starts_with("az://")
        {
            log::debug!("[ObjectStoreAdapter]   branch: cloud storage (S3/GCS/Azure)");
            format!("{}/{}", self.base_path.trim_end_matches('/'), path_str)
        }
        // Fallback: return as-is
        else {
            log::debug!("[ObjectStoreAdapter]   branch: fallback (return as-is)");
            path_str.to_string()
        };

        log::debug!("[ObjectStoreAdapter]   final path returned: '{}'", result);
        result
    }

    /// Convert our ObjectMetadata to ObjectStore's ObjectMeta
    fn to_object_meta(&self, metadata: ObjectMetadata, location: ObjectPath) -> ObjectMeta {
        ObjectMeta {
            location,
            last_modified: metadata.last_modified,
            size: metadata.size, // ObjectMeta.size is u64 in object_store 0.12.4
            e_tag: metadata.e_tag,
            version: None,
        }
    }

    /// Convert our Error to ObjectStore error
    fn to_object_store_error(error: Error) -> object_store::Error {
        match error {
            Error::FileNotFound { path } => object_store::Error::NotFound {
                path: path.to_string_lossy().to_string(),
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("File not found: {:?}", path),
                )),
            },
            Error::PermissionDenied { path } => object_store::Error::Generic {
                store: "storage_backend",
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("Permission denied: {:?}", path),
                )),
            },
            Error::Io(e) => object_store::Error::Generic {
                store: "storage_backend",
                source: Box::new(e),
            },
            other => object_store::Error::Generic {
                store: "storage_backend",
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Storage error: {}", other),
                )),
            },
        }
    }
}

impl fmt::Display for ObjectStoreAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ObjectStoreAdapter({}:{})",
            self.backend.storage_type(),
            self.base_path
        )
    }
}

#[async_trait]
impl ObjectStore for ObjectStoreAdapter {
    async fn put(&self, location: &ObjectPath, payload: PutPayload) -> OSResult<PutResult> {
        let path = self.to_storage_path(location);

        // Collect payload chunks into a single Bytes
        let mut all_bytes = Vec::new();
        for chunk in payload.into_iter() {
            all_bytes.extend_from_slice(&chunk);
        }
        let bytes = Bytes::from(all_bytes);

        let options = PutOptions::default();

        self.backend
            .put(&path, bytes, &options)
            .await
            .map_err(Self::to_object_store_error)?;

        Ok(PutResult {
            e_tag: None,
            version: None,
        })
    }

    async fn put_opts(
        &self,
        location: &ObjectPath,
        payload: PutPayload,
        _opts: OSPutOptions,
    ) -> OSResult<PutResult> {
        // For now, ignore opts and delegate to put()
        self.put(location, payload).await
    }

    async fn put_multipart(&self, _location: &ObjectPath) -> OSResult<Box<dyn MultipartUpload>> {
        Err(object_store::Error::NotSupported {
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Multipart upload not yet implemented",
            )),
        })
    }

    async fn put_multipart_opts(
        &self,
        location: &ObjectPath,
        _opts: PutMultipartOptions,
    ) -> OSResult<Box<dyn MultipartUpload>> {
        self.put_multipart(location).await
    }

    async fn get(&self, location: &ObjectPath) -> OSResult<GetResult> {
        let path = self.to_storage_path(location);
        let options = GetOptions::default();

        let bytes = self
            .backend
            .get(&path, &options)
            .await
            .map_err(Self::to_object_store_error)?;

        let meta = self
            .backend
            .head(&path)
            .await
            .map_err(Self::to_object_store_error)?;

        let object_meta = self.to_object_meta(meta.clone(), location.clone());

        // Full file request, so range is 0..file_size
        let file_range = 0..meta.size;

        Ok(GetResult {
            payload: object_store::GetResultPayload::Stream(
                futures::stream::once(async move { Ok(bytes) }).boxed(),
            ),
            meta: object_meta,
            range: file_range,
            attributes: Default::default(),
        })
    }

    async fn get_opts(&self, location: &ObjectPath, options: OSGetOptions) -> OSResult<GetResult> {
        let path = self.to_storage_path(location);

        // Convert ObjectStore GetOptions to our GetOptions
        let our_options = if let Some(range) = options.range {
            // GetRange is an enum with Bounded, Offset, and Suffix variants
            use object_store::GetRange;
            match range {
                GetRange::Bounded(r) => GetOptions {
                    range: Some((r.start as u64, r.end as u64)),
                    ..Default::default()
                },
                GetRange::Offset(start) => {
                    // For offset, we need to get metadata first to determine end
                    let meta = self
                        .backend
                        .head(&path)
                        .await
                        .map_err(Self::to_object_store_error)?;
                    GetOptions {
                        range: Some((start as u64, meta.size)),
                        ..Default::default()
                    }
                }
                GetRange::Suffix(n) => {
                    // For suffix, get last n bytes
                    let meta = self
                        .backend
                        .head(&path)
                        .await
                        .map_err(Self::to_object_store_error)?;
                    let start = meta.size.saturating_sub(n as u64);
                    GetOptions {
                        range: Some((start, meta.size)),
                        ..Default::default()
                    }
                }
            }
        } else {
            GetOptions::default()
        };

        let bytes = self
            .backend
            .get(&path, &our_options)
            .await
            .map_err(Self::to_object_store_error)?;

        let meta = self
            .backend
            .head(&path)
            .await
            .map_err(Self::to_object_store_error)?;

        let object_meta = self.to_object_meta(meta.clone(), location.clone());

        // Calculate the actual range in the file
        // If a range was requested, use it; otherwise it's the full file
        let actual_range = if let Some((start, end)) = our_options.range {
            start..end
        } else {
            0..meta.size
        };

        Ok(GetResult {
            payload: object_store::GetResultPayload::Stream(
                futures::stream::once(async move { Ok(bytes) }).boxed(),
            ),
            meta: object_meta,
            range: actual_range,
            attributes: Default::default(),
        })
    }

    async fn get_range(&self, location: &ObjectPath, range: Range<u64>) -> OSResult<Bytes> {
        let path = self.to_storage_path(location);

        self.backend
            .get_range(&path, range.start as u64, range.end as u64)
            .await
            .map_err(Self::to_object_store_error)
    }

    async fn head(&self, location: &ObjectPath) -> OSResult<ObjectMeta> {
        let path = self.to_storage_path(location);

        let metadata = self
            .backend
            .head(&path)
            .await
            .map_err(Self::to_object_store_error)?;

        Ok(self.to_object_meta(metadata, location.clone()))
    }

    async fn delete(&self, location: &ObjectPath) -> OSResult<()> {
        let path = self.to_storage_path(location);

        self.backend
            .delete(&path)
            .await
            .map_err(Self::to_object_store_error)
    }

    fn list(&self, prefix: Option<&ObjectPath>) -> BoxStream<'static, OSResult<ObjectMeta>> {
        let prefix_str = prefix.map(|p| {
            let p_str = p.as_ref();
            if self.base_path.starts_with('/') && !p_str.starts_with('/') {
                format!("{}/{}", self.base_path.trim_end_matches('/'), p_str)
            } else {
                p_str.to_string()
            }
        });

        let backend = self.backend.clone();

        // Create async stream
        let stream = async_stream::stream! {
            let options = ListOptions {
                prefix: prefix_str,
                ..Default::default()
            };

            match backend.list(&options).await {
                Ok(result) => {
                    for obj in result.objects {
                        let location = ObjectPath::from(obj.path.as_str());
                        let meta = ObjectMeta {
                            location,
                            last_modified: obj.last_modified,
                            size: obj.size,
                            e_tag: obj.e_tag,
                            version: None,
                        };
                        yield Ok(meta);
                    }
                }
                Err(e) => {
                    yield Err(Self::to_object_store_error(e));
                }
            }
        };

        stream.boxed()
    }

    async fn list_with_delimiter(&self, prefix: Option<&ObjectPath>) -> OSResult<OSListResult> {
        let prefix_str = prefix.map(|p| {
            let p_str = p.as_ref();
            if self.base_path.starts_with('/') && !p_str.starts_with('/') {
                format!("{}/{}", self.base_path.trim_end_matches('/'), p_str)
            } else {
                p_str.to_string()
            }
        });

        let options = ListOptions {
            prefix: prefix_str,
            delimiter: Some("/".to_string()),
            ..Default::default()
        };

        let result = self
            .backend
            .list(&options)
            .await
            .map_err(Self::to_object_store_error)?;

        let objects = result
            .objects
            .into_iter()
            .map(|obj| {
                let location = ObjectPath::from(obj.path.as_str());
                ObjectMeta {
                    location,
                    last_modified: obj.last_modified,
                    size: obj.size,
                    e_tag: obj.e_tag,
                    version: None,
                }
            })
            .collect();

        let common_prefixes = result
            .prefixes
            .into_iter()
            .map(|p| ObjectPath::from(p.as_str()))
            .collect();

        Ok(OSListResult {
            objects,
            common_prefixes,
        })
    }

    async fn copy(&self, from: &ObjectPath, to: &ObjectPath) -> OSResult<()> {
        let from_path = self.to_storage_path(from);
        let to_path = self.to_storage_path(to);

        self.backend
            .copy(&from_path, &to_path)
            .await
            .map_err(Self::to_object_store_error)
    }

    async fn copy_if_not_exists(&self, from: &ObjectPath, to: &ObjectPath) -> OSResult<()> {
        let to_path = self.to_storage_path(to);

        // Check if destination exists
        let exists = self
            .backend
            .exists(&to_path)
            .await
            .map_err(Self::to_object_store_error)?;

        if exists {
            return Err(object_store::Error::AlreadyExists {
                path: to.to_string(),
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "Destination already exists",
                )),
            });
        }

        self.copy(from, to).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::storage::LocalBackend;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_adapter_creation() {
        let backend = Arc::new(LocalBackend::new().unwrap());
        let adapter = ObjectStoreAdapter::new(backend, "/tmp".to_string());
        assert_eq!(format!("{}", adapter), "ObjectStoreAdapter(local:/tmp)");
    }

    #[tokio::test]
    async fn test_path_conversion() {
        let backend = Arc::new(LocalBackend::new().unwrap());
        let adapter = ObjectStoreAdapter::new(backend, "/tmp".to_string());

        let obj_path = ObjectPath::from("test.parquet");
        let storage_path = adapter.to_storage_path(&obj_path);
        assert_eq!(storage_path, "/tmp/test.parquet");
    }

    #[tokio::test]
    async fn test_head() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let temp_path = temp_dir.path();
        let test_file = temp_path.join("test.txt");

        tokio::fs::write(&test_file, b"test data").await?;

        let backend = Arc::new(LocalBackend::new()?);
        let adapter = ObjectStoreAdapter::new(backend, temp_path.to_string_lossy().to_string());

        let obj_path = ObjectPath::from("test.txt");
        let meta = adapter.head(&obj_path).await.unwrap();

        assert_eq!(meta.size, 9);
        assert_eq!(meta.location.as_ref(), "test.txt");

        Ok(())
    }

    #[tokio::test]
    async fn test_get() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let temp_path = temp_dir.path();
        let test_file = temp_path.join("test.txt");

        tokio::fs::write(&test_file, b"test data").await?;

        let backend = Arc::new(LocalBackend::new()?);
        let adapter = ObjectStoreAdapter::new(backend, temp_path.to_string_lossy().to_string());

        let obj_path = ObjectPath::from("test.txt");
        let result = adapter.get(&obj_path).await.unwrap();

        assert_eq!(result.meta.size, 9);

        // Read stream
        let bytes = match result.payload {
            object_store::GetResultPayload::Stream(mut stream) => {
                stream.next().await.unwrap().unwrap()
            }
            _ => panic!("Expected stream payload"),
        };

        assert_eq!(bytes.as_ref(), b"test data");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_range() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let temp_path = temp_dir.path();
        let test_file = temp_path.join("test.txt");

        tokio::fs::write(&test_file, b"0123456789").await?;

        let backend = Arc::new(LocalBackend::new()?);
        let adapter = ObjectStoreAdapter::new(backend, temp_path.to_string_lossy().to_string());

        let obj_path = ObjectPath::from("test.txt");
        let bytes = adapter.get_range(&obj_path, 2..5).await.unwrap();

        assert_eq!(bytes.as_ref(), b"234");

        Ok(())
    }
}
