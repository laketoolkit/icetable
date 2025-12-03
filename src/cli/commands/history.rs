//! History command implementation
//!
//! Shows version history for Delta Lake and Iceberg tables.

use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use colored::Colorize;

use crate::cli::parser::HistoryArgs;
use crate::core::storage::{StorageBackend, StorageBackendFactory};
use crate::error::{Error, Result};

/// A single version/snapshot entry in history
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// Version number (Delta) or snapshot ID (Iceberg)
    pub version: i64,
    /// Timestamp of the version
    pub timestamp: DateTime<Utc>,
    /// Operation type (e.g., "WRITE", "CREATE", "APPEND")
    pub operation: String,
    /// Additional details about the operation
    pub details: std::collections::HashMap<String, String>,
}

/// Detected table format
#[derive(Debug, Clone, Copy, PartialEq)]
enum TableFormat {
    Delta,
    Iceberg,
}

/// Handler for history command
pub struct HistoryCommand;

impl HistoryCommand {
    /// Execute history command
    pub async fn execute(args: HistoryArgs) -> Result<()> {
        // Create storage backend (supports local and cloud)
        let storage = StorageBackendFactory::create_backend(&args.path).await?;

        // Detect or use specified format
        let format = if let Some(ref fmt) = args.format {
            match fmt.to_lowercase().as_str() {
                "delta" => TableFormat::Delta,
                "iceberg" => TableFormat::Iceberg,
                _ => {
                    return Err(Error::General(format!(
                        "Unknown format '{}'. Supported: delta, iceberg",
                        fmt
                    )))
                }
            }
        } else {
            Self::detect_format(&args.path, &storage).await?
        };

        let entries = match format {
            TableFormat::Delta => Self::get_delta_history(&args.path, &storage, &args).await?,
            TableFormat::Iceberg => Self::get_iceberg_history(&args.path, &storage, &args).await?,
        };

        // Output
        match args.output.as_str() {
            "json" => Self::output_json(&entries)?,
            _ => Self::output_table(&entries, format == TableFormat::Delta)?,
        }

        Ok(())
    }

    /// Detect table format using storage backend
    async fn detect_format(
        path: &str,
        storage: &Arc<dyn StorageBackend>,
    ) -> Result<TableFormat> {
        // Check for Delta Lake (_delta_log directory)
        if Self::check_delta_exists(path, storage).await {
            return Ok(TableFormat::Delta);
        }

        // Check for Iceberg (metadata directory with .metadata.json files)
        if Self::check_iceberg_exists(path, storage).await {
            return Ok(TableFormat::Iceberg);
        }

        Err(Error::General(format!(
            "Path '{}' is not a Delta Lake or Iceberg table. Use --format to specify explicitly.",
            path
        )))
    }

    /// Check if Delta Lake table exists using storage backend
    async fn check_delta_exists(path: &str, storage: &Arc<dyn StorageBackend>) -> bool {
        use crate::core::storage::traits::ListOptions;

        // Use full path with scheme - storage backend handles parsing
        let path = path.trim_end_matches('/');
        let delta_log_prefix = format!("{}/_delta_log/", path);

        let list_opts = ListOptions {
            prefix: Some(delta_log_prefix),
            delimiter: None,
            max_results: Some(1),
            continuation_token: None,
        };

        matches!(storage.list(&list_opts).await, Ok(result) if !result.objects.is_empty())
    }

    /// Check if Iceberg table exists using storage backend
    async fn check_iceberg_exists(path: &str, storage: &Arc<dyn StorageBackend>) -> bool {
        use crate::core::storage::traits::ListOptions;

        // Use full path with scheme - storage backend handles parsing
        let path = path.trim_end_matches('/');
        let metadata_prefix = format!("{}/metadata/", path);

        let list_opts = ListOptions {
            prefix: Some(metadata_prefix),
            delimiter: None,
            max_results: Some(5),
            continuation_token: None,
        };

        match storage.list(&list_opts).await {
            Ok(result) => result
                .objects
                .iter()
                .any(|obj| obj.path.contains(".metadata.json")),
            Err(_) => false,
        }
    }

