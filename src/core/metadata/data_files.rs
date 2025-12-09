//! Data file operations for Iceberg tables
//!
//! Functions for reading and listing data files from manifests.

use std::collections::HashSet;

use futures::stream::{self, StreamExt};
use iceberg::io::FileIO;
use iceberg::spec::{ManifestList, ManifestStatus, Snapshot, TableMetadata};

use super::iceberg_partition;
use super::traits::DataFileInfo;
use crate::error::{Error, Result};

/// List data files for a specific snapshot
pub async fn list_data_files_for_snapshot(
    file_io: &FileIO,
    metadata: &TableMetadata,
    snapshot: &Snapshot,
) -> Result<Vec<DataFileInfo>> {
    let manifest_list_path = snapshot.manifest_list();
    let manifest_list_content = file_io
        .new_input(manifest_list_path)
        .map_err(|e| Error::General(format!("Failed to open manifest list: {}", e)))?
        .read()
        .await
        .map_err(|e| Error::General(format!("Failed to read manifest list: {}", e)))?;

    let manifest_list =
        ManifestList::parse_with_version(&manifest_list_content, metadata.format_version())
            .map_err(|e| Error::General(format!("Failed to parse manifest list: {}", e)))?;

    list_data_files_from_manifest_list(file_io, &manifest_list).await
}

/// List data files from a manifest list with deduplication
pub async fn list_data_files_from_manifest_list(
    file_io: &FileIO,
    manifest_list: &ManifestList,
) -> Result<Vec<DataFileInfo>> {
    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut deleted_paths: HashSet<String> = HashSet::new();
    let mut data_files = Vec::new();

    // First pass: collect all deleted paths
    for manifest_file_entry in manifest_list.entries() {
        if manifest_file_entry.content != iceberg::spec::ManifestContentType::Data {
            continue;
        }

        let manifest = manifest_file_entry
            .load_manifest(file_io)
            .await
            .map_err(|e| Error::General(format!("Failed to load manifest: {}", e)))?;

        for entry in manifest.entries() {
            if entry.status() == ManifestStatus::Deleted {
                deleted_paths.insert(entry.data_file().file_path().to_string());
            }
        }
    }

    // Second pass: collect alive files
    for manifest_file_entry in manifest_list.entries() {
        if manifest_file_entry.content != iceberg::spec::ManifestContentType::Data {
            continue;
        }

        let manifest = manifest_file_entry
            .load_manifest(file_io)
            .await
            .map_err(|e| Error::General(format!("Failed to load manifest: {}", e)))?;

        for entry in manifest.entries() {
            if entry.status() == ManifestStatus::Deleted {
                continue;
            }

            let data_file = entry.data_file();
            let path = data_file.file_path().to_string();

            if deleted_paths.contains(&path) || seen_paths.contains(&path) {
                continue;
            }
            seen_paths.insert(path.clone());

            data_files.push(DataFileInfo {
                path,
                size: data_file.file_size_in_bytes(),
                record_count: data_file.record_count(),
                partition: iceberg_partition::extract_partition_from_path_static(
                    data_file.file_path(),
                ),
            });
        }
    }

    Ok(data_files)
}

/// List data files with parallel manifest loading
pub async fn list_data_files_parallel(
    file_io: &FileIO,
    metadata: &TableMetadata,
    snapshot: &Snapshot,
    concurrency: usize,
) -> Result<Vec<DataFileInfo>> {
    let manifest_list_path = snapshot.manifest_list();
    let manifest_list_content = file_io
        .new_input(manifest_list_path)
        .map_err(|e| Error::General(format!("Failed to open manifest list: {}", e)))?
        .read()
        .await
        .map_err(|e| Error::General(format!("Failed to read manifest list: {}", e)))?;

    let manifest_list =
        ManifestList::parse_with_version(&manifest_list_content, metadata.format_version())
            .map_err(|e| Error::General(format!("Failed to parse manifest list: {}", e)))?;

    // Filter to data manifests only
    let data_manifest_entries: Vec<_> = manifest_list
        .entries()
        .iter()
        .filter(|e| e.content == iceberg::spec::ManifestContentType::Data)
        .cloned()
        .collect();

    // Load manifests in parallel
    let manifests: Vec<_> = stream::iter(data_manifest_entries)
        .map(|entry| {
            let file_io = file_io.clone();
            async move { entry.load_manifest(&file_io).await }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;

    // Process manifests and collect files
    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut deleted_paths: HashSet<String> = HashSet::new();
    let mut data_files = Vec::new();

    // First pass: collect deleted paths
    for manifest_result in &manifests {
        let manifest = manifest_result
            .as_ref()
            .map_err(|e| Error::General(format!("Failed to load manifest: {}", e)))?;

        for entry in manifest.entries() {
            if entry.status() == ManifestStatus::Deleted {
                deleted_paths.insert(entry.data_file().file_path().to_string());
            }
        }
    }

    // Second pass: collect alive files
    for manifest_result in manifests {
        let manifest = manifest_result
            .map_err(|e| Error::General(format!("Failed to load manifest: {}", e)))?;

        for entry in manifest.entries() {
            if entry.status() == ManifestStatus::Deleted {
                continue;
            }

            let data_file = entry.data_file();
            let path = data_file.file_path().to_string();

            if deleted_paths.contains(&path) || seen_paths.contains(&path) {
                continue;
            }
            seen_paths.insert(path.clone());

            data_files.push(DataFileInfo {
                path,
                size: data_file.file_size_in_bytes(),
                record_count: data_file.record_count(),
                partition: iceberg_partition::extract_partition_from_path_static(
                    data_file.file_path(),
                ),
            });
        }
    }

    Ok(data_files)
}
