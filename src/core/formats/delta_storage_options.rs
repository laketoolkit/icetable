//! Storage options helper for Delta Lake
//!
//! This module provides utilities to build storage_options HashMap
//! from StorageBackend configuration for use with delta-rs.

use std::collections::HashMap;

use crate::core::storage::{S3Backend, StorageBackend};
use crate::error::{Error, Result};

/// Build storage options for delta-rs from a StorageBackend
pub fn build_storage_options(
    storage: &dyn StorageBackend,
    uri: &str,
) -> Result<HashMap<String, String>> {
    let mut options = HashMap::new();

    // Determine storage type from URI
    if uri.starts_with("s3://") || uri.starts_with("s3a://") {
        // S3 backend
        if storage.storage_type() == "s3" {
            // Try to downcast to S3Backend to extract credentials
            // Since we can't downcast trait objects directly, we need to pass
            // the configuration through the storage backend interface

            // For now, we'll use a workaround: check if AWS env vars are set
            // In a production system, you'd want to extract this from S3Backend
            if let Ok(access_key) = std::env::var("AWS_ACCESS_KEY_ID") {
                options.insert("AWS_ACCESS_KEY_ID".to_string(), access_key);
            }

            if let Ok(secret_key) = std::env::var("AWS_SECRET_ACCESS_KEY") {
                options.insert("AWS_SECRET_ACCESS_KEY".to_string(), secret_key);
            }

            if let Ok(endpoint) = std::env::var("AWS_ENDPOINT_URL") {
                options.insert("AWS_ENDPOINT_URL".to_string(), endpoint);
            }

            if let Ok(region) = std::env::var("AWS_REGION") {
                options.insert("AWS_REGION".to_string(), region);
            } else {
                // Default region
                options.insert("AWS_REGION".to_string(), "us-east-1".to_string());
            }

            if let Ok(allow_http) = std::env::var("AWS_ALLOW_HTTP") {
                options.insert("AWS_ALLOW_HTTP".to_string(), allow_http);
            }

            if let Ok(unsafe_rename) = std::env::var("AWS_S3_ALLOW_UNSAFE_RENAME") {
                options.insert("AWS_S3_ALLOW_UNSAFE_RENAME".to_string(), unsafe_rename);
            }
        }
    } else if uri.starts_with("gs://") {
        // GCS backend
        if let Ok(service_account) = std::env::var("GOOGLE_SERVICE_ACCOUNT") {
            options.insert("GOOGLE_SERVICE_ACCOUNT".to_string(), service_account);
        }

        if let Ok(service_account_key) = std::env::var("GOOGLE_SERVICE_ACCOUNT_KEY") {
            options.insert(
                "GOOGLE_SERVICE_ACCOUNT_KEY".to_string(),
                service_account_key,
            );
        }
    } else if uri.starts_with("az://") || uri.starts_with("azure://") {
        // Azure backend
        if let Ok(account_name) = std::env::var("AZURE_STORAGE_ACCOUNT_NAME") {
            options.insert("AZURE_STORAGE_ACCOUNT_NAME".to_string(), account_name);
        }

        if let Ok(account_key) = std::env::var("AZURE_STORAGE_ACCOUNT_KEY") {
            options.insert("AZURE_STORAGE_ACCOUNT_KEY".to_string(), account_key);
        }

        if let Ok(sas_token) = std::env::var("AZURE_STORAGE_SAS_TOKEN") {
            options.insert("AZURE_STORAGE_SAS_TOKEN".to_string(), sas_token);
        }
    }

    Ok(options)
}

