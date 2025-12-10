//! Table integrity checks
//!
//! Checks for metadata, snapshots, manifests, and data files.

use std::collections::HashSet;

use futures::StreamExt;

use crate::core::metadata::IcebergMetadataService;
use crate::core::storage::{to_path, ObjectStoreExt, Storage};

use super::CheckResult;

/// Check metadata format (standard Iceberg naming)
pub async fn check_metadata_format(
    storage: &Storage,
    table_path: &str,
) -> (CheckResult, Option<i32>) {
    use crate::utils::core::{extract_version_from_path, find_latest_metadata};

    match find_latest_metadata(table_path, storage).await {
        Ok(metadata_path) => {
            let filename = metadata_path
                .split('/')
                .next_back()
                .unwrap_or(&metadata_path);
            let version = extract_version_from_path(&metadata_path);

            if let Some(v) = version {
                (
                    CheckResult::ok("Metadata Format", format!("v{} ({})", v, filename)),
                    Some(v),
                )
            } else {
                (
                    CheckResult::error(
                        "Metadata Format",
                        format!("Invalid format: {}", filename),
                        "Expected standard Iceberg format: <version>-<uuid>.metadata.json",
                    ),
                    None,
                )
            }
        }
        Err(e) => (
            CheckResult::error(
                "Metadata Format",
                format!("Cannot find metadata: {}", e),
                "No valid metadata.json files found in metadata/ directory",
            ),
            None,
        ),
    }
}

/// Check metadata JSON is valid
pub async fn check_metadata_json(
    storage: &Storage,
    table_path: &str,
) -> (CheckResult, Option<serde_json::Value>) {
    use crate::utils::core::find_latest_metadata;

    let metadata_path = match find_latest_metadata(table_path, storage).await {
        Ok(path) => path,
        Err(e) => {
            return (
                CheckResult::error(
                    "Metadata JSON",
                    format!("Cannot find metadata: {}", e),
                    "Table metadata/ directory is empty or corrupted",
                ),
                None,
            );
        }
    };

    match storage.get_bytes_str(&metadata_path).await {
        Ok(bytes) => {
            let content = String::from_utf8_lossy(&bytes);
            match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(meta) => {
                    let has_format_version = meta.get("format-version").is_some();
                    let has_table_uuid = meta.get("table-uuid").is_some();
                    let has_location = meta.get("location").is_some();

                    if has_format_version && has_table_uuid && has_location {
                        let format_version = meta
                            .get("format-version")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let filename = metadata_path
                            .split('/')
                            .next_back()
                            .unwrap_or(&metadata_path);
                        (
                            CheckResult::ok(
                                "Metadata JSON",
                                format!("{} (format v{})", filename, format_version),
                            ),
                            Some(meta),
                        )
                    } else {
                        let missing: Vec<&str> = [
                            (!has_format_version, "format-version"),
                            (!has_table_uuid, "table-uuid"),
                            (!has_location, "location"),
                        ]
                        .iter()
                        .filter(|(missing, _)| *missing)
                        .map(|(_, name)| *name)
                        .collect();

                        (
                            CheckResult::error(
                                "Metadata JSON",
                                format!("Missing fields: {}", missing.join(", ")),
                                "Metadata file is incomplete or corrupted",
                            ),
                            None,
                        )
                    }
                }
                Err(e) => (
                    CheckResult::error(
                        "Metadata JSON",
                        format!("Parse error: {}", e),
                        "Metadata file contains invalid JSON",
                    ),
                    None,
                ),
            }
        }
        Err(e) => (
            CheckResult::error(
                "Metadata JSON",
                format!("Cannot read: {}", e),
                "Metadata file is missing or inaccessible",
            ),
            None,
        ),
    }
}

/// Check snapshot graph for cycles and orphan references
pub fn check_snapshot_graph(metadata: &serde_json::Value) -> CheckResult {
    let snapshots = match metadata.get("snapshots").and_then(|s| s.as_array()) {
        Some(s) if !s.is_empty() => s,
        _ => return CheckResult::ok("Snapshot Graph", "No snapshots (empty table)"),
    };

    let snapshot_ids: HashSet<i64> = snapshots
        .iter()
        .filter_map(|s| s.get("snapshot-id").and_then(|id| id.as_i64()))
        .collect();

    let mut orphan_count = 0;
    for snapshot in snapshots {
        if let Some(parent_id) = snapshot
            .get("parent-snapshot-id")
            .and_then(|id| id.as_i64())
            && parent_id > 0
            && !snapshot_ids.contains(&parent_id)
        {
            orphan_count += 1;
        }
    }

    if orphan_count > 0 {
        CheckResult::warning(
            "Snapshot Graph",
            format!(
                "{} snapshots, {} orphan references",
                snapshots.len(),
                orphan_count
            ),
            "Some snapshots reference expired parents (normal after expire)",
        )
    } else {
        CheckResult::ok(
            "Snapshot Graph",
            format!("{} snapshots, no cycles", snapshots.len()),
        )
    }
}

