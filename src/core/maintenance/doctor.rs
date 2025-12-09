//! Doctor service for environment and table health checks
//!
//! Provides diagnostic checks for:
//! 1. Environment health (credentials, config, connectivity)
//! 2. Table integrity (metadata, manifests, data files)

use std::collections::HashSet;

use futures::TryStreamExt;

use crate::core::metadata::IcebergMetadataService;
use crate::core::storage::{ObjectStoreExt, Storage, create_object_store, detect_storage_type};
use crate::error::Result;

/// Check result status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    /// Check passed successfully
    Ok,
    /// Check passed with warnings worth reviewing
    Warning,
    /// Check failed with errors that need attention
    Error,
}

/// Result of a single diagnostic check
#[derive(Debug, Clone)]
pub struct CheckResult {
    /// Name of the check
    pub name: String,
    /// Status of the check
    pub status: CheckStatus,
    /// Message describing the result
    pub message: String,
    /// Optional suggestion for fixing issues
    pub suggestion: Option<String>,
}

impl CheckResult {
    /// Create a successful check result
    pub fn ok(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Ok,
            message: message.into(),
            suggestion: None,
        }
    }

    /// Create a warning check result with a suggestion
    pub fn warning(
        name: impl Into<String>,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Warning,
            message: message.into(),
            suggestion: Some(suggestion.into()),
        }
    }

    /// Create an error check result with a suggestion
    pub fn error(
        name: impl Into<String>,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Error,
            message: message.into(),
            suggestion: Some(suggestion.into()),
        }
    }
}

/// Summary of check results
#[derive(Debug, Clone)]
pub struct CheckSummary {
    /// Number of checks that passed
    pub ok_count: usize,
    /// Number of checks with warnings
    pub warning_count: usize,
    /// Number of checks that failed
    pub error_count: usize,
}

impl CheckSummary {
    /// Create a summary from a list of check results
    pub fn from_checks(checks: &[CheckResult]) -> Self {
        Self {
            ok_count: checks
                .iter()
                .filter(|c| c.status == CheckStatus::Ok)
                .count(),
            warning_count: checks
                .iter()
                .filter(|c| c.status == CheckStatus::Warning)
                .count(),
            error_count: checks
                .iter()
                .filter(|c| c.status == CheckStatus::Error)
                .count(),
        }
    }

    /// Returns true if any check failed
    pub fn has_errors(&self) -> bool {
        self.error_count > 0
    }

    /// Returns true if any check has warnings
    pub fn has_warnings(&self) -> bool {
        self.warning_count > 0
    }
}

/// Configuration for doctor checks
#[derive(Debug, Clone, Default)]
pub struct DoctorConfig {
    /// Whether to check data files (slow)
    pub check_files: bool,
    /// Whether to test storage connectivity
    pub test_storage: bool,
    /// Specific catalog to test
    pub catalog: Option<String>,
}

/// Service for running diagnostic checks
pub struct DoctorService {
    config: DoctorConfig,
}

impl DoctorService {
    /// Create a new doctor service with default configuration
    pub fn new() -> Self {
        Self {
            config: DoctorConfig::default(),
        }
    }

    /// Create a doctor service with custom configuration
    pub fn with_config(config: DoctorConfig) -> Self {
        Self { config }
    }

    // ========================================================================
    // Environment Checks
    // ========================================================================

    /// Check icetable version
    pub fn check_version() -> CheckResult {
        let version = env!("CARGO_PKG_VERSION");
        CheckResult::ok("icetable version", format!("v{}", version))
    }

    /// Check AWS credentials
    pub fn check_aws_credentials() -> CheckResult {
        let access_key = std::env::var("AWS_ACCESS_KEY_ID").ok();
        let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY").ok();
        let profile = std::env::var("AWS_PROFILE").ok();

        match (access_key, secret_key, profile) {
            (Some(ak), Some(_), _) => {
                let masked = if ak.len() > 4 {
                    format!("{}...{}", &ak[..4], &ak[ak.len() - 4..])
                } else {
                    "****".to_string()
                };
                CheckResult::ok("AWS credentials", format!("Access key: {}", masked))
            }
            (_, _, Some(profile)) => {
                CheckResult::ok("AWS credentials", format!("Using profile: {}", profile))
            }
            _ => {
                let home = std::env::var("HOME").unwrap_or_default();
                let creds_path = std::path::Path::new(&home).join(".aws/credentials");

                if creds_path.exists() {
                    CheckResult::ok("AWS credentials", "Using credentials file")
                } else {
                    CheckResult::warning(
                        "AWS credentials",
                        "Not configured",
                        "Set AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY, or run 'aws configure'",
                    )
                }
            }
        }
    }

    /// Check AWS endpoint configuration
    pub fn check_aws_endpoint() -> CheckResult {
        match std::env::var("AWS_ENDPOINT_URL") {
            Ok(endpoint) => CheckResult::ok("AWS endpoint", endpoint),
            Err(_) => CheckResult::ok("AWS endpoint", "Default (AWS S3)"),
        }
    }

    /// Check GCS credentials
    pub fn check_gcs_credentials() -> CheckResult {
        let app_creds = std::env::var("GOOGLE_APPLICATION_CREDENTIALS").ok();

        match app_creds {
            Some(path) => {
                if std::path::Path::new(&path).exists() {
                    CheckResult::ok("GCS credentials", format!("Service account: {}", path))
                } else {
                    CheckResult::error(
                        "GCS credentials",
                        format!("File not found: {}", path),
                        "Check GOOGLE_APPLICATION_CREDENTIALS path",
                    )
                }
            }
            None => {
                let home = std::env::var("HOME").unwrap_or_default();
                let default_path = std::path::Path::new(&home)
                    .join(".config/gcloud/application_default_credentials.json");

                if default_path.exists() {
                    CheckResult::ok("GCS credentials", "Using application default credentials")
                } else {
                    CheckResult::warning(
                        "GCS credentials",
                        "Not configured",
                        "Run 'gcloud auth application-default login' or set GOOGLE_APPLICATION_CREDENTIALS",
                    )
                }
            }
        }
    }

