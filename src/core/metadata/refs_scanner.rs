//! Reference scanner for Iceberg tables
//!
//! Scans all snapshots to find all referenced data files using native scan API.
//! Used for orphan detection and garbage collection.

use std::collections::HashSet;

use futures::TryStreamExt;
use iceberg::table::StaticTable;
use indicatif::{ProgressBar, ProgressStyle};

use crate::error::Result;

/// Scan all snapshots and return set of all referenced data files
///
/// Uses native scan().snapshot_id() API for each snapshot.
/// Files returned by plan_files() are automatically filtered (no deleted files).
pub async fn scan_all_referenced_files(table: &StaticTable) -> Result<HashSet<String>> {
    let metadata = table.metadata();
    let snapshots: Vec<_> = metadata.snapshots().collect();

    let pb = ProgressBar::new(snapshots.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {spinner:.cyan} Scanning snapshots {bar:30.dim.white/dim} {pos}/{len}")
            .expect("hardcoded progress template is valid")
            .progress_chars("━━╺"),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let mut all_files: HashSet<String> = HashSet::new();

    for (i, snapshot) in snapshots.iter().enumerate() {
        let scan = match table.scan()
            .snapshot_id(snapshot.snapshot_id())
            .build()
        {
            Ok(s) => s,
            Err(_) => {
                pb.set_position((i + 1) as u64);
                continue;
            }
        };

        let tasks: Vec<_> = match scan.plan_files().await {
            Ok(stream) => match stream.try_collect().await {
                Ok(t) => t,
                Err(_) => {
                    pb.set_position((i + 1) as u64);
                    continue;
                }
            },
            Err(_) => {
                pb.set_position((i + 1) as u64);
                continue;
            }
        };

        for task in tasks {
            all_files.insert(task.data_file_path().to_string());
        }

        pb.set_position((i + 1) as u64);
    }

    pb.finish_and_clear();

    Ok(all_files)
}
