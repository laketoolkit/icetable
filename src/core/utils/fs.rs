//! Filesystem utilities
//!
//! Common filesystem operations for scanning directories and normalizing paths.

use std::path::Path;

/// Normalize a file path by stripping the file:// prefix if present
pub fn normalize_path(path: &str) -> String {
    path.strip_prefix("file://").unwrap_or(path).to_string()
}

/// Information about a scanned file
#[derive(Debug, Clone)]
pub struct ScannedFile {
    /// Full path to the file
    pub path: String,
    /// File size in bytes
    pub size: u64,
    /// Modification time as unix timestamp (seconds)
    pub mtime_seconds: i64,
}

/// Configuration for directory scanning
#[derive(Debug, Clone, Default)]
pub struct ScanConfig {
    /// Directories to skip during scanning
    pub skip_dirs: Vec<String>,
    /// File extension to look for (e.g., "parquet")
    pub extension: String,
    /// Optional cutoff timestamp - only include files older than this
    pub cutoff_timestamp: Option<i64>,
}

impl ScanConfig {
    /// Create a config for scanning parquet files
    pub fn parquet() -> Self {
        Self {
            skip_dirs: vec!["_delta_log".to_string(), "metadata".to_string()],
            extension: "parquet".to_string(),
            cutoff_timestamp: None,
        }
    }

    /// Set cutoff timestamp
    pub fn with_cutoff(mut self, timestamp: i64) -> Self {
        self.cutoff_timestamp = Some(timestamp);
        self
    }
}

/// Scan a directory recursively for files matching the config
///
/// Returns a vector of scanned files. Errors during scanning are logged
/// but do not stop the scan.
pub fn scan_parquet_files(dir: &Path, config: &ScanConfig) -> Vec<ScannedFile> {
    let mut files = Vec::new();
    scan_directory_recursive(dir, config, &mut files);
    files
}

fn scan_directory_recursive(dir: &Path, config: &ScanConfig, files: &mut Vec<ScannedFile>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => {
            // Silently skip directories we can't read (permission issues, etc.)
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip configured directories
        if config.skip_dirs.contains(&name) {
            continue;
        }

        if path.is_dir() {
            scan_directory_recursive(&path, config, files);
        } else if path
            .extension()
            .map_or(false, |ext| ext == config.extension.as_str())
        {
            if let Some(scanned) = scan_single_file(&path, config.cutoff_timestamp) {
                files.push(scanned);
            }
        }
    }
}

fn scan_single_file(path: &Path, cutoff_timestamp: Option<i64>) -> Option<ScannedFile> {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => {
            // Skip files we can't read metadata for
            return None;
        }
    };

    let mtime = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // Apply cutoff filter if specified
    if let Some(cutoff) = cutoff_timestamp {
        if mtime >= cutoff {
            return None;
        }
    }

    Some(ScannedFile {
        path: normalize_path(&path.to_string_lossy()),
        size: metadata.len(),
        mtime_seconds: mtime,
    })
}
