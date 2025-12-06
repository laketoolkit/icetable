//! Doctor command implementation
//!
//! Diagnoses environment health and table integrity.
//!
//! Two modes:
//! 1. Environment check (no table path): Checks credentials, config, connectivity
//! 2. Table integrity check (with --table): Validates Iceberg table structure
//!    similar to `git fsck`

use colored::Colorize;
use comfy_table::{presets::UTF8_FULL, Cell, Color, Table};
use std::sync::Arc;

use crate::cli::parser::DoctorArgs;
use crate::core::storage::{StorageBackend, StorageBackendFactory};
use crate::error::Result;

/// Check result status
#[derive(Debug, Clone, Copy, PartialEq)]
enum CheckStatus {
    Ok,
    Warning,
    Error,
}

impl CheckStatus {
    fn symbol(&self) -> String {
        match self {
            CheckStatus::Ok => "✓".green().to_string(),
            CheckStatus::Warning => "⚠".yellow().to_string(),
            CheckStatus::Error => "✗".red().to_string(),
        }
    }

    fn color(&self) -> Color {
        match self {
            CheckStatus::Ok => Color::Rgb { r: 80, g: 200, b: 120 },       // Medium green
            CheckStatus::Warning => Color::Rgb { r: 220, g: 180, b: 60 },  // Gold/amber
            CheckStatus::Error => Color::Rgb { r: 220, g: 90, b: 90 },     // Medium red
        }
    }
}

/// Result of a single diagnostic check
struct CheckResult {
    name: String,
    status: CheckStatus,
    message: String,
    suggestion: Option<String>,
}

/// Handler for doctor command
pub struct DoctorCommand;

impl DoctorCommand {
    /// Execute doctor command
    pub async fn execute(args: DoctorArgs) -> Result<()> {
        use crate::config::ResolvePath;

        // Try to resolve the table path (from -t or from config)
        match args.path.resolve() {
            Ok(path) => {
                // Table path available, run table integrity checks
                Self::execute_table_check(&args, &path).await
            }
            Err(_) => {
                // No table path, run environment health checks
                Self::execute_environment_check(&args).await
            }
        }
    }

    /// Run environment health checks (credentials, config, etc.)
    async fn execute_environment_check(args: &DoctorArgs) -> Result<()> {
        // Warn if --check-files is used without a table
        if args.check_files {
            println!(
                "{}: --check-files requires a table path (-t or configured via 'icetable config use'). Ignoring.",
                "Warning".yellow()
            );
            println!();
        }

        println!(
            "{}",
            "icetable doctor - Environment Health Check".bold().cyan()
        );
        println!();

        let mut checks = Vec::new();

        // Run all diagnostic checks
        checks.push(Self::check_rust_version().await);
        checks.push(Self::check_config_file().await);
        checks.push(Self::check_aws_credentials().await);
        checks.push(Self::check_aws_endpoint().await);
        checks.push(Self::check_gcs_credentials().await);
        checks.push(Self::check_azure_credentials().await);

        // Display results
        if args.output == "json" {
            Self::display_json(&checks);
        } else {
            Self::display_table(&checks);
        }

        // Summary
        let errors = checks.iter().filter(|c| c.status == CheckStatus::Error).count();
        let warnings = checks
            .iter()
            .filter(|c| c.status == CheckStatus::Warning)
            .count();
        let ok = checks.iter().filter(|c| c.status == CheckStatus::Ok).count();

        println!();
        println!(
            "{}: {} passed, {} warnings, {} errors",
            "Summary".bold(),
            ok.to_string().green(),
            warnings.to_string().yellow(),
            errors.to_string().red()
        );

        if errors > 0 {
            println!();
            println!(
                "{}",
                "Some checks failed. Review the suggestions above to fix issues."
                    .yellow()
            );
        }

        Ok(())
    }

    /// Check Rust toolchain version
    async fn check_rust_version() -> CheckResult {
        let version = env!("CARGO_PKG_VERSION");
        CheckResult {
            name: "icetable version".to_string(),
            status: CheckStatus::Ok,
            message: format!("v{}", version),
            suggestion: None,
        }
    }