/// Check current snapshot reference is valid
pub fn check_current_snapshot(metadata: &serde_json::Value) -> CheckResult {
    let current_id = metadata
        .get("current-snapshot-id")
        .and_then(|id| id.as_i64());

    match current_id {
        Some(-1) | None => {
            CheckResult::ok("Current Snapshot", "No current snapshot (empty table)")
        }
        Some(id) => {
            let snapshots = metadata
                .get("snapshots")
                .and_then(|s| s.as_array())
                .map(|arr| arr.iter().collect::<Vec<_>>())
                .unwrap_or_default();

            let exists = snapshots
                .iter()
                .any(|s| s.get("snapshot-id").and_then(|sid| sid.as_i64()) == Some(id));

            if exists {
                CheckResult::ok("Current Snapshot", format!("ID {} exists", id))
            } else {
                CheckResult::error(
                    "Current Snapshot",
                    format!("ID {} not found in snapshots", id),
                    "Current snapshot reference is invalid. Table may be corrupted.",
                )
            }
        }
    }
}

/// Check that manifest files exist (parallel)
pub async fn check_manifests_exist(
    storage: &Storage,
    service: &IcebergMetadataService,
) -> CheckResult {
    let table = service.table();
    let metadata = table.metadata();

    let current_snapshot = match metadata.current_snapshot() {
        Some(s) => s,
        None => return CheckResult::ok("Manifest Files", "No manifests (empty table)"),
    };

    let file_io = service.file_io();
    let manifest_list = match current_snapshot.load_manifest_list(file_io, &metadata).await {
        Ok(ml) => ml,
        Err(e) => {
            return CheckResult::error(
                "Manifest Files",
                format!("Cannot load manifest list: {}", e),
                "Manifest list file is corrupted or missing",
            );
        }
    };

    let manifest_paths: Vec<String> = manifest_list
        .entries()
        .iter()
        .map(|entry| entry.manifest_path.clone())
        .collect();

    // Check manifest files in parallel (up to 32 concurrent checks)
    let missing = futures::stream::iter(manifest_paths.iter())
        .map(|path| {
            let storage = storage.clone();
            let path = path.clone();
            async move {
                let exists = storage.exists(&to_path(&path)).await.unwrap_or(false);
                if exists { 0usize } else { 1usize }
            }
        })
        .buffer_unordered(32)
        .fold(0usize, |acc, x| async move { acc + x })
        .await;

    if missing > 0 {
        CheckResult::error(
            "Manifest Files",
            format!("{}/{} manifests missing", missing, manifest_paths.len()),
            "Some manifest files are missing. Table may be corrupted.",
        )
    } else {
        CheckResult::ok(
            "Manifest Files",
            format!("{} manifests verified", manifest_paths.len()),
        )
    }
}

/// Check that data files exist (parallel)
pub async fn check_data_files_exist(
    storage: &Storage,
    service: &IcebergMetadataService,
) -> CheckResult {
    use futures::TryStreamExt;

    let table = service.table();
    let metadata = table.metadata();

    if metadata.current_snapshot().is_none() {
        return CheckResult::ok("Data Files", "No data files (empty table)");
    }

    let scan = match table.scan().build() {
        Ok(s) => s,
        Err(e) => {
            return CheckResult::error(
                "Data Files",
                format!("Cannot build scan: {}", e),
                "Table scan failed",
            );
        }
    };

    let tasks: Vec<_> = match scan.plan_files().await {
        Ok(stream) => match stream.try_collect().await {
            Ok(t) => t,
            Err(e) => {
                return CheckResult::error(
                    "Data Files",
                    format!("Cannot plan files: {}", e),
                    "File planning failed",
                );
            }
        },
        Err(e) => {
            return CheckResult::error(
                "Data Files",
                format!("Cannot scan table: {}", e),
                "Table scan failed",
            );
        }
    };

    let data_files: Vec<String> = tasks
        .iter()
        .map(|task| task.data_file_path().to_string())
        .collect();

    if data_files.is_empty() {
        return CheckResult::ok("Data Files", "No data files in current snapshot");
    }

    let total_files = data_files.len();

    // Check data files in parallel (up to 64 concurrent checks for larger file sets)
    let missing = futures::stream::iter(data_files.iter())
        .map(|file_path| {
            let storage = storage.clone();
            let file_path = file_path.clone();
            async move {
                let exists = storage.exists(&to_path(&file_path)).await.unwrap_or(false);
                if exists { 0usize } else { 1usize }
            }
        })
        .buffer_unordered(64)
        .fold(0usize, |acc, x| async move { acc + x })
        .await;

    if missing > 0 {
        CheckResult::error(
            "Data Files",
            format!("{}/{} files missing", missing, total_files),
            "Some data files are missing. Data may have been deleted externally.",
        )
    } else {
        CheckResult::ok("Data Files", format!("{} files verified", total_files))
    }
}
