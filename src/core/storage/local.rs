//! Local filesystem storage backend

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

use crate::core::storage::traits::*;
use crate::error::{Error, Result};

/// Local filesystem storage backend
pub struct LocalBackend {
    // No configuration needed for local filesystem
}

impl LocalBackend {
    /// Create a new local filesystem backend
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Convert path string to PathBuf, handling file:// URLs
    fn normalize_path(&self, path: &str) -> PathBuf {
        if let Some(p) = path.strip_prefix("file://") {
            PathBuf::from(p)
        } else {
            PathBuf::from(path)
        }
    }
}

#[async_trait]
impl StorageBackend for LocalBackend {
    fn storage_type(&self) -> &str {
        "local"
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        let path_buf = self.normalize_path(path);
        Ok(tokio::fs::try_exists(&path_buf).await?)
    }

    async fn head(&self, path: &str) -> Result<ObjectMetadata> {
        let path_buf = self.normalize_path(path);

        let metadata = tokio::fs::metadata(&path_buf).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::FileNotFound {
                    path: path_buf.clone(),
                }
            } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                Error::PermissionDenied {
                    path: path_buf.clone(),
                }
            } else {
                Error::Io(e)
            }
        })?;

        let modified = metadata.modified()?;
        let datetime: DateTime<Utc> = modified.into();

        Ok(ObjectMetadata {
            path: path.to_string(),
            size: metadata.len(),
            last_modified: datetime,
            e_tag: None,
            content_type: None,
        })
    }

    async fn get(&self, path: &str, options: &GetOptions) -> Result<Bytes> {
        let path_buf = self.normalize_path(path);

        let data = tokio::fs::read(&path_buf).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::FileNotFound {
                    path: path_buf.clone(),
                }
            } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                Error::PermissionDenied {
                    path: path_buf.clone(),
                }
            } else {
                Error::Io(e)
            }
        })?;

        // Handle range requests if specified
        if let Some((start, end)) = options.range {
            let start = start as usize;
            let end = (end as usize).min(data.len());
            if start < end && start < data.len() {
                Ok(Bytes::from(data[start..end].to_vec()))
            } else {
                Ok(Bytes::new())
            }
        } else {
            Ok(Bytes::from(data))
        }
    }

    async fn put(&self, path: &str, data: Bytes, _options: &PutOptions) -> Result<()> {
        let path_buf = self.normalize_path(path);

        // Create parent directories if they don't exist
        if let Some(parent) = path_buf.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(&path_buf, data.as_ref())
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    Error::PermissionDenied {
                        path: path_buf.clone(),
                    }
                } else {
                    Error::Io(e)
                }
            })
    }

    async fn list(&self, options: &ListOptions) -> Result<ListResult> {
        let prefix = options.prefix.as_deref().unwrap_or(".");
        let path_buf = PathBuf::from(prefix);

        if !path_buf.exists() {
            return Ok(ListResult {
                objects: Vec::new(),
                prefixes: Vec::new(),
                continuation_token: None,
            });
        }

        let mut objects = Vec::new();
        let mut prefixes = Vec::new();

        let mut entries = tokio::fs::read_dir(&path_buf).await?;

        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            let entry_path = entry.path();
            let path_str = entry_path.to_string_lossy().to_string();

            if metadata.is_file() {
                let modified = metadata.modified()?;
                let datetime: DateTime<Utc> = modified.into();

                objects.push(ObjectMetadata {
                    path: path_str,
                    size: metadata.len(),
                    last_modified: datetime,
                    e_tag: None,
                    content_type: None,
                });
            } else if metadata.is_dir() {
                prefixes.push(path_str);
            }

            // Apply max_results limit if specified
            if let Some(max) = options.max_results {
                if objects.len() + prefixes.len() >= max {
                    break;
                }
            }
        }

        Ok(ListResult {
            objects,
            prefixes,
            continuation_token: None,
        })
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let path_buf = self.normalize_path(path);
        tokio::fs::remove_file(&path_buf).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::FileNotFound {
                    path: path_buf.clone(),
                }
            } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                Error::PermissionDenied {
                    path: path_buf.clone(),
                }
            } else {
                Error::Io(e)
            }
        })
    }

    async fn copy(&self, from: &str, to: &str) -> Result<()> {
        let from_buf = self.normalize_path(from);
        let to_buf = self.normalize_path(to);

        // Create parent directories if they don't exist
        if let Some(parent) = to_buf.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::copy(&from_buf, &to_buf).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::FileNotFound {
                    path: from_buf.clone(),
                }
            } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                Error::PermissionDenied {
                    path: from_buf.clone(),
                }
            } else {
                Error::Io(e)
            }
        })?;

        Ok(())
    }

    fn supports_atomic_operations(&self) -> bool {
        true // Local filesystem supports atomic rename
    }
}