    /// Check config file exists and is readable
    async fn check_config_file() -> CheckResult {
        use crate::config::Config;

        match Config::config_path() {
            Ok(path) => {
                if path.exists() {
                    match Config::load() {
                        Ok(_) => CheckResult {
                            name: "Config file".to_string(),
                            status: CheckStatus::Ok,
                            message: path.display().to_string(),
                            suggestion: None,
                        },
                        Err(e) => CheckResult {
                            name: "Config file".to_string(),
                            status: CheckStatus::Warning,
                            message: format!("Parse error: {}", e),
                            suggestion: Some("Check config file syntax".to_string()),
                        },
                    }
                } else {
                    CheckResult {
                        name: "Config file".to_string(),
                        status: CheckStatus::Ok,
                        message: "Not created yet (will use defaults)".to_string(),
                        suggestion: None,
                    }
                }
            }
            Err(e) => CheckResult {
                name: "Config file".to_string(),
                status: CheckStatus::Warning,
                message: format!("Cannot determine path: {}", e),
                suggestion: Some("Check HOME environment variable".to_string()),
            },
        }
    }

    /// Check AWS credentials
    async fn check_aws_credentials() -> CheckResult {
        let access_key = std::env::var("AWS_ACCESS_KEY_ID").ok();
        let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY").ok();
        let profile = std::env::var("AWS_PROFILE").ok();

        match (access_key, secret_key, profile) {
            (Some(ak), Some(_), _) => {
                // Mask the key for display
                let masked = if ak.len() > 4 {
                    format!("{}...{}", &ak[..4], &ak[ak.len() - 4..])
                } else {
                    "****".to_string()
                };
                CheckResult {
                    name: "AWS credentials".to_string(),
                    status: CheckStatus::Ok,
                    message: format!("Access key: {}", masked),
                    suggestion: None,
                }
            }
            (_, _, Some(profile)) => CheckResult {
                name: "AWS credentials".to_string(),
                status: CheckStatus::Ok,
                message: format!("Using profile: {}", profile),
                suggestion: None,
            },
            _ => {
                // Check if ~/.aws/credentials exists
                let home = std::env::var("HOME").unwrap_or_default();
                let creds_path = std::path::Path::new(&home).join(".aws/credentials");

                if creds_path.exists() {
                    CheckResult {
                        name: "AWS credentials".to_string(),
                        status: CheckStatus::Ok,
                        message: "Using credentials file".to_string(),
                        suggestion: None,
                    }
                } else {
                    CheckResult {
                        name: "AWS credentials".to_string(),
                        status: CheckStatus::Warning,
                        message: "Not configured".to_string(),
                        suggestion: Some(
                            "Set AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY, or run 'aws configure'"
                                .to_string(),
                        ),
                    }
                }
            }
        }
    }

    /// Check AWS endpoint (for MinIO/LocalStack)
    async fn check_aws_endpoint() -> CheckResult {
        match std::env::var("AWS_ENDPOINT_URL") {
            Ok(endpoint) => CheckResult {
                name: "AWS endpoint".to_string(),
                status: CheckStatus::Ok,
                message: endpoint,
                suggestion: None,
            },
            Err(_) => CheckResult {
                name: "AWS endpoint".to_string(),
                status: CheckStatus::Ok,
                message: "Default (AWS S3)".to_string(),
                suggestion: None,
            },
        }
    }

    /// Check GCS credentials
    async fn check_gcs_credentials() -> CheckResult {
        let app_creds = std::env::var("GOOGLE_APPLICATION_CREDENTIALS").ok();

        match app_creds {
            Some(path) => {
                if std::path::Path::new(&path).exists() {
                    CheckResult {
                        name: "GCS credentials".to_string(),
                        status: CheckStatus::Ok,
                        message: format!("Service account: {}", path),
                        suggestion: None,
                    }
                } else {
                    CheckResult {
                        name: "GCS credentials".to_string(),
                        status: CheckStatus::Error,
                        message: format!("File not found: {}", path),
                        suggestion: Some("Check GOOGLE_APPLICATION_CREDENTIALS path".to_string()),
                    }
                }
            }
            None => {
                // Check for default credentials location
                let home = std::env::var("HOME").unwrap_or_default();
                let default_path =
                    std::path::Path::new(&home).join(".config/gcloud/application_default_credentials.json");

                if default_path.exists() {
                    CheckResult {
                        name: "GCS credentials".to_string(),
                        status: CheckStatus::Ok,
                        message: "Using application default credentials".to_string(),
                        suggestion: None,
                    }
                } else {
                    CheckResult {
                        name: "GCS credentials".to_string(),
                        status: CheckStatus::Warning,
                        message: "Not configured".to_string(),
                        suggestion: Some(
                            "Run 'gcloud auth application-default login' or set GOOGLE_APPLICATION_CREDENTIALS"
                                .to_string(),
                        ),
                    }
                }
            }
        }
    }

