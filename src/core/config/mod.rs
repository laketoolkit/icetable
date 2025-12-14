//! Configuration management for icetable
//!
//! This module contains all configuration-related logic:
//! - Application config (load/save from ~/.config/icetable/)
//! - Catalog configurations (REST, auth methods)
//! - Credential management (tokens, OAuth2, IAM)
//! - Catalog credentials (separate credentials.yaml file)
//! - Auth service (login, logout, status operations)
//! - Table resolution (aliases, paths, catalog references)

mod auth_service;
mod catalog;
mod catalog_credentials;
mod credentials;
mod manager;
mod resolver;

pub use auth_service::{AuthService, AuthStatus, LoginResult, LogoutResult};
pub use catalog::{CatalogAuth, CatalogConfig, CatalogProvider, CatalogType};
pub use catalog_credentials::{CatalogCredentials, OAuth2LoginRequest};
pub use credentials::CredentialSource;
pub use manager::Config;
pub use resolver::{ResolvePath, ResolveTableRef, ResolvedTable};

/// Check if a string looks like a direct path
pub fn is_direct_path(s: &str) -> bool {
    // Cloud/remote paths
    s.starts_with("s3://")
        || s.starts_with("s3a://")
        || s.starts_with("gs://")
        || s.starts_with("gcs://")
        || s.starts_with("abfs://")
        || s.starts_with("abfss://")
        || s.starts_with("file://")
        // Absolute paths
        || s.starts_with('/')
        // Relative paths (contains path separator or starts with ./)
        || s.contains(std::path::MAIN_SEPARATOR)
        || s.starts_with("./")
        || s.starts_with("../")
}
