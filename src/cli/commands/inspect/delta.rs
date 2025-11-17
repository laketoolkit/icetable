//! Delta Lake inspection

use std::path::Path;
use std::sync::Arc;

use crate::core::storage::StorageBackend;
use crate::error::Result;

use super::common::*;

/// Inspect Delta Lake table
#[cfg(feature = "delta")]
pub async fn inspect_delta_layout(
    path: &Path,
    _storage: Arc<dyn StorageBackend>,
    _options: &PhysicalInspectOptions,
) -> Result<PhysicalInspectResult> {
    // Simplified Delta Lake inspection
    let file_info = vec![
        kv_item("Path", path.display().to_string(), 20),
        kv_item("Format", "Delta Lake", 20),
        text_item("Note: Full Delta Lake inspection requires Delta Lake APIs"),
    ];

    Ok(PhysicalInspectResult {
        file_info,
        schema: None,
        layout: None,
        statistics: None,
        stats_title: None,
    })
}

#[cfg(not(feature = "delta"))]
pub async fn inspect_delta_layout(
    _path: &Path,
    _storage: Arc<dyn StorageBackend>,
    _options: &PhysicalInspectOptions,
) -> Result<PhysicalInspectResult> {
    Err(crate::error::Error::General(
        "Delta Lake support not enabled. Rebuild with --features delta".to_string(),
    ))
}
