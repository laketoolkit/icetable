//! Reference scanner for Iceberg tables
//!
//! Scans all manifests across all snapshots to find all referenced data files.
//! Used for orphan detection and garbage collection.

use std::collections::HashSet;

use futures::stream::{self, StreamExt};
use iceberg::io::FileIO;
use iceberg::spec::{ManifestContentType, ManifestFile, ManifestList, ManifestStatus, TableMetadata};
use indicatif::{ProgressBar, ProgressStyle};

use crate::error::{Error, Result};

/// Concurrency level for parallel manifest loading
const MANIFEST_CONCURRENCY: usize = 10;

/// Scan all snapshots and return set of all referenced data files
///
/// This function walks through ALL snapshots (not just current) to find every
/// data file that's referenced by any manifest. Used for orphan detection.
///
/// Files that appear as Added or Existing in ANY manifest are considered referenced.
/// Files marked as Deleted are excluded (they're no longer part of the table).
pub async fn scan_all_referenced_files(
    file_io: &FileIO,
    metadata: &TableMetadata,
) -> Result<HashSet<String>> {
    // Collect unique manifest entries from ALL snapshots (deduplicated by path)
    let manifest_entries = collect_manifest_entries(file_io, metadata).await?;

    // Scan all manifests for referenced files
    scan_manifests_for_files(file_io, manifest_entries).await
}

/// Collect all unique manifest entries from all snapshots
async fn collect_manifest_entries(
    file_io: &FileIO,
    metadata: &TableMetadata,
) -> Result<Vec<ManifestFile>> {
    let mut seen_manifest_paths: HashSet<String> = HashSet::new();
    let mut manifest_entries: Vec<ManifestFile> = Vec::new();

    for snapshot in metadata.snapshots() {
        let manifest_list_path = snapshot.manifest_list();

        let manifest_list_content = match file_io
            .new_input(manifest_list_path)
            .map_err(|e| Error::General(format!("Failed to open manifest list: {}", e)))?
            .read()
            .await
        {
            Ok(content) => content,
            Err(_) => continue,
        };

        let manifest_list = match ManifestList::parse_with_version(
            &manifest_list_content,
            metadata.format_version(),
        ) {
            Ok(ml) => ml,
            Err(_) => continue,
        };

        for entry in manifest_list.entries() {
            if entry.content == ManifestContentType::Data
                && !seen_manifest_paths.contains(&entry.manifest_path)
            {
                seen_manifest_paths.insert(entry.manifest_path.clone());
                manifest_entries.push(entry.clone());
            }
        }
    }

    Ok(manifest_entries)
}

/// Scan manifests in parallel to extract all referenced file paths
async fn scan_manifests_for_files(
    file_io: &FileIO,
    manifest_entries: Vec<ManifestFile>,
) -> Result<HashSet<String>> {
    let total_manifests = manifest_entries.len();

    let pb = ProgressBar::new(total_manifests as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {spinner:.cyan} Scanning manifests {bar:30.dim.white/dim} {pos}/{len}")
            .expect("hardcoded progress template is valid")
            .progress_chars("━━╺"),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let mut all_alive: HashSet<String> = HashSet::new();
    let mut processed = 0usize;

    for chunk in manifest_entries.chunks(MANIFEST_CONCURRENCY) {
        let chunk_owned: Vec<_> = chunk.to_vec();
        let file_io = file_io.clone();

        let results: Vec<_> = stream::iter(chunk_owned)
            .map(|entry| {
                let file_io = file_io.clone();
                async move { entry.load_manifest(&file_io).await.ok() }
            })
            .buffer_unordered(MANIFEST_CONCURRENCY)
            .collect()
            .await;

        for manifest_opt in results {
            processed += 1;
            let Some(manifest) = manifest_opt else {
                continue;
            };

            for entry in manifest.entries() {
                // For orphan detection: if a file appears as Added/Existing in ANY manifest,
                // it's referenced and not an orphan
                if entry.status() != ManifestStatus::Deleted {
                    all_alive.insert(entry.data_file().file_path().to_string());
                }
            }
            // Manifest is dropped here, freeing memory
        }
        pb.set_position(processed as u64);
    }

    pb.finish_and_clear();

    Ok(all_alive)
}
