//! Azure Blob Storage backend

use async_trait::async_trait;
use bytes::Bytes;

use crate::core::storage::traits::*;
use crate::error::Result;

/// Azure Blob Storage backend
pub struct AzureBackend {
    // Will use object_store::azure::MicrosoftAzure internally
}

impl AzureBackend {
    /// Create a new Azure backend
    pub async fn new() -> Result<Self> {
        // TODO: Implement
        todo!("AzureBackend::new - to be implemented by Rust-Developer")
    }
}

#[async_trait]
impl StorageBackend for AzureBackend {
    fn storage_type(&self) -> &str {
        "azure"
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        todo!("AzureBackend::exists - to be implemented by Rust-Developer")
    }

    async fn head(&self, path: &str) -> Result<ObjectMetadata> {
        todo!("AzureBackend::head - to be implemented by Rust-Developer")
    }

    async fn get(&self, path: &str, options: &GetOptions) -> Result<Bytes> {
        todo!("AzureBackend::get - to be implemented by Rust-Developer")
    }

    async fn put(&self, path: &str, data: Bytes, options: &PutOptions) -> Result<()> {
        todo!("AzureBackend::put - to be implemented by Rust-Developer")
    }

    async fn list(&self, options: &ListOptions) -> Result<ListResult> {
        todo!("AzureBackend::list - to be implemented by Rust-Developer")
    }

    async fn delete(&self, path: &str) -> Result<()> {
        todo!("AzureBackend::delete - to be implemented by Rust-Developer")
    }

    async fn copy(&self, from: &str, to: &str) -> Result<()> {
        todo!("AzureBackend::copy - to be implemented by Rust-Developer")
    }

    fn supports_multipart(&self) -> bool {
        true
    }
}
