//! Reference scanner for Iceberg tables
//!
//! Scans all snapshots to find all referenced data files using native scan API.
//! Used for orphan detection and garbage collection.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::{StreamExt, TryStreamExt};
use iceberg::table::StaticTable;

use crate::error::Result;
use crate::utils::create_progress_bar;

/// Scan all snapshots and return set of all referenced data files
///
/// Uses native scan().snapshot_id() API for each snapshot.
/// Files returned by plan_files() are automatically filtered (no deleted files).
/// Snapshots are processed in parallel (up to 8 concurrent) for better performance.
pub async fn scan_all_referenced_files(table: &StaticTable) -> Result<HashSet<String>> {
    let metadata = table.metadata();
    // Collect snapshot IDs upfront to avoid lifetime issues with async closures
    let snapshot_ids: Vec<i64> = metadata.snapshots().map(|s| s.snapshot_id()).collect();

    let pb = create_progress_bar(snapshot_ids.len() as u64, "Scanning snapshots");

    // Counter for progress updates
    let progress_counter = Arc::new(AtomicU64::new(0));

    // Process snapshots in parallel (up to 8 concurrent)
    let results: Vec<Vec<String>> = futures::stream::iter(snapshot_ids)
        .map(|snapshot_id| {
            let counter = Arc::clone(&progress_counter);
            let pb = pb.clone();
            async move {
                let result = scan_snapshot_files(table, snapshot_id).await;
                let pos = counter.fetch_add(1, Ordering::Relaxed) + 1;
                pb.set_position(pos);
                result
            }
        })
        .buffer_unordered(8)
        .collect()
        .await;

    pb.finish_and_clear();

    // Consolidate results
    let mut all_files = HashSet::new();
    for files in results {
        all_files.extend(files);
    }

    Ok(all_files)
}

/// Scan a single snapshot and return its data file paths
async fn scan_snapshot_files(table: &StaticTable, snapshot_id: i64) -> Vec<String> {
    let scan = match table.scan().snapshot_id(snapshot_id).build() {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let tasks: Vec<_> = match scan.plan_files().await {
        Ok(stream) => match stream.try_collect().await {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        },
        Err(_) => return Vec::new(),
    };

    tasks
        .iter()
        .map(|task| task.data_file_path().to_string())
        .collect()
}
