//! Doctor command implementation
//!
//! Diagnoses environment health including AWS/GCS/Azure connectivity,
//! credentials validation, and dependency checks.

use colored::Colorize;
use comfy_table::{presets::UTF8_FULL, Cell, Color, Table};

use crate::cli::parser::DoctorArgs;
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
            CheckStatus::Ok => Color::Green,
            CheckStatus::Warning => Color::Yellow,
            CheckStatus::Error => Color::Red,
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

        // If a specific path was provided, check connectivity to it
        if let Some(ref path) = args.path {
            checks.push(Self::check_storage_connectivity(path).await);
        }

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

    /// Check connectivity to a specific storage path
    async fn check_storage_connectivity(path: &str) -> CheckResult {
        use crate::core::storage::StorageBackendFactory;

        match StorageBackendFactory::create_backend(path).await {
            Ok(backend) => {
                use crate::core::storage::traits::ListOptions;

                let list_opts = ListOptions {
                    prefix: Some(format!("{}/metadata", path.trim_end_matches('/'))),
                    delimiter: Some("/".to_string()),
                    max_results: Some(1),
                    continuation_token: None,
                };

                match backend.list(&list_opts).await {
                    Ok(_) => CheckResult {
                        name: format!("Storage: {}", Self::truncate_path(path)),
                        status: CheckStatus::Ok,
                        message: "Connected successfully".to_string(),
                        suggestion: None,
                    },
                    Err(e) => {
                        let error_str = e.to_string();
                        let suggestion = if error_str.contains("403")
                            || error_str.contains("Access Denied")
                        {
                            Some("Check IAM permissions for the bucket".to_string())
                        } else if error_str.contains("404")
                            || error_str.contains("NoSuchBucket")
                        {
                            Some("Verify the bucket/path exists".to_string())
                        } else if error_str.contains("timeout")
                            || error_str.contains("connection")
                        {
                            Some("Check network connectivity and endpoint URL".to_string())
                        } else {
                            Some("Check storage configuration".to_string())
                        };

                        CheckResult {
                            name: format!("Storage: {}", Self::truncate_path(path)),
                            status: CheckStatus::Error,
                            message: Self::truncate_message(&error_str, 50),
                            suggestion,
                        }
                    }
                }
            }
            Err(e) => CheckResult {
                name: format!("Storage: {}", Self::truncate_path(path)),
                status: CheckStatus::Error,
                message: Self::truncate_message(&e.to_string(), 50),
                suggestion: Some("Check storage URL format and credentials".to_string()),
            },
        }
    }

    /// Truncate path for display
    fn truncate_path(path: &str) -> String {
        if path.len() > 40 {
            format!("{}...{}", &path[..20], &path[path.len() - 17..])
        } else {
            path.to_string()
        }
    }

    /// Truncate message for display
    fn truncate_message(msg: &str, max_len: usize) -> String {
        if msg.len() > max_len {
            format!("{}...", &msg[..max_len])
        } else {
            msg.to_string()
        }
    }

    /// Display results as a table
    fn display_table(checks: &[CheckResult]) {
        let mut table = Table::new();
        table.load_preset(UTF8_FULL);

        table.set_header(vec![
            Cell::new("Status".cyan().to_string()),
            Cell::new("Check".cyan().to_string()),
            Cell::new("Result".cyan().to_string()),
        ]);

        for check in checks {
            let mut row = vec![
                Cell::new(check.status.symbol()),
                Cell::new(&check.name),
                Cell::new(&check.message),
            ];

            // Color the result based on status
            row[2] = Cell::new(&check.message).fg(check.status.color());

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
}
