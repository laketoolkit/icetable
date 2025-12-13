//! Doctor service for environment and table health checks
//!
//! Provides diagnostic checks for:
//! 1. Environment health (credentials, config, connectivity)
//! 2. Table integrity (metadata, manifests, data files)
//!
//! # Module organization
//!
//! - `environment`: Credential and connectivity checks
//! - `table`: Table metadata and file integrity checks

mod environment;
mod table;

use crate::core::CatalogConfig;
use crate::core::metadata::IcebergMetadataService;
use crate::core::storage::{Storage, create_object_store, detect_storage_type};
use crate::error::Result;

// Internal modules - checks are exposed through DoctorService methods

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
    // Environment Checks (delegated to environment module)
    // ========================================================================

    /// Check icetable version
    pub fn check_version() -> CheckResult {
        environment::check_version()
    }

    /// Check AWS credentials
    pub fn check_aws_credentials() -> CheckResult {
        environment::check_aws_credentials()
    }

    /// Check AWS endpoint configuration
    pub fn check_aws_endpoint() -> CheckResult {
        environment::check_aws_endpoint()
    }

    /// Check GCS credentials
    pub fn check_gcs_credentials() -> CheckResult {
        environment::check_gcs_credentials()
    }

    /// Check Azure credentials
    pub fn check_azure_credentials() -> CheckResult {
        environment::check_azure_credentials()
    }

    /// Test storage connectivity
    pub async fn check_storage_connectivity() -> CheckResult {
        environment::check_storage_connectivity().await
    }

    /// Test catalog connectivity
    pub async fn test_catalog_connectivity(name: &str, catalog: &CatalogConfig) -> Result<()> {
        environment::test_catalog_connectivity(name, catalog).await
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
        let (metadata_format_check, _current_version) =
            table::check_metadata_format(&storage, table_path).await;
        checks.push(metadata_format_check);

        // Check metadata JSON
        let (metadata_check, metadata) = table::check_metadata_json(&storage, table_path).await;
        checks.push(metadata_check);

        // If metadata is valid, run additional checks
        if let Some(ref meta) = metadata {
            checks.push(table::check_snapshot_graph(meta));
            checks.push(table::check_current_snapshot(meta));

            // Try to load native service for manifest/file checks
            match IcebergMetadataService::new_async(table_path.to_string()).await {
                Ok(service) => {
                    checks.push(table::check_manifests_exist(&storage, &service).await);
                    if self.config.check_files {
                        checks.push(table::check_data_files_exist(&storage, &service).await);
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
}

impl Default for DoctorService {
    fn default() -> Self {
        Self::new()
    }
}
