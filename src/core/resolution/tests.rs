//! Tests for resolution module

use super::*;

// =========================================================================
// CatalogContext Tests
// =========================================================================

#[test]
fn test_catalog_context_table_ref_with_namespace_and_table() {
    let ctx = CatalogContext {
        namespace: Some("my_ns".to_string()),
        table: Some("my_table".to_string()),
        ..Default::default()
    };
    assert_eq!(ctx.table_ref(), Some("my_ns.my_table".to_string()));
}

#[test]
fn test_catalog_context_table_ref_table_only() {
    let ctx = CatalogContext {
        namespace: None,
        table: Some("my_table".to_string()),
        ..Default::default()
    };
    assert_eq!(ctx.table_ref(), Some("my_table".to_string()));
}

#[test]
fn test_catalog_context_table_ref_none() {
    let ctx = CatalogContext::default();
    assert_eq!(ctx.table_ref(), None);
}

#[test]
fn test_catalog_context_table_ref_already_qualified() {
    // Table already has namespace, should not add another
    let ctx = CatalogContext {
        namespace: Some("ns1".to_string()),
        table: Some("ns2.my_table".to_string()),
        ..Default::default()
    };
    assert_eq!(ctx.table_ref(), Some("ns2.my_table".to_string()));
}

#[test]
fn test_catalog_context_table_ref_s3_path() {
    // S3 paths should not get namespace prepended
    let ctx = CatalogContext {
        namespace: Some("ns".to_string()),
        table: Some("s3://bucket/table".to_string()),
        ..Default::default()
    };
    assert_eq!(ctx.table_ref(), Some("s3://bucket/table".to_string()));
}

#[test]
fn test_catalog_context_table_ref_gs_path() {
    // GCS paths should not get namespace prepended
    let ctx = CatalogContext {
        namespace: Some("ns".to_string()),
        table: Some("gs://bucket/table".to_string()),
        ..Default::default()
    };
    assert_eq!(ctx.table_ref(), Some("gs://bucket/table".to_string()));
}

#[test]
fn test_catalog_context_table_ref_absolute_path() {
    // Absolute paths should not get namespace prepended
    let ctx = CatalogContext {
        namespace: Some("ns".to_string()),
        table: Some("/path/to/table".to_string()),
        ..Default::default()
    };
    assert_eq!(ctx.table_ref(), Some("/path/to/table".to_string()));
}

#[test]
fn test_catalog_context_default() {
    let ctx = CatalogContext::default();
    assert!(ctx.table.is_none());
    assert!(ctx.namespace.is_none());
    assert!(ctx.catalog.is_none());
    assert!(ctx.warehouse.is_none());
    assert!(ctx.catalog_config.is_none());
}

// =========================================================================
// TableResolution Tests
// =========================================================================

#[test]
fn test_table_resolution_path_location() {
    let resolution = TableResolution::Path("s3://bucket/table".to_string());
    assert_eq!(resolution.location(), "s3://bucket/table");
}

#[test]
fn test_table_resolution_path_is_catalog() {
    let resolution = TableResolution::Path("s3://bucket/table".to_string());
    assert!(!resolution.is_catalog());
}

#[test]
fn test_table_resolution_path_as_path() {
    let resolution = TableResolution::Path("s3://bucket/table".to_string());
    assert_eq!(resolution.as_path(), Some("s3://bucket/table"));
}

#[test]
fn test_table_resolution_path_as_catalog_table() {
    let resolution = TableResolution::Path("s3://bucket/table".to_string());
    assert!(resolution.as_catalog_table().is_none());
}

// =========================================================================
// Error Helper Tests
// =========================================================================

#[test]
fn test_no_catalog_error() {
    use crate::error::Error;
    let err = no_catalog_error();
    assert!(matches!(err, Error::NoCatalog));
}

#[test]
fn test_no_namespace_error() {
    use crate::error::Error;
    let err = no_namespace_error();
    assert!(matches!(err, Error::NoNamespace));
}

#[test]
fn test_no_table_error() {
    use crate::error::Error;
    let err = no_table_error();
    assert!(matches!(err, Error::NoTable));
}