    /// Check Azure credentials
    async fn check_azure_credentials() -> CheckResult {
        let storage_account = std::env::var("AZURE_STORAGE_ACCOUNT").ok();
        let storage_key = std::env::var("AZURE_STORAGE_KEY").ok();
        let connection_string = std::env::var("AZURE_STORAGE_CONNECTION_STRING").ok();

        match (storage_account, storage_key, connection_string) {
            (Some(account), Some(_), _) => CheckResult {
                name: "Azure credentials".to_string(),
                status: CheckStatus::Ok,
                message: format!("Account: {}", account),
                suggestion: None,
            },
            (_, _, Some(_)) => CheckResult {
                name: "Azure credentials".to_string(),
                status: CheckStatus::Ok,
                message: "Using connection string".to_string(),
                suggestion: None,
            },
            _ => CheckResult {
                name: "Azure credentials".to_string(),
                status: CheckStatus::Warning,
                message: "Not configured".to_string(),
                suggestion: Some(
                    "Set AZURE_STORAGE_ACCOUNT and AZURE_STORAGE_KEY, or run 'az login'".to_string(),
                ),
            },
        }
    }

    /// Display results as a table
    fn display_table(checks: &[CheckResult]) {
        use comfy_table::ContentArrangement;

        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_content_arrangement(ContentArrangement::Dynamic);

        table.set_header(vec![
            Cell::new("Status").fg(Color::Cyan).set_alignment(comfy_table::CellAlignment::Center),
            Cell::new("Check").fg(Color::Cyan).set_alignment(comfy_table::CellAlignment::Center),
            Cell::new("Result").fg(Color::Cyan).set_alignment(comfy_table::CellAlignment::Center),
        ]);

        for check in checks {
            let row = vec![
                Cell::new(check.status.symbol()).set_alignment(comfy_table::CellAlignment::Center),
                Cell::new(&check.name),
                Cell::new(&check.message).fg(check.status.color()),
            ];

            table.add_row(row);
        }

        println!("{}", table);

        // Print suggestions for issues
        let issues: Vec<_> = checks
            .iter()
            .filter(|c| c.suggestion.is_some() && c.status != CheckStatus::Ok)
            .collect();

        if !issues.is_empty() {
            println!();
            println!("{}", "Suggestions:".bold());
            for check in issues {
                if let Some(ref suggestion) = check.suggestion {
                    println!(
                        "  {} {}: {}",
                        "→".dimmed(),
                        check.name.cyan(),
                        suggestion
                    );
                }
            }
        }
    }

    /// Display results as JSON
    fn display_json(checks: &[CheckResult]) {
        let json_checks: Vec<_> = checks
            .iter()
            .map(|c| {
                serde_json::json!({
                    "name": c.name,
                    "status": match c.status {
                        CheckStatus::Ok => "ok",
                        CheckStatus::Warning => "warning",
                        CheckStatus::Error => "error",
                    },
                    "message": c.message,
                    "suggestion": c.suggestion,
                })
            })
            .collect();

        let json = serde_json::json!({
            "checks": json_checks,
            "summary": {
                "ok": checks.iter().filter(|c| c.status == CheckStatus::Ok).count(),
                "warnings": checks.iter().filter(|c| c.status == CheckStatus::Warning).count(),
                "errors": checks.iter().filter(|c| c.status == CheckStatus::Error).count(),
            }
        });

        println!(
            "{}",
            serde_json::to_string_pretty(&json).unwrap_or_default()
        );
    }

    // ========================================================================
    // Table Integrity Check (like git fsck)
    // ========================================================================

