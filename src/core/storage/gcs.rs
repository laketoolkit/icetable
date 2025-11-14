//! Google Cloud Storage backend

use async_trait::async_trait;
use bytes::Bytes;

use crate::core::storage::traits::*;
use crate::error::Result;

/// Google Cloud Storage backend
pub struct GcsBackend {
    // Will use object_store::gcp::GoogleCloudStorage internally
}

impl GcsBackend {
    /// Create a new GCS backend
    pub async fn new() -> Result<Self> {
        // TODO: Implement
        todo!("GcsBackend::new - to be implemented by Rust-Developer")
    }
}

#[async_trait]
impl StorageBackend for GcsBackend {
    fn storage_type(&self) -> &str {
        "gcs"
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        todo!("GcsBackend::exists - to be implemented by Rust-Developer")
    }

    async fn head(&self, path: &str) -> Result<ObjectMetadata> {
        todo!("GcsBackend::head - to be implemented by Rust-Developer")
    }

    async fn get(&self, path: &str, options: &GetOptions) -> Result<Bytes> {
        todo!("GcsBackend::get - to be implemented by Rust-Developer")
    }

    async fn put(&self, path: &str, data: Bytes, options: &PutOptions) -> Result<()> {
        todo!("GcsBackend::put - to be implemented by Rust-Developer")
    }

    async fn list(&self, options: &ListOptions) -> Result<ListResult> {
        todo!("GcsBackend::list - to be implemented by Rust-Developer")
    }

    async fn delete(&self, path: &str) -> Result<()> {
        todo!("GcsBackend::delete - to be implemented by Rust-Developer")
    }

    async fn copy(&self, from: &str, to: &str) -> Result<()> {
        todo!("GcsBackend::copy - to be implemented by Rust-Developer")
    }

    fn supports_multipart(&self) -> bool {
        true
    }
}