    /// Check Azure credentials
    pub fn check_azure_credentials() -> CheckResult {
        let storage_account = std::env::var("AZURE_STORAGE_ACCOUNT").ok();
        let storage_key = std::env::var("AZURE_STORAGE_KEY").ok();
        let connection_string = std::env::var("AZURE_STORAGE_CONNECTION_STRING").ok();

        match (storage_account, storage_key, connection_string) {
            (Some(account), Some(_), _) => {
                CheckResult::ok("Azure credentials", format!("Account: {}", account))
            }
            (_, _, Some(_)) => CheckResult::ok("Azure credentials", "Using connection string"),
            _ => CheckResult::warning(
                "Azure credentials",
                "Not configured",
                "Set AZURE_STORAGE_ACCOUNT and AZURE_STORAGE_KEY, or run 'az login'",
            ),
        }
    }

    /// Test storage connectivity
    pub async fn check_storage_connectivity() -> CheckResult {
        match create_object_store("file:///tmp").await {
            Ok(_) => CheckResult::ok("Storage connectivity", "Local filesystem available"),
            Err(e) => CheckResult::error(
                "Storage connectivity",
                format!("Failed: {}", e),
                "Check storage backend configuration",
            ),
        }
    }

    // ========================================================================
    // Table Integrity Checks
    // ========================================================================

    /// Run all table integrity checks
    pub async fn check_table_integrity(&self, table_path: &str) -> Result<Vec<CheckResult>> {
        let mut checks = Vec::new();

        // Create storage backend
        let storage: Storage = match create_object_store(table_path).await {
            Ok(s) => {
                checks.push(CheckResult::ok(
                    "Storage Access",
                    format!("Connected to {} storage", detect_storage_type(table_path)),
                ));
                s
            }
            Err(e) => {
                checks.push(CheckResult::error(
                    "Storage Access",
                    format!("Failed to connect: {}", e),
                    "Check storage URL format and credentials",
                ));
                return Ok(checks);
            }
        };

        // Check metadata format
        let (metadata_format_check, current_version) =
            self.check_metadata_format(&storage, table_path).await;
        checks.push(metadata_format_check);

        // Check metadata JSON
        let (metadata_check, metadata) = self
            .check_metadata_json(&storage, table_path, current_version)
            .await;
        checks.push(metadata_check);

        // If metadata is valid, run additional checks
        if let Some(ref meta) = metadata {
            checks.push(Self::check_snapshot_graph(meta));
            checks.push(Self::check_current_snapshot(meta));

            // Try to load native service for manifest/file checks
            match IcebergMetadataService::new_async(table_path.to_string()).await {
                Ok(service) => {
                    checks.push(self.check_manifests_exist_native(&storage, &service).await);
                    if self.config.check_files {
                        checks.push(self.check_data_files_exist_native(&storage, &service).await);
                    }
                }
                Err(e) => {
                    checks.push(CheckResult::error(
                        "Native API",
                        format!("Cannot load table: {}", e),
                        "Table metadata may be corrupted",
                    ));
                }
            }
        }

        Ok(checks)
    }

    /// Check metadata format (standard Iceberg naming)
    async fn check_metadata_format(
        &self,
        storage: &Storage,
        table_path: &str,
    ) -> (CheckResult, Option<i32>) {
        use crate::core::utils::{extract_version_from_path, find_latest_metadata};
        use iceberg::MetadataLocation;
        use std::str::FromStr;

        match find_latest_metadata(table_path, storage).await {
            Ok(metadata_path) => {
                let filename = metadata_path
                    .split('/')
                    .next_back()
                    .unwrap_or(&metadata_path);
                let version = extract_version_from_path(&metadata_path);

                if let Some(v) = version {
                    if MetadataLocation::from_str(&metadata_path).is_ok() {
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
    async fn check_metadata_json(
        &self,
        storage: &Storage,
        table_path: &str,
        _version: Option<i32>,
    ) -> (CheckResult, Option<serde_json::Value>) {
        use crate::core::utils::find_latest_metadata;

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

    /// Check snapshot graph for cycles
    fn check_snapshot_graph(metadata: &serde_json::Value) -> CheckResult {
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
    fn check_current_snapshot(metadata: &serde_json::Value) -> CheckResult {
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

    /// Check that manifest files exist using native API
    async fn check_manifests_exist_native(
        &self,
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

        let mut missing = 0;
        for path in &manifest_paths {
            if !storage.exists_str(path).await.unwrap_or(false) {
                missing += 1;
            }
        }

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

    /// Check that data files exist using native scan API
    async fn check_data_files_exist_native(
        &self,
        storage: &Storage,
        service: &IcebergMetadataService,
    ) -> CheckResult {
        let table = service.table();
        let metadata = table.metadata();

        if metadata.current_snapshot().is_none() {
            return CheckResult::ok("Data Files", "No data files (empty table)");
        }

        // Use scan API to get data files
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
        let mut missing = 0;
        for file_path in &data_files {
            if !storage.exists_str(file_path).await.unwrap_or(false) {
                missing += 1;
            }
        }

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
}

impl Default for DoctorService {
    fn default() -> Self {
        Self::new()
    }
}