    /// Run table integrity checks (metadata, manifests, data files)
    async fn execute_table_check(args: &DoctorArgs, table_path: &str) -> Result<()> {
        let is_json = args.output == "json";

        if !is_json {
            println!(
                "{}",
                "icetable doctor - Table Integrity Check".bold().cyan()
            );
            println!("Table: {}", table_path.cyan());
            println!();
        }

        let mut checks = Vec::new();

        // Create storage backend
        let storage: Arc<dyn StorageBackend> =
            match StorageBackendFactory::create_backend(table_path).await {
                Ok(s) => s.into(),
                Err(e) => {
                    checks.push(CheckResult {
                        name: "Storage Access".to_string(),
                        status: CheckStatus::Error,
                        message: format!("Failed to connect: {}", e),
                        suggestion: Some(
                            "Check storage URL format and credentials".to_string(),
                        ),
                    });

                    if args.output == "json" {
                        Self::display_json(&checks);
                    } else {
                        Self::display_table(&checks);
                    }
                    return Ok(());
                }
            };

        checks.push(CheckResult {
            name: "Storage Access".to_string(),
            status: CheckStatus::Ok,
            message: format!("Connected to {}", storage.storage_type()),
            suggestion: None,
        });

        // Check 1: version-hint.text exists and is valid
        let version_hint_check =
            Self::check_version_hint(&storage, table_path).await;
        let current_version = match &version_hint_check {
            CheckResult {
                status: CheckStatus::Ok,
                message,
                ..
            } => message.split(' ').next().and_then(|v| v.parse::<i32>().ok()),
            _ => None,
        };
        checks.push(version_hint_check);

        // Check 2: Metadata JSON is valid and parseable
        let (metadata_check, metadata) =
            Self::check_metadata_json(&storage, table_path, current_version).await;
        checks.push(metadata_check);

        // If metadata is valid, run additional checks
        if let Some(ref meta) = metadata {
            // Check 3: Snapshot graph validity
            checks.push(Self::check_snapshot_graph(meta));

            // Check 4: Current snapshot reference
            checks.push(Self::check_current_snapshot(meta));

            // Check 5: Manifest files exist
            let manifest_check =
                Self::check_manifests_exist(&storage, table_path, meta).await;
            checks.push(manifest_check);

            // Check 6 (optional): Data files exist
            if args.check_files {
                let data_files_check =
                    Self::check_data_files_exist(&storage, table_path, meta).await;
                checks.push(data_files_check);
            }
        }

        // Display results
        if is_json {
            Self::display_json(&checks);
        } else {
            Self::display_table(&checks);

            // Summary (only in human mode)
            let errors = checks.iter().filter(|c| c.status == CheckStatus::Error).count();
            let warnings = checks
                .iter()
                .filter(|c| c.status == CheckStatus::Warning)
                .count();
            let ok = checks.iter().filter(|c| c.status == CheckStatus::Ok).count();

            println!();
            println!(
                "{}: {} passed, {} warnings, {} errors",
                "Summary".bold(),
                ok.to_string().green(),
                warnings.to_string().yellow(),
                errors.to_string().red()
            );

            if errors > 0 {
                println!();
                println!(
                    "{}",
                    "Table integrity issues found. Review the errors above.".red()
                );
            } else if warnings > 0 {
                println!();
                println!(
                    "{}",
                    "Table appears healthy but has warnings worth reviewing.".yellow()
                );
            } else {
                println!();
                println!("{}", "Table integrity verified. No issues found.".green());
            }
        }

        Ok(())
    }

    /// Check version-hint.text
    async fn check_version_hint(
        storage: &Arc<dyn StorageBackend>,
        table_path: &str,
    ) -> CheckResult {
        use crate::core::storage::traits::GetOptions;

        let version_hint_path = format!("{}/metadata/version-hint.text", table_path.trim_end_matches('/'));
        let get_opts = GetOptions::default();

        match storage.get(&version_hint_path, &get_opts).await {
            Ok(bytes) => {
                let content = String::from_utf8_lossy(&bytes).trim().to_string();
                match content.parse::<i32>() {
                    Ok(version) => CheckResult {
                        name: "version-hint.text".to_string(),
                        status: CheckStatus::Ok,
                        message: format!("{} (valid)", version),
                        suggestion: None,
                    },
                    Err(_) => CheckResult {
                        name: "version-hint.text".to_string(),
                        status: CheckStatus::Warning,
                        message: format!("Invalid content: '{}'", content),
                        suggestion: Some(
                            "version-hint.text should contain a single integer".to_string(),
                        ),
                    },
                }
            }
            Err(_) => CheckResult {
                name: "version-hint.text".to_string(),
                status: CheckStatus::Warning,
                message: "Not found (optional file)".to_string(),
                suggestion: Some(
                    "Without version-hint.text, clients must scan metadata/ directory".to_string(),
                ),
            },
        }
    }

