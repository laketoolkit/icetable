//! Authentication service for catalog credentials management
//!
//! Provides business logic for login, logout, and status operations.
//! This service coordinates between Config and CatalogCredentials.

use std::path::PathBuf;

use super::{CatalogAuth, CatalogCredentials, Config};
use crate::error::{Error, Result};

// =============================================================================
// Result Types
// =============================================================================

/// Result of a successful login operation
#[derive(Debug)]
pub struct LoginResult {
    /// Name of the catalog that was logged into
    pub catalog_name: String,
    /// Type of authentication used (e.g., "oauth2", "bearer")
    pub auth_type: &'static str,
    /// Path where credentials were saved
    pub credentials_path: PathBuf,
}

/// Result of a logout operation
#[derive(Debug)]
pub enum LogoutResult {
    /// Successfully logged out from a single catalog
    Single {
        /// Name of the catalog
        catalog_name: String,
    },
    /// Successfully logged out from all catalogs
    All {
        /// Number of catalogs that were logged out
        count: usize,
    },
    /// No credentials found for the catalog
    NotFound {
        /// Name of the catalog
        catalog_name: String,
    },
}

/// Authentication status for a catalog
#[derive(Debug)]
pub struct AuthStatus {
    /// Name of the catalog
    pub catalog_name: String,
    /// Whether the catalog exists in config.yaml
    pub catalog_exists: bool,
    /// Whether credentials are stored for this catalog
    pub has_credentials: bool,
    /// The stored authentication configuration (if any)
    pub auth: Option<CatalogAuth>,
}

// =============================================================================
// AuthService
// =============================================================================

/// Service for managing catalog authentication
///
/// Coordinates between Config (config.yaml) and CatalogCredentials (credentials.yaml)
/// to provide login, logout, and status operations.
pub struct AuthService {
    config: Config,
    credentials: CatalogCredentials,
}

impl AuthService {
    /// Create a new auth service, loading config and credentials
    pub fn new() -> Result<Self> {
        Ok(Self {
            config: Config::load()?,
            credentials: CatalogCredentials::load()?,
        })
    }

    /// Resolve catalog name from optional override or current context
    ///
    /// Priority: explicit name > current catalog from config
    pub fn resolve_catalog_name(&self, override_name: Option<&str>) -> Result<String> {
        override_name
            .map(String::from)
            .or_else(|| self.config.get_current_catalog().map(String::from))
            .ok_or(Error::NoCatalog)
    }

    /// Check if a catalog exists in the configuration
    pub fn catalog_exists(&self, catalog_name: &str) -> bool {
        self.config.catalogs.contains_key(catalog_name)
    }

    /// Login to a catalog (store credentials)
    ///
    /// Validates that the catalog exists in config before storing credentials.
    pub fn login(&mut self, catalog_name: &str, auth: CatalogAuth) -> Result<LoginResult> {
        // Validate catalog exists in config
        if !self.config.catalogs.contains_key(catalog_name) {
            return Err(Error::CatalogNotFound {
                name: catalog_name.to_string(),
            });
        }

        let auth_type = auth.describe();
        self.credentials.set(catalog_name, auth);
        self.credentials.save()?;

        Ok(LoginResult {
            catalog_name: catalog_name.to_string(),
            auth_type,
            credentials_path: CatalogCredentials::credentials_path()?,
        })
    }

    /// Logout from a catalog or all catalogs
    ///
    /// If `all` is true, removes credentials for all catalogs.
    /// Otherwise removes credentials for the specified catalog.
    pub fn logout(&mut self, catalog_name: Option<&str>, all: bool) -> Result<LogoutResult> {
        if all {
            let count = self.credentials.catalogs.len();
            self.credentials.catalogs.clear();
            self.credentials.save()?;
            return Ok(LogoutResult::All { count });
        }

        let name = self.resolve_catalog_name(catalog_name)?;

        if self.credentials.remove(&name) {
            self.credentials.save()?;
            Ok(LogoutResult::Single { catalog_name: name })
        } else {
            Ok(LogoutResult::NotFound { catalog_name: name })
        }
    }

    /// Get authentication status for a specific catalog
    pub fn status(&self, catalog_name: &str) -> AuthStatus {
        AuthStatus {
            catalog_name: catalog_name.to_string(),
            catalog_exists: self.config.catalogs.contains_key(catalog_name),
            has_credentials: self.credentials.has(catalog_name),
            auth: self.credentials.get(catalog_name).cloned(),
        }
    }

    /// Get authentication status for all catalogs with stored credentials
    pub fn status_all(&self) -> Vec<AuthStatus> {
        self.credentials
            .catalogs
            .iter()
            .map(|(name, auth)| AuthStatus {
                catalog_name: name.clone(),
                catalog_exists: self.config.catalogs.contains_key(name),
                has_credentials: true,
                auth: Some(auth.clone()),
            })
            .collect()
    }

    /// Check if any credentials are stored
    pub fn has_any_credentials(&self) -> bool {
        !self.credentials.catalogs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logout_result_variants() {
        // Just verify the enum variants compile correctly
        let _single = LogoutResult::Single {
            catalog_name: "test".to_string(),
        };
        let _all = LogoutResult::All { count: 5 };
        let _not_found = LogoutResult::NotFound {
            catalog_name: "test".to_string(),
        };
    }

    #[test]
    fn test_auth_status_fields() {
        let status = AuthStatus {
            catalog_name: "polaris".to_string(),
            catalog_exists: true,
            has_credentials: true,
            auth: Some(CatalogAuth::None),
        };

        assert_eq!(status.catalog_name, "polaris");
        assert!(status.catalog_exists);
        assert!(status.has_credentials);
    }
}
