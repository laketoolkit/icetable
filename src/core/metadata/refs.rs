//! Branch and reference management for Iceberg tables
//!
//! Provides functionality for listing and resolving Iceberg table references
//! (branches and tags).
//!
//! Note: iceberg 0.7 provides `snapshot_for_ref()` for looking up individual refs,
//! but does NOT provide a method to list all refs. Therefore, `list_refs()` must
//! parse the raw JSON metadata to enumerate all references.

use iceberg::spec::TableMetadata;

use crate::core::storage::{ObjectStoreExt, Storage};
use crate::error::{Error, Result};

/// Information about a reference (branch or tag)
#[derive(Debug, Clone)]
pub struct RefInfo {
    /// Name of the reference
    pub name: String,
    /// Snapshot ID the reference points to
    pub snapshot_id: i64,
    /// Type of reference: "branch" or "tag"
    pub ref_type: String,
}

/// List all references (branches and tags) from raw metadata JSON
///
/// This function parses the raw JSON because iceberg 0.7 doesn't expose
/// a method to list all refs (only `snapshot_for_ref()` for single lookups).
///
/// Returns a list of RefInfo with name, snapshot_id, and type.
/// Always includes "main" pointing to current snapshot if not already present.
pub async fn list_refs(storage: &Storage, metadata_path: &str) -> Result<Vec<RefInfo>> {
    let content = storage.get_bytes_str(metadata_path).await?;
    let json: serde_json::Value = serde_json::from_slice(&content)
        .map_err(|e| Error::General(format!("Failed to parse metadata JSON: {}", e)))?;

    parse_refs_from_json(&json)
}

/// Parse refs from a JSON metadata value
pub fn parse_refs_from_json(json: &serde_json::Value) -> Result<Vec<RefInfo>> {
    let mut refs = Vec::new();

    if let Some(refs_obj) = json.get("refs").and_then(|v| v.as_object()) {
        for (name, ref_value) in refs_obj {
            let snapshot_id = ref_value
                .get("snapshot-id")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            let ref_type = ref_value
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            refs.push(RefInfo {
                name: name.clone(),
                snapshot_id,
                ref_type,
            });
        }
    }

    // Always include "main" pointing to current snapshot if not already present
    if !refs.iter().any(|r| r.name == "main")
        && let Some(current_id) = json.get("current-snapshot-id").and_then(|v| v.as_i64())
    {
        refs.push(RefInfo {
            name: "main".to_string(),
            snapshot_id: current_id,
            ref_type: "branch".to_string(),
        });
    }

    Ok(refs)
}

/// Resolve branch name to snapshot ID using iceberg's native API
///
/// If branch is None, returns current snapshot ID.
/// If branch is Some, uses `snapshot_for_ref()` to look up the branch.
pub fn resolve_branch_snapshot_id(metadata: &TableMetadata, branch: Option<&str>) -> Result<i64> {
    match branch {
        Some(branch_name) => metadata
            .snapshot_for_ref(branch_name)
            .map(|snap_ref| snap_ref.snapshot_id())
            .ok_or_else(|| Error::General(format!("Branch '{}' not found", branch_name))),
        None => metadata
            .current_snapshot_id()
            .ok_or_else(|| Error::General("No current snapshot".to_string())),
    }
}

/// Get the branch name to use (defaults to "main" if None)
pub fn branch_name_or_default(branch: Option<&str>) -> &str {
    branch.unwrap_or("main")
}