    /// Get history from Delta Lake table
    #[cfg(feature = "delta")]
    async fn get_delta_history(
        path: &str,
        _storage: &Arc<dyn StorageBackend>,
        args: &HistoryArgs,
    ) -> Result<Vec<HistoryEntry>> {
        use deltalake::DeltaTableBuilder;

        let table = DeltaTableBuilder::from_uri(path)
            .load()
            .await
            .map_err(|e| Error::General(format!("Failed to open Delta table: {}", e)))?;

        let mut entries = Vec::new();

        // Get current version
        let current_version = table.version().unwrap_or(0);

        let limit = if args.all {
            current_version as usize + 1
        } else {
            args.limit
        };

        // Read commit info from _delta_log using storage
        // Note: deltalake handles cloud storage internally
        let log_store = table.log_store();

        for version in (0..=current_version).rev().take(limit) {
            if let Ok(commit) = log_store.read_commit_entry(version).await {
                if let Some(bytes) = commit {
                    let content = String::from_utf8_lossy(&bytes);
                    if let Ok(entry) = Self::parse_delta_commit(version, &content) {
                        entries.push(entry);
                    }
                }
            }
        }

        Ok(entries)
    }

    #[cfg(not(feature = "delta"))]
    async fn get_delta_history(
        _path: &str,
        _storage: &Arc<dyn StorageBackend>,
        _args: &HistoryArgs,
    ) -> Result<Vec<HistoryEntry>> {
        Err(Error::UnsupportedFeature {
            feature: "Delta Lake support not enabled".to_string(),
        })
    }

    /// Parse a Delta commit JSON file
    #[cfg(feature = "delta")]
    fn parse_delta_commit(version: i64, content: &str) -> Result<HistoryEntry> {
        let mut timestamp = Utc::now();
        let mut operation = "UNKNOWN".to_string();
        let mut details = std::collections::HashMap::new();

        // Parse each line (Delta log is newline-delimited JSON)
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }

            if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                // Look for commitInfo
                if let Some(commit_info) = json.get("commitInfo") {
                    if let Some(ts) = commit_info.get("timestamp").and_then(|v| v.as_i64()) {
                        timestamp = Utc
                            .timestamp_millis_opt(ts)
                            .single()
                            .unwrap_or_else(Utc::now);
                    }
                    if let Some(op) = commit_info.get("operation").and_then(|v| v.as_str()) {
                        operation = op.to_string();
                    }
                    if let Some(metrics) =
                        commit_info.get("operationMetrics").and_then(|v| v.as_object())
                    {
                        for (k, v) in metrics {
                            if let Some(s) = v.as_str() {
                                details.insert(k.clone(), s.to_string());
                            } else {
                                details.insert(k.clone(), v.to_string());
                            }
                        }
                    }
                }

                // Look for metaData (table creation)
                if json.get("metaData").is_some() && operation == "UNKNOWN" {
                    operation = "CREATE TABLE".to_string();
                }

