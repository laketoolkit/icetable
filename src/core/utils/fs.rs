//! Filesystem utilities
//!
//! Common filesystem operations for scanning directories and normalizing paths.

use std::path::Path;

use crate::error::{Error, Result};

/// Normalize a file path for consistent comparison
///
/// This function ensures paths are consistently formatted:
/// - Strips `file://` prefix if present
/// - Removes trailing slashes (except for root "/")
///
/// Use this function when comparing paths from different sources
/// (e.g., metadata vs filesystem) to avoid false mismatches.
pub fn normalize_path(path: &str) -> String {
    let path = path.strip_prefix("file://").unwrap_or(path);
    // Remove trailing slash unless it's the root path
    let path = path.strip_suffix('/').unwrap_or(path);
    if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    }
}

/// Normalize both paths and compute the relative path from base to full
///
/// Returns the relative portion of `full_path` after removing `base_path`.
/// Handles `file://` prefixes transparently.
///
/// # Examples
/// ```ignore
/// normalize_relative_path("/data/table/file.parquet", "/data/table")
/// // Returns: "file.parquet"
///
/// normalize_relative_path("file:///data/table/file.parquet", "/data/table")
/// // Returns: "file.parquet"
/// ```
pub fn normalize_relative_path(full_path: &str, base_path: &str) -> Option<String> {
    let normalized_full = normalize_path(full_path);
    let normalized_base = normalize_path(base_path);

    // Try to strip the base path
    if let Some(relative) = normalized_full.strip_prefix(&normalized_base) {
        let relative = relative.trim_start_matches('/');
        if relative.is_empty() {
            None
        } else {
            Some(relative.to_string())
        }
    } else {
        // Paths don't share the same base
        None
    }
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
/// Returns an error if any directory or file cannot be read.
/// This ensures operations like vacuum don't make decisions based on incomplete data.
pub fn scan_parquet_files(dir: &Path, config: &ScanConfig) -> Result<Vec<ScannedFile>> {
    let mut files = Vec::new();
    scan_directory_recursive(dir, config, &mut files)?;
    Ok(files)
}

fn scan_directory_recursive(
    dir: &Path,
    config: &ScanConfig,
    files: &mut Vec<ScannedFile>,
) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| Error::Storage {
        message: format!("Failed to read directory '{}': {}", dir.display(), e),
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| Error::Storage {
            message: format!("Failed to read entry in '{}': {}", dir.display(), e),
        })?;

        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip configured directories
        if config.skip_dirs.contains(&name) {
            continue;
        }

        if path.is_dir() {
            scan_directory_recursive(&path, config, files)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext == config.extension.as_str())
        {
            let scanned = scan_single_file(&path, config.cutoff_timestamp)?;
            if let Some(file) = scanned {
                files.push(file);
            }
        }
    }

    Ok(())
}

fn scan_single_file(path: &Path, cutoff_timestamp: Option<i64>) -> Result<Option<ScannedFile>> {
    let metadata = std::fs::metadata(path).map_err(|e| Error::Storage {
        message: format!("Failed to read metadata for '{}': {}", path.display(), e),
    })?;

    let mtime = metadata
        .modified()
        .map_err(|e| Error::Storage {
            message: format!(
                "Failed to get modification time for '{}': {}",
                path.display(),
                e
            ),
        })?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // Apply cutoff filter if specified
    if let Some(cutoff) = cutoff_timestamp
        && mtime >= cutoff
    {
        return Ok(None);
    }

    Ok(Some(ScannedFile {
        path: normalize_path(&path.to_string_lossy()),
        size: metadata.len(),
        mtime_seconds: mtime,
    }))
}
