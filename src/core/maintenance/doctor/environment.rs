//! Environment health checks
//!
//! Checks for credentials, endpoints, and storage connectivity.

use crate::core::storage::create_object_store;
use crate::core::{CatalogConfig, CatalogType};
use crate::error::{Error, Result};

use super::CheckResult;

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

/// Test catalog connectivity
pub async fn test_catalog_connectivity(name: &str, catalog: &CatalogConfig) -> Result<()> {
    log::debug!("Testing connectivity to catalog: {}", name);

    if catalog.catalog_type == CatalogType::Rest {
        if catalog.uri.is_empty() {
            return Err(Error::Configuration {
                message: "REST catalog URI is empty".to_string(),
            });
        }

        if !catalog.uri.starts_with("http://") && !catalog.uri.starts_with("https://") {
            return Err(Error::Configuration {
                message: format!(
                    "REST catalog URI should start with http:// or https://: {}",
                    catalog.uri
                ),
            });
        }

        let client = reqwest::Client::new();
        let config_url = format!("{}/v1/config", catalog.uri.trim_end_matches('/'));
        match client
            .get(&config_url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                log::debug!("REST catalog {} is reachable", catalog.uri);
                Ok(())
            }
            Ok(resp) => Err(Error::Network {
                message: format!("REST catalog returned status {}", resp.status()),
                source: None,
            }),
            Err(e) => Err(Error::Network {
                message: format!("Cannot connect to REST catalog: {}", e),
                source: None,
            }),
        }
    } else {
        log::debug!(
            "Catalog type {} configuration validated",
            catalog.catalog_type
        );
        Ok(())
    }
}