    /// Check metadata JSON is valid
    async fn check_metadata_json(
        storage: &Arc<dyn StorageBackend>,
        table_path: &str,
        version: Option<i32>,
    ) -> (CheckResult, Option<serde_json::Value>) {
        use crate::core::storage::traits::{GetOptions, ListOptions};

        // Find metadata file
        let metadata_path = if let Some(v) = version {
            format!(
                "{}/metadata/v{}.metadata.json",
                table_path.trim_end_matches('/'),
                v
            )
        } else {
            // List metadata directory to find latest
            let list_opts = ListOptions {
                prefix: Some(format!("{}/metadata/", table_path.trim_end_matches('/'))),
                delimiter: None,
                max_results: None,
                continuation_token: None,
            };

            match storage.list(&list_opts).await {
                Ok(result) => {
                    let mut metadata_files: Vec<_> = result
                        .objects
                        .iter()
                        .filter(|o| o.path.ends_with(".metadata.json"))
                        .collect();

                    metadata_files.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));

                    match metadata_files.first() {
                        Some(f) => f.path.clone(),
                        None => {
                            return (
                                CheckResult {
                                    name: "Metadata JSON".to_string(),
                                    status: CheckStatus::Error,
                                    message: "No metadata files found".to_string(),
                                    suggestion: Some(
                                        "Table metadata/ directory is empty or corrupted".to_string(),
                                    ),
                                },
                                None,
                            );
                        }
                    }
                }
                Err(e) => {
                    return (
                        CheckResult {
                            name: "Metadata JSON".to_string(),
                            status: CheckStatus::Error,
                            message: format!("Cannot list metadata: {}", e),
                            suggestion: Some("Check storage permissions".to_string()),
                        },
                        None,
                    );
                }
            }
        };

        // Read and parse metadata
        let get_opts = GetOptions::default();
        match storage.get(&metadata_path, &get_opts).await {
            Ok(bytes) => {
                let content = String::from_utf8_lossy(&bytes);
                match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(meta) => {
                        // Validate required fields
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
                                CheckResult {
                                    name: "Metadata JSON".to_string(),
                                    status: CheckStatus::Ok,
                                    message: format!(
                                        "{} (format v{})",
                                        filename, format_version
                                    ),
                                    suggestion: None,
                                },
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
                                CheckResult {
                                    name: "Metadata JSON".to_string(),
                                    status: CheckStatus::Error,
                                    message: format!("Missing fields: {}", missing.join(", ")),
                                    suggestion: Some(
                                        "Metadata file is incomplete or corrupted".to_string(),
                                    ),
                                },
                                None,
                            )
                        }
                    }
                    Err(e) => (
                        CheckResult {
                            name: "Metadata JSON".to_string(),
                            status: CheckStatus::Error,
                            message: format!("Parse error: {}", e),
                            suggestion: Some("Metadata file contains invalid JSON".to_string()),
                        },
                        None,
                    ),
                }
            }
            Err(e) => (
                CheckResult {
                    name: "Metadata JSON".to_string(),
                    status: CheckStatus::Error,
                    message: format!("Cannot read: {}", e),
                    suggestion: Some("Metadata file is missing or inaccessible".to_string()),
                },
                None,
            ),
        }
    }

    /// Check snapshot graph for cycles
    fn check_snapshot_graph(metadata: &serde_json::Value) -> CheckResult {
        let snapshots = match metadata.get("snapshots").and_then(|s| s.as_array()) {
            Some(s) => s,
            None => {
                return CheckResult {
                    name: "Snapshot Graph".to_string(),
                    status: CheckStatus::Ok,
                    message: "No snapshots (empty table)".to_string(),
                    suggestion: None,
                };
            }
        };

        if snapshots.is_empty() {
            return CheckResult {
                name: "Snapshot Graph".to_string(),
                status: CheckStatus::Ok,
                message: "No snapshots (empty table)".to_string(),
                suggestion: None,
            };
        }

        // Build snapshot ID set
        let snapshot_ids: std::collections::HashSet<i64> = snapshots
            .iter()
            .filter_map(|s| s.get("snapshot-id").and_then(|id| id.as_i64()))
            .collect();

        // Check for cycles (simple: ensure parent exists or is 0/-1)
        let mut orphan_count = 0;
        for snapshot in snapshots {
            if let Some(parent_id) = snapshot.get("parent-snapshot-id").and_then(|id| id.as_i64()) {
                if parent_id > 0 && !snapshot_ids.contains(&parent_id) {
                    orphan_count += 1;
                }
            }
        }

        if orphan_count > 0 {
            CheckResult {
                name: "Snapshot Graph".to_string(),
                status: CheckStatus::Warning,
                message: format!(
                    "{} snapshots, {} orphan references",
                    snapshots.len(),
                    orphan_count
                ),
                suggestion: Some(
                    "Some snapshots reference expired parents (normal after expire)".to_string(),
                ),
            }
        } else {
            CheckResult {
                name: "Snapshot Graph".to_string(),
                status: CheckStatus::Ok,
                message: format!("{} snapshots, no cycles", snapshots.len()),
                suggestion: None,
            }
        }
    }

    /// Check current snapshot reference is valid
    fn check_current_snapshot(metadata: &serde_json::Value) -> CheckResult {
        let current_id = metadata
            .get("current-snapshot-id")
            .and_then(|id| id.as_i64());

        match current_id {
            Some(-1) | None => CheckResult {
                name: "Current Snapshot".to_string(),
                status: CheckStatus::Ok,
                message: "No current snapshot (empty table)".to_string(),
                suggestion: None,
            },
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
                    CheckResult {
                        name: "Current Snapshot".to_string(),
                        status: CheckStatus::Ok,
                        message: format!("ID {} exists", id),
                        suggestion: None,
                    }
                } else {
                    CheckResult {
                        name: "Current Snapshot".to_string(),
                        status: CheckStatus::Error,
                        message: format!("ID {} not found in snapshots", id),
                        suggestion: Some(
                            "Current snapshot reference is invalid. Table may be corrupted."
                                .to_string(),
                        ),
                    }
                }
            }
        }
    }

    /// Check that manifest files exist
    async fn check_manifests_exist(
        storage: &Arc<dyn StorageBackend>,
        table_path: &str,
        metadata: &serde_json::Value,
    ) -> CheckResult {
        use crate::core::storage::traits::GetOptions;

        let current_id = metadata
            .get("current-snapshot-id")
            .and_then(|id| id.as_i64())
            .unwrap_or(-1);

        if current_id == -1 {
            return CheckResult {
                name: "Manifest Files".to_string(),
                status: CheckStatus::Ok,
                message: "No manifests (empty table)".to_string(),
                suggestion: None,
            };
        }

        let snapshots = match metadata.get("snapshots").and_then(|s| s.as_array()) {
            Some(s) => s,
            None => {
                return CheckResult {
                    name: "Manifest Files".to_string(),
                    status: CheckStatus::Ok,
                    message: "No snapshots".to_string(),
                    suggestion: None,
                };
            }
        };

        // Find current snapshot
        let current_snapshot = snapshots.iter().find(|s| {
            s.get("snapshot-id").and_then(|id| id.as_i64()) == Some(current_id)
        });

        let manifest_list_path = match current_snapshot
            .and_then(|s| s.get("manifest-list"))
            .and_then(|m| m.as_str())
        {
            Some(p) => {
                let table_location = metadata
                    .get("location")
                    .and_then(|l| l.as_str())
                    .unwrap_or(table_path);

                Self::resolve_path(table_location, p)
            }
            None => {
                return CheckResult {
                    name: "Manifest Files".to_string(),
                    status: CheckStatus::Error,
                    message: "No manifest-list in current snapshot".to_string(),
                    suggestion: Some("Snapshot metadata is incomplete".to_string()),
                };
            }
        };

        // Check manifest-list exists
        let get_opts = GetOptions::default();
        let manifest_list_bytes = match storage.get(&manifest_list_path, &get_opts).await {
            Ok(b) => b,
            Err(_) => {
                return CheckResult {
                    name: "Manifest Files".to_string(),
                    status: CheckStatus::Error,
                    message: "Manifest list file not found".to_string(),
                    suggestion: Some(format!("Missing: {}", manifest_list_path)),
                };
            }
        };

        // Parse manifest list to get manifest paths
        let manifest_reader = match apache_avro::Reader::new(&manifest_list_bytes[..]) {
            Ok(r) => r,
            Err(e) => {
                return CheckResult {
                    name: "Manifest Files".to_string(),
                    status: CheckStatus::Error,
                    message: format!("Cannot parse manifest list: {}", e),
                    suggestion: Some("Manifest list file is corrupted".to_string()),
                };
            }
        };

        let table_location = metadata
            .get("location")
            .and_then(|l| l.as_str())
            .unwrap_or(table_path);

        let mut manifest_paths = Vec::new();
        for value_result in manifest_reader {
            if let Ok(apache_avro::types::Value::Record(fields)) = value_result {
                if let Some(path) = fields
                    .iter()
                    .find(|(name, _)| name == "manifest-path" || name == "manifest_path")
                    .and_then(|(_, v)| {
                        if let apache_avro::types::Value::String(s) = v {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                {
                    manifest_paths.push(Self::resolve_path(table_location, &path));
                }
            }
        }

        // Check each manifest exists
        let mut missing = 0;
        for path in &manifest_paths {
            if !storage.exists(path).await.unwrap_or(false) {
                missing += 1;
            }
        }

        if missing > 0 {
            CheckResult {
                name: "Manifest Files".to_string(),
                status: CheckStatus::Error,
                message: format!(
                    "{}/{} manifests missing",
                    missing,
                    manifest_paths.len()
                ),
                suggestion: Some("Some manifest files are missing. Table may be corrupted.".to_string()),
            }
        } else {
            CheckResult {
                name: "Manifest Files".to_string(),
                status: CheckStatus::Ok,
                message: format!("{} manifests verified", manifest_paths.len()),
                suggestion: None,
            }
        }
    }

    /// Check that data files exist (slow operation)
    async fn check_data_files_exist(
        storage: &Arc<dyn StorageBackend>,
        table_path: &str,
        metadata: &serde_json::Value,
    ) -> CheckResult {
        use crate::core::storage::traits::GetOptions;
        use indicatif::{ProgressBar, ProgressStyle};

        let current_id = metadata
            .get("current-snapshot-id")
            .and_then(|id| id.as_i64())
            .unwrap_or(-1);

        if current_id == -1 {
            return CheckResult {
                name: "Data Files".to_string(),
                status: CheckStatus::Ok,
                message: "No data files (empty table)".to_string(),
                suggestion: None,
            };
        }

        // Get manifest list path
        let snapshots = match metadata.get("snapshots").and_then(|s| s.as_array()) {
            Some(s) => s,
            None => {
                return CheckResult {
                    name: "Data Files".to_string(),
                    status: CheckStatus::Ok,
                    message: "No snapshots".to_string(),
                    suggestion: None,
                };
            }
        };

        let current_snapshot = snapshots.iter().find(|s| {
            s.get("snapshot-id").and_then(|id| id.as_i64()) == Some(current_id)
        });

        let table_location = metadata
            .get("location")
            .and_then(|l| l.as_str())
            .unwrap_or(table_path);

        let manifest_list_path = match current_snapshot
            .and_then(|s| s.get("manifest-list"))
            .and_then(|m| m.as_str())
        {
            Some(p) => Self::resolve_path(table_location, p),
            None => {
                return CheckResult {
                    name: "Data Files".to_string(),
                    status: CheckStatus::Warning,
                    message: "No manifest-list to check".to_string(),
                    suggestion: None,
                };
            }
        };

        // Read manifest list
        let get_opts = GetOptions::default();
        let manifest_list_bytes = match storage.get(&manifest_list_path, &get_opts).await {
            Ok(b) => b,
            Err(_) => {
                return CheckResult {
                    name: "Data Files".to_string(),
                    status: CheckStatus::Error,
                    message: "Cannot read manifest list".to_string(),
                    suggestion: None,
                };
            }
        };

        let manifest_reader = match apache_avro::Reader::new(&manifest_list_bytes[..]) {
            Ok(r) => r,
            Err(_) => {
                return CheckResult {
                    name: "Data Files".to_string(),
                    status: CheckStatus::Error,
                    message: "Cannot parse manifest list".to_string(),
                    suggestion: None,
                };
            }
        };

        // Collect manifest paths
        let mut manifest_paths = Vec::new();
        for value_result in manifest_reader {
            if let Ok(apache_avro::types::Value::Record(fields)) = value_result {
                if let Some(path) = fields
                    .iter()
                    .find(|(name, _)| name == "manifest-path" || name == "manifest_path")
                    .and_then(|(_, v)| {
                        if let apache_avro::types::Value::String(s) = v {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                {
                    manifest_paths.push(Self::resolve_path(table_location, &path));
                }
            }
        }

        // Collect all data file paths from manifests
        let mut data_files = Vec::new();
        let pb = ProgressBar::new(manifest_paths.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} Reading manifests [{bar:40.cyan/blue}] {pos}/{len}")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("━━╺"),
        );

        for manifest_path in &manifest_paths {
            pb.inc(1);

            let manifest_bytes = match storage.get(manifest_path, &get_opts).await {
                Ok(b) => b,
                Err(_) => continue,
            };

            let manifest_reader = match apache_avro::Reader::new(&manifest_bytes[..]) {
                Ok(r) => r,
                Err(_) => continue,
            };

            for value_result in manifest_reader {
                if let Ok(apache_avro::types::Value::Record(fields)) = value_result {
                    // Look for data_file.file_path in the manifest entry
                    if let Some(data_file) = fields
                        .iter()
                        .find(|(name, _)| name == "data_file")
                        .and_then(|(_, v)| {
                            if let apache_avro::types::Value::Record(df_fields) = v {
                                Some(df_fields)
                            } else {
                                None
                            }
                        })
                    {
                        if let Some(file_path) = data_file
                            .iter()
                            .find(|(name, _)| name == "file_path")
                            .and_then(|(_, v)| {
                                if let apache_avro::types::Value::String(s) = v {
                                    Some(s.clone())
                                } else {
                                    None
                                }
                            })
                        {
                            data_files.push(Self::resolve_path(table_location, &file_path));
                        }
                    }
                }
            }
        }
        pb.finish_and_clear();

        if data_files.is_empty() {
            return CheckResult {
                name: "Data Files".to_string(),
                status: CheckStatus::Ok,
                message: "No data files in current snapshot".to_string(),
                suggestion: None,
            };
        }

        // Check data files exist (with progress)
        let total_files = data_files.len();
        let pb = ProgressBar::new(total_files as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} Checking data files [{bar:40.cyan/blue}] {pos}/{len}")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("━━╺"),
        );

        let mut missing = 0;
        for file_path in &data_files {
            pb.inc(1);
            if !storage.exists(file_path).await.unwrap_or(false) {
                missing += 1;
            }
        }
        pb.finish_and_clear();

        if missing > 0 {
            CheckResult {
                name: "Data Files".to_string(),
                status: CheckStatus::Error,
                message: format!("{}/{} files missing", missing, total_files),
                suggestion: Some(
                    "Some data files are missing. Data may have been deleted externally."
                        .to_string(),
                ),
            }
        } else {
            CheckResult {
                name: "Data Files".to_string(),
                status: CheckStatus::Ok,
                message: format!("{} files verified", total_files),
                suggestion: None,
            }
        }
    }

    /// Resolve a path that may be relative or absolute
    fn resolve_path(table_location: &str, path: &str) -> String {
        if path.starts_with("s3://")
            || path.starts_with("gs://")
            || path.starts_with("abfs://")
            || path.starts_with("file://")
            || path.starts_with('/')
        {
            path.to_string()
        } else {
            format!(
                "{}/{}",
                table_location.trim_end_matches('/'),
                path.trim_start_matches('/')
            )
        }
    }
}
