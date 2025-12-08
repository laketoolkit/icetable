//! Orphan file detection for Iceberg tables

use apache_avro::Reader;
use futures::stream::{self, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashSet;
use std::sync::Arc;

use super::manifest::normalize_path;
use crate::core::inspection::traits::{OrphanFileEntry, OrphanFilesInfo};
use crate::core::metadata::{IcebergMetadataService, MetadataService};
use crate::core::storage::{Storage, ObjectStoreExt};
use crate::error::Result;

/// Detect orphan files - files in data/ not tracked in metadata
///
/// - `deep_scan=true`: Check ALL snapshots (slow but accurate)
/// - `deep_scan=false`: Check only current snapshot (fast but may have false positives)
pub async fn detect_orphan_files(
    table_path: &str,
    storage: &Storage,
    metadata: &serde_json::Value,
    deep_scan: bool,
) -> Result<OrphanFilesInfo> {
    let metadata_service = IcebergMetadataService::new_async(table_path.to_string()).await?;

    // Get referenced files based on scan mode
    let reference_set: HashSet<String> = if deep_scan {
        // Deep scan: check ALL snapshots
        let all_referenced = metadata_service.get_all_referenced_files().await?;
        let mut set = HashSet::new();
        for path in &all_referenced {
            set.insert(path.clone());
            if let Some(filename) = path.rsplit('/').next() {
                set.insert(filename.to_string());
            }
        }
        set
    } else {
        // Quick scan: only current snapshot
        let table_location = metadata
            .get("location")
            .and_then(|l| l.as_str())
            .unwrap_or(table_path);
        get_tracked_files(storage, metadata, table_location).await?
    };

    // Scan storage for parquet files
    let storage_files = metadata_service.scan_data_files_on_storage().await?;

    // Find orphan files
    let mut orphans: Vec<OrphanFileEntry> = Vec::new();
    let mut total_size: u64 = 0;
    let mut total_orphan_count: usize = 0;

    for file in &storage_files {
        let is_referenced = reference_set
            .iter()
            .any(|referenced| file.path.ends_with(referenced) || file.path == *referenced);

        if !is_referenced {
            total_orphan_count += 1;
            total_size += file.size;

            if orphans.len() < 10 {
                orphans.push(OrphanFileEntry {
                    path: file.path.clone(),
                    size: file.size,
                });
            }
        }
    }

    Ok(OrphanFilesInfo {
        count: total_orphan_count,
        total_size,
        files: orphans,
        truncated: total_orphan_count > 10,
        is_deep_scan: deep_scan,
    })
}

/// Get all file paths tracked in current snapshot manifests (parallelized)
///
/// Note: For accurate orphan detection across ALL snapshots, use
/// IcebergMetadataService::get_all_referenced_files() instead.
/// This method is used for quick scans (current snapshot only).
pub async fn get_tracked_files(
    storage: &Storage,
    metadata: &serde_json::Value,
    table_location: &str,
) -> Result<HashSet<String>> {
    // Get current snapshot
    let current_snapshot_id = metadata
        .get("current-snapshot-id")
        .and_then(|id| id.as_i64());

    let snapshots = metadata
        .get("snapshots")
        .and_then(|s| s.as_array())
        .map(|arr| arr.as_slice())
        .unwrap_or(&[]);

    // Find current snapshot
    let current_snapshot = snapshots
        .iter()
        .find(|s| s.get("snapshot-id").and_then(|id| id.as_i64()) == current_snapshot_id);

    let Some(snapshot) = current_snapshot else {
        return Ok(HashSet::new());
    };

    let Some(manifest_list) = snapshot.get("manifest-list").and_then(|m| m.as_str()) else {
        return Ok(HashSet::new());
    };

    let manifest_list_path = normalize_path(manifest_list, table_location);

    // Read manifest list
    let manifest_list_bytes = match storage.get_bytes_str(&manifest_list_path).await {
        Ok(bytes) => bytes,
        Err(_) => return Ok(HashSet::new()),
    };

    let manifest_list_reader = match Reader::new(&manifest_list_bytes[..]) {
        Ok(reader) => reader,
        Err(_) => return Ok(HashSet::new()),
    };

    // Collect all manifest paths first
    let mut manifest_paths: Vec<String> = Vec::new();
    for value_result in manifest_list_reader {
        if let Ok(apache_avro::types::Value::Record(fields)) = value_result {
            let manifest_path = fields
                .iter()
                .find(|(name, _)| name == "manifest-path" || name == "manifest_path")
                .and_then(|(_, v)| {
                    if let apache_avro::types::Value::String(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                });

            if let Some(path) = manifest_path {
                manifest_paths.push(normalize_path(&path, table_location));
            }
        }
    }

    // Read manifests in parallel with concurrency limit
    let total_manifests = manifest_paths.len();
    let pb = ProgressBar::new(total_manifests as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.cyan} Scanning manifests {bar:30.dim.white/dim} {pos}/{len}")
            .expect("hardcoded progress template is valid")
            .progress_chars("━━╺"),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let results: Vec<HashSet<String>> = stream::iter(manifest_paths.into_iter())
        .map(|path| {
            let storage = Arc::clone(storage);
            let pb = pb.clone();
            async move {
                let result = if let Ok(bytes) = storage.get_bytes_str(&path).await {
                    if let Ok(reader) = Reader::new(&bytes[..]) {
                        extract_file_paths(reader)
                    } else {
                        HashSet::new()
                    }
                } else {
                    HashSet::new()
                };
                pb.inc(1);
                result
            }
        })
        .buffer_unordered(32) // Process up to 32 manifests concurrently
        .collect()
        .await;

    pb.finish_and_clear();

    // Merge all results
    let mut tracked_files: HashSet<String> = HashSet::new();
    for result in results {
        tracked_files.extend(result);
    }

    Ok(tracked_files)
}

/// Extract file paths from a manifest
fn extract_file_paths(manifest_reader: Reader<&[u8]>) -> HashSet<String> {
    let mut files = HashSet::new();
    for data_file_result in manifest_reader {
        if let Ok(apache_avro::types::Value::Record(data_fields)) = data_file_result {
            let data_file_record = data_fields
                .iter()
                .find(|(name, _)| name == "data_file" || name == "data-file")
                .and_then(|(_, v)| {
                    if let apache_avro::types::Value::Record(fields) = v {
                        Some(fields)
                    } else {
                        None
                    }
                });

            let fields_to_process = data_file_record.unwrap_or(&data_fields);

            if let Some((_, file_path_value)) = fields_to_process
                .iter()
                .find(|(name, _)| name == "file-path" || name == "file_path")
                && let apache_avro::types::Value::String(path) = file_path_value
            {
                let filename = path.split('/').next_back().unwrap_or(path);
                files.insert(filename.to_string());
                files.insert(path.clone());
            }
        }
    }
    files
}