                // Look for add actions
                if let Some(add) = json.get("add") {
                    if operation == "UNKNOWN" {
                        operation = "ADD".to_string();
                    }
                    if let Some(size) = add.get("size").and_then(|v| v.as_i64()) {
                        let current: i64 = details
                            .get("bytesAdded")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                        details.insert("bytesAdded".to_string(), (current + size).to_string());
                    }
                }
            }
        }

        Ok(HistoryEntry {
            version,
            timestamp,
            operation,
            details,
        })
    }

    /// Get history from Iceberg table
    #[cfg(feature = "iceberg")]
    async fn get_iceberg_history(
        path: &str,
        storage: &Arc<dyn StorageBackend>,
        args: &HistoryArgs,
    ) -> Result<Vec<HistoryEntry>> {
        use iceberg::table::StaticTable;
        use iceberg::TableIdent;

        // Find metadata file using storage backend
        let metadata_location = Self::find_iceberg_metadata(path, storage).await?;

        // Create FileIO based on storage type
        let file_io = Self::create_file_io(path)?;

        let table_ident = TableIdent::from_strs(&["iceberg", "table"])
            .map_err(|e| Error::General(format!("Failed to create table identifier: {}", e)))?;

        let table = StaticTable::from_metadata_file(&metadata_location, table_ident, file_io)
            .await
            .map_err(|e| Error::General(format!("Failed to load Iceberg table: {}", e)))?;

        let metadata = table.metadata();
        let mut entries = Vec::new();

        // Get snapshots
        let limit = if args.all { usize::MAX } else { args.limit };

        for snapshot in metadata.snapshots().take(limit) {
            let mut details = std::collections::HashMap::new();

            // Add summary info
            let summary = snapshot.summary();
            details.insert(
                "operation".to_string(),
                format!("{:?}", summary.operation),
            );

            for (k, v) in &summary.additional_properties {
                details.insert(k.clone(), v.clone());
            }

            let timestamp = Utc
                .timestamp_millis_opt(snapshot.timestamp_ms())
                .single()
                .unwrap_or_else(Utc::now);

            entries.push(HistoryEntry {
                version: snapshot.snapshot_id(),
                timestamp,
                operation: format!("{:?}", summary.operation),
                details,
            });
        }

        // Sort by timestamp descending (newest first)
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        Ok(entries)
    }

    #[cfg(not(feature = "iceberg"))]
    async fn get_iceberg_history(
        _path: &str,
        _storage: &Arc<dyn StorageBackend>,
        _args: &HistoryArgs,
    ) -> Result<Vec<HistoryEntry>> {
        Err(Error::UnsupportedFeature {
            feature: "Iceberg support not enabled".to_string(),
        })
    }

    /// Create FileIO based on path scheme
    #[cfg(feature = "iceberg")]
    fn create_file_io(path: &str) -> Result<iceberg::io::FileIO> {
        use iceberg::io::FileIOBuilder;

        if path.starts_with("s3://") || path.starts_with("s3a://") {
            // For S3, build FileIO with s3 scheme
            // Read credentials and config from environment
            let mut builder = FileIOBuilder::new("s3");

            // S3 credentials
            if let Ok(key) = std::env::var("AWS_ACCESS_KEY_ID") {
                builder = builder.with_prop("s3.access-key-id", key);
            }
            if let Ok(secret) = std::env::var("AWS_SECRET_ACCESS_KEY") {
                builder = builder.with_prop("s3.secret-access-key", secret);
            }
            if let Ok(token) = std::env::var("AWS_SESSION_TOKEN") {
                builder = builder.with_prop("s3.session-token", token);
            }

            // S3 endpoint (for MinIO or other S3-compatible storage)
            if let Ok(endpoint) = std::env::var("AWS_ENDPOINT_URL") {
                builder = builder.with_prop("s3.endpoint", endpoint);
            }

            // S3 region
            if let Ok(region) = std::env::var("AWS_REGION") {
                builder = builder.with_prop("s3.region", region);
            } else if let Ok(region) = std::env::var("AWS_DEFAULT_REGION") {
                builder = builder.with_prop("s3.region", region);
            } else {
                // Default region for MinIO/local S3
                builder = builder.with_prop("s3.region", "us-east-1");
            }

            // Enable path-style access for MinIO
            builder = builder.with_prop("s3.path-style-access", "true");

            builder
                .build()
                .map_err(|e| Error::General(format!("Failed to create S3 FileIO: {}", e)))
        } else if path.starts_with("gs://") || path.starts_with("gcs://") {
            // For GCS
            FileIOBuilder::new("gcs")
                .build()
                .map_err(|e| Error::General(format!("Failed to create GCS FileIO: {}", e)))
        } else if path.starts_with("az://") || path.starts_with("abfs://") || path.starts_with("abfss://") {
            // For Azure
            FileIOBuilder::new("azblob")
                .build()
                .map_err(|e| Error::General(format!("Failed to create Azure FileIO: {}", e)))
        } else {
            // Local filesystem
            FileIOBuilder::new_fs_io()
                .build()
                .map_err(|e| Error::General(format!("Failed to create FileIO: {}", e)))
        }
    }

    /// Find the latest Iceberg metadata file using storage backend
    #[cfg(feature = "iceberg")]
    async fn find_iceberg_metadata(
        path: &str,
        storage: &Arc<dyn StorageBackend>,
    ) -> Result<String> {
        use crate::core::storage::traits::{GetOptions, ListOptions};

        let path = path.trim_end_matches('/');
        let metadata_dir = format!("{}/metadata", path);

        // Try to read version-hint.text first
        let version_hint_path = format!("{}/version-hint.text", metadata_dir);
        let get_opts = GetOptions::default();

        if let Ok(version_bytes) = storage.get(&version_hint_path, &get_opts).await {
            let version_str = String::from_utf8_lossy(&version_bytes).trim().to_string();
            if let Ok(version) = version_str.parse::<i32>() {
                return Ok(format!("{}/v{}.metadata.json", metadata_dir, version));
            }
        }

        // Fallback: list metadata directory and find the latest metadata file
        // Use full path with scheme - storage backend handles it
        let list_opts = ListOptions {
            prefix: Some(format!("{}/", metadata_dir)),
            delimiter: None,
            max_results: Some(100),
            continuation_token: None,
        };

        let result = storage.list(&list_opts).await.map_err(|e| {
            Error::General(format!("Failed to list metadata directory: {}", e))
        })?;

        // Find the latest metadata.json file
        // Iceberg metadata files can be:
        // - v1.metadata.json, v2.metadata.json (version prefix)
        // - 00001-uuid.metadata.json (sequence number prefix)
        let mut max_version = 0i64;
        let mut metadata_path: Option<String> = None;

        for obj in &result.objects {
            if !obj.path.ends_with(".metadata.json") {
                continue;
            }

            let name = obj.path.rsplit('/').next().unwrap_or(&obj.path);

            // Try v*.metadata.json format
            if name.starts_with('v') {
                if let Some(v_str) = name
                    .strip_prefix('v')
                    .and_then(|s| s.strip_suffix(".metadata.json"))
                {
                    if let Ok(v) = v_str.parse::<i64>() {
                        if v > max_version {
                            max_version = v;
                            metadata_path = Some(obj.path.clone());
                        }
                    }
                }
            }
            // Try 00001-uuid.metadata.json format
            else if let Some(seq_str) = name.split('-').next() {
                if let Ok(v) = seq_str.parse::<i64>() {
                    if v > max_version {
                        max_version = v;
                        metadata_path = Some(obj.path.clone());
                    }
                }
            }
        }

        metadata_path.ok_or_else(|| {
            Error::General(format!(
                "No metadata file found in {}",
                metadata_dir
            ))
        })
    }

    /// Output history as a table
    fn output_table(entries: &[HistoryEntry], is_delta: bool) -> Result<()> {
        if entries.is_empty() {
            println!("No history entries found.");
            return Ok(());
        }

        let version_label = if is_delta { "Version" } else { "Snapshot ID" };

        println!(
            "{:>12} | {:^19} | {:^15} | {}",
            version_label.bold(),
            "Timestamp".bold(),
            "Operation".bold(),
            "Details".bold()
        );
        println!("{}", "-".repeat(80));

        for entry in entries {
            let timestamp = entry.timestamp.format("%Y-%m-%d %H:%M:%S");

            // Format key details
            let details_str = Self::format_details(&entry.details);

            println!(
                "{:>12} | {} | {:^15} | {}",
                entry.version.to_string().cyan(),
                timestamp,
                entry.operation.green(),
                details_str.dimmed()
            );
        }

        println!();
        println!("Total: {} entries", entries.len());

        Ok(())
    }

    /// Format details map into a string
    fn format_details(details: &std::collections::HashMap<String, String>) -> String {
        let interesting_keys = [
            "numFiles",
            "numOutputRows",
            "numAddedFiles",
            "added-data-files",
            "added-records",
            "total-records",
            "bytesAdded",
        ];

        let parts: Vec<String> = interesting_keys
            .iter()
            .filter_map(|k| details.get(*k).map(|v| format!("{}={}", k, v)))
            .collect();

        if parts.is_empty() {
            "-".to_string()
        } else {
            parts.join(", ")
        }
    }

    /// Output history as JSON
    fn output_json(entries: &[HistoryEntry]) -> Result<()> {
        let json_entries: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "version": e.version,
                    "timestamp": e.timestamp.to_rfc3339(),
                    "operation": e.operation,
                    "details": e.details,
                })
            })
            .collect();

        println!(
            "{}",
            serde_json::to_string_pretty(&json_entries)
                .map_err(|e| Error::General(format!("Failed to serialize: {}", e)))?
        );

        Ok(())
    }
}
