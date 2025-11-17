//! Iceberg inspection

use std::path::Path;
use std::sync::Arc;

use crate::core::storage::StorageBackend;
use crate::error::Result;

use super::common::*;

/// Inspect Iceberg table
#[cfg(feature = "iceberg")]
pub async fn inspect_iceberg_layout(
    path: &Path,
    _storage: Arc<dyn StorageBackend>,
    _options: &PhysicalInspectOptions,
) -> Result<PhysicalInspectResult> {
    // Placeholder for Iceberg implementation
    let file_info = vec![
        kv_item("Path", path.display().to_string(), 20),
        kv_item("Format", "Apache Iceberg", 20),
    ];

    Ok(PhysicalInspectResult {
        file_info,
        schema: None,
        layout: None,
        statistics: None,
        stats_title: None,
    })
}

#[cfg(not(feature = "iceberg"))]
pub async fn inspect_iceberg_layout(
    _path: &Path,
    _storage: Arc<dyn StorageBackend>,
    _options: &PhysicalInspectOptions,
) -> Result<PhysicalInspectResult> {
    Err(crate::error::Error::General(
        "Iceberg support not enabled. Rebuild with --features iceberg".to_string(),
    ))
}
