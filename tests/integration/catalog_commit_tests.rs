//! Integration tests for REST catalog commit functionality
//!
//! These tests verify the TableCommitter behavior for catalog-aware operations.
//! Note: Full integration tests with a live REST catalog (Nessie/Polaris) should
//! be run separately with the catalog available.

use icetable::core::catalog::{CatalogAuth, CatalogConfig, TableCommitter};
use icetable::core::config::CredentialSource;

/// Test that TableCommitter can be created for direct mode (no catalog)
#[test]
fn test_committer_direct_mode() {
    let committer = TableCommitter::direct();
    assert!(
        !committer.uses_catalog(),
        "Direct committer should not use catalog"
    );
    assert!(
        committer.table_ident().is_none(),
        "Direct committer should have no table ident"
    );
    assert!(
        committer.catalog_config().is_none(),
        "Direct committer should have no config"
    );
}

/// Test that TableCommitter can be created for catalog mode
#[test]
fn test_committer_catalog_mode() {
    let config = CatalogConfig::rest("http://localhost:19120/api/v2");

    let committer = TableCommitter::with_catalog(
        config.clone(),
        vec!["analytics".to_string()],
        "orders".to_string(),
    )
    .expect("Failed to create committer");

    assert!(
        committer.uses_catalog(),
        "Catalog committer should use catalog"
    );
    assert!(
        committer.table_ident().is_some(),
        "Catalog committer should have table ident"
    );
    assert!(
        committer.catalog_config().is_some(),
        "Catalog committer should have config"
    );

    let ident = committer.table_ident().unwrap();
    assert_eq!(ident.name(), "orders");
}

/// Test that TableCommitter handles multi-level namespaces
#[test]
fn test_committer_multi_level_namespace() {
    let config = CatalogConfig::rest("http://localhost:19120/api/v2");

    let committer = TableCommitter::with_catalog(
        config,
        vec![
            "prod".to_string(),
            "analytics".to_string(),
            "data".to_string(),
        ],
        "events".to_string(),
    )
    .expect("Failed to create committer");

    let ident = committer.table_ident().unwrap();
    assert_eq!(ident.name(), "events");
    assert_eq!(ident.namespace().as_ref(), &["prod", "analytics", "data"]);
}

/// Test catalog config with bearer auth
#[test]
fn test_catalog_config_with_bearer_auth() {
    let config = CatalogConfig::rest("http://localhost:19120/api/v2").with_auth(
        CatalogAuth::bearer(CredentialSource::Inline("user:password".to_string())),
    );

    assert_eq!(config.uri, "http://localhost:19120/api/v2");
    assert!(matches!(config.auth, CatalogAuth::Bearer { .. }));
}

/// Test catalog config with bearer token
#[test]
fn test_catalog_config_with_bearer_token() {
    let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.test";
    let config = CatalogConfig::rest("http://localhost:19120/api/v2").with_auth(
        CatalogAuth::bearer(CredentialSource::Inline(token.to_string())),
    );

    // Check auth is set correctly
    let props = config.to_catalog_properties().unwrap();
    assert_eq!(props.get("token"), Some(&token.to_string()));
}

/// Test single-level namespace
#[test]
fn test_committer_single_level_namespace() {
    let config = CatalogConfig::rest("http://localhost:19120/api/v2");

    let committer =
        TableCommitter::with_catalog(config, vec!["default".to_string()], "my_table".to_string())
            .expect("Failed to create committer");

    let ident = committer.table_ident().unwrap();
    assert_eq!(ident.name(), "my_table");
    assert_eq!(ident.namespace().as_ref(), &["default"]);
}

/// Test committer config is accessible
#[test]
fn test_committer_config_accessible() {
    let config = CatalogConfig::rest("http://nessie:19120/api/v2")
        .with_auth(CatalogAuth::bearer(CredentialSource::Inline(
            "admin:secret".to_string(),
        )))
        .with_warehouse("s3://lakehouse/warehouse");

    let committer = TableCommitter::with_catalog(
        config.clone(),
        vec!["prod".to_string()],
        "events".to_string(),
    )
    .expect("Failed to create committer");

    let retrieved_config = committer.catalog_config().unwrap();
    assert_eq!(retrieved_config.uri, config.uri);
    assert!(matches!(retrieved_config.auth, CatalogAuth::Bearer { .. }));
    assert_eq!(retrieved_config.warehouse, config.warehouse);
}
