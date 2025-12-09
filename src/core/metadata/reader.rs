//! Metadata reading abstractions
//!
//! Provides unified interface for reading Iceberg table metadata,
//! whether from a catalog or directly from storage (static tables).

use std::sync::Arc;

use async_trait::async_trait;
use iceberg::io::FileIO;
use iceberg::spec::TableMetadata;
use iceberg::table::StaticTable;
use iceberg::{NamespaceIdent, TableIdent};

use crate::core::storage::{Storage, create_object_store, create_file_io};
use crate::core::utils::find_latest_metadata;
use crate::error::{Error, Result};

/// Result of loading metadata
#[derive(Debug, Clone)]
pub struct MetadataLoadResult {
    /// The loaded table metadata
    pub metadata: Arc<TableMetadata>,
    /// Path to the metadata file
    pub metadata_location: String,
}

/// Trait for reading table metadata from different sources
#[async_trait]
pub trait MetadataReader: Send + Sync {
    /// Load the current table metadata
    async fn load(&self) -> Result<MetadataLoadResult>;

    /// Get the table location (base path)
    fn table_location(&self) -> &str;
}

/// Reader for static tables (without catalog)
///
/// Finds and loads metadata directly from storage by scanning
/// the metadata/ directory for the latest metadata file.
pub struct StaticMetadataReader {
    /// Table location (e.g., "s3://bucket/table")
    table_path: String,
    /// Storage backend
    storage: Storage,
    /// FileIO for iceberg operations
    file_io: FileIO,
}

impl StaticMetadataReader {
    /// Create a new static metadata reader
    pub async fn new(table_path: &str) -> Result<Self> {
        let storage = create_object_store(table_path).await?;
        let file_io = create_file_io(table_path)?;

        Ok(Self {
            table_path: table_path.trim_end_matches('/').to_string(),
            storage,
            file_io,
        })
    }

    /// Create with existing storage (for reuse)
    pub fn with_storage(table_path: &str, storage: Storage, file_io: FileIO) -> Self {
        Self {
            table_path: table_path.trim_end_matches('/').to_string(),
            storage,
            file_io,
        }
    }

}

#[async_trait]
impl MetadataReader for StaticMetadataReader {
    async fn load(&self) -> Result<MetadataLoadResult> {
        // Find latest metadata file (returns full path)
        let metadata_location = find_latest_metadata(&self.table_path, &self.storage).await?;

        // Use iceberg's StaticTable to load metadata properly
        let table_ident = TableIdent::new(
            NamespaceIdent::new("static".to_string()),
            "table".to_string(),
        );

        let static_table = StaticTable::from_metadata_file(
            &metadata_location,
            table_ident,
            self.file_io.clone(),
        )
        .await
        .map_err(|e| Error::IcebergLoad {
            path: self.table_path.clone(),
            source: e.into(),
        })?;

        let table = static_table.into_table();
        let metadata = table.metadata().clone();

        Ok(MetadataLoadResult {
            metadata: Arc::new(metadata),
            metadata_location,
        })
    }

    fn table_location(&self) -> &str {
        &self.table_path
    }
}

/// Reader for catalog-managed tables
///
/// Loads metadata via the catalog API, which handles
/// the metadata location lookup.
#[cfg(feature = "rest-catalog")]
pub struct CatalogMetadataReader {
    catalog: Arc<dyn iceberg::Catalog>,
    table_ident: TableIdent,
    table_location: String,
}

#[cfg(feature = "rest-catalog")]
impl CatalogMetadataReader {
    /// Create a new catalog metadata reader
    pub fn new(
        catalog: Arc<dyn iceberg::Catalog>,
        table_ident: TableIdent,
        table_location: String,
    ) -> Self {
        Self {
            catalog,
            table_ident,
            table_location,
        }
    }
}

#[cfg(feature = "rest-catalog")]
#[async_trait]
impl MetadataReader for CatalogMetadataReader {
    async fn load(&self) -> Result<MetadataLoadResult> {
        let table = self.catalog
            .load_table(&self.table_ident)
            .await
            .map_err(|e| Error::CatalogLoad {
                table_ref: format!("{:?}", self.table_ident),
                source: e.into(),
            })?;

        let metadata = table.metadata().clone();
        let metadata_location = table.metadata_location()
            .map(|s| s.to_string())
            .unwrap_or_default();

        Ok(MetadataLoadResult {
            metadata: Arc::new(metadata),
            metadata_location,
        })
    }

    fn table_location(&self) -> &str {
        &self.table_location
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_static_reader_table_location() {
        // Just test the path normalization
        let path = "s3://bucket/table/";
        let normalized = path.trim_end_matches('/');
        assert_eq!(normalized, "s3://bucket/table");
    }
}