/// Build storage options from explicit parameters
pub fn build_s3_storage_options(
    access_key: &str,
    secret_key: &str,
    region: &str,
    endpoint: Option<&str>,
    allow_http: bool,
    allow_unsafe_rename: bool,
) -> HashMap<String, String> {
    let mut options = HashMap::new();

    options.insert("AWS_ACCESS_KEY_ID".to_string(), access_key.to_string());
    options.insert(
        "AWS_SECRET_ACCESS_KEY".to_string(),
        secret_key.to_string(),
    );
    options.insert("AWS_REGION".to_string(), region.to_string());

    if let Some(endpoint_url) = endpoint {
        options.insert("AWS_ENDPOINT_URL".to_string(), endpoint_url.to_string());
    }

    if allow_http {
        options.insert("AWS_ALLOW_HTTP".to_string(), "true".to_string());
    }

    if allow_unsafe_rename {
        options.insert(
            "AWS_S3_ALLOW_UNSAFE_RENAME".to_string(),
            "true".to_string(),
        );
    }

    options
}

/// Build GCS storage options from explicit parameters
pub fn build_gcs_storage_options(
    service_account: Option<&str>,
    service_account_key: Option<&str>,
) -> HashMap<String, String> {
    let mut options = HashMap::new();

    if let Some(account) = service_account {
        options.insert("GOOGLE_SERVICE_ACCOUNT".to_string(), account.to_string());
    }

    if let Some(key) = service_account_key {
        options.insert(
            "GOOGLE_SERVICE_ACCOUNT_KEY".to_string(),
            key.to_string(),
        );
    }

    options
}

/// Build Azure storage options from explicit parameters
pub fn build_azure_storage_options(
    account_name: &str,
    account_key: Option<&str>,
    sas_token: Option<&str>,
) -> HashMap<String, String> {
    let mut options = HashMap::new();

    options.insert(
        "AZURE_STORAGE_ACCOUNT_NAME".to_string(),
        account_name.to_string(),
    );

    if let Some(key) = account_key {
        options.insert("AZURE_STORAGE_ACCOUNT_KEY".to_string(), key.to_string());
    }

    if let Some(token) = sas_token {
        options.insert("AZURE_STORAGE_SAS_TOKEN".to_string(), token.to_string());
    }

    options
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_s3_storage_options() {
        let options = build_s3_storage_options(
            "access_key",
            "secret_key",
            "us-west-2",
            Some("http://localhost:9000"),
            true,
            true,
        );

        assert_eq!(options.get("AWS_ACCESS_KEY_ID"), Some(&"access_key".to_string()));
        assert_eq!(
            options.get("AWS_SECRET_ACCESS_KEY"),
            Some(&"secret_key".to_string())
        );
        assert_eq!(options.get("AWS_REGION"), Some(&"us-west-2".to_string()));
        assert_eq!(
            options.get("AWS_ENDPOINT_URL"),
            Some(&"http://localhost:9000".to_string())
        );
        assert_eq!(options.get("AWS_ALLOW_HTTP"), Some(&"true".to_string()));
        assert_eq!(
            options.get("AWS_S3_ALLOW_UNSAFE_RENAME"),
            Some(&"true".to_string())
        );
    }

    #[test]
    fn test_build_gcs_storage_options() {
        let options = build_gcs_storage_options(
            Some("service-account@project.iam.gserviceaccount.com"),
            Some("key-content"),
        );

        assert_eq!(
            options.get("GOOGLE_SERVICE_ACCOUNT"),
            Some(&"service-account@project.iam.gserviceaccount.com".to_string())
        );
        assert_eq!(
            options.get("GOOGLE_SERVICE_ACCOUNT_KEY"),
            Some(&"key-content".to_string())
        );
    }

    #[test]
    fn test_build_azure_storage_options() {
        let options = build_azure_storage_options(
            "myaccount",
            Some("account_key"),
            Some("sas_token"),
        );

        assert_eq!(
            options.get("AZURE_STORAGE_ACCOUNT_NAME"),
            Some(&"myaccount".to_string())
        );
        assert_eq!(
            options.get("AZURE_STORAGE_ACCOUNT_KEY"),
            Some(&"account_key".to_string())
        );
        assert_eq!(
            options.get("AZURE_STORAGE_SAS_TOKEN"),
            Some(&"sas_token".to_string())
        );
    }
}
