//! Local filesystem storage backend

use async_trait::async_trait;
use bytes::Bytes;

use crate::core::storage::traits::*;
use crate::error::Result;

/// Local filesystem storage backend
pub struct LocalBackend {
    // Configuration fields will be added as needed
}

impl LocalBackend {
    /// Create a new local filesystem backend
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }
}

#[async_trait]
impl StorageBackend for LocalBackend {
    fn storage_type(&self) -> &str {
        "local"
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        // TODO: Implement
        todo!("LocalBackend::exists - to be implemented by Rust-Developer")
    }

    async fn head(&self, path: &str) -> Result<ObjectMetadata> {
        // TODO: Implement
        todo!("LocalBackend::head - to be implemented by Rust-Developer")
    }

    async fn get(&self, path: &str, options: &GetOptions) -> Result<Bytes> {
        // TODO: Implement
        todo!("LocalBackend::get - to be implemented by Rust-Developer")
    }

    async fn put(&self, path: &str, data: Bytes, options: &PutOptions) -> Result<()> {
        // TODO: Implement
        todo!("LocalBackend::put - to be implemented by Rust-Developer")
    }

    async fn list(&self, options: &ListOptions) -> Result<ListResult> {
        // TODO: Implement
        todo!("LocalBackend::list - to be implemented by Rust-Developer")
    }

    async fn delete(&self, path: &str) -> Result<()> {
        // TODO: Implement
        todo!("LocalBackend::delete - to be implemented by Rust-Developer")
    }

    async fn copy(&self, from: &str, to: &str) -> Result<()> {
        // TODO: Implement
        todo!("LocalBackend::copy - to be implemented by Rust-Developer")
    }

    fn supports_atomic_operations(&self) -> bool {
        true // Local filesystem supports atomic rename
    }
}
