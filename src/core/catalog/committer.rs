//! Catalog-aware committer for Iceberg table updates
//!
//! This module provides a unified interface for committing table updates,
//! whether through a REST catalog (for multi-writer safety) or directly
//! to storage (single-writer mode).
//!
//! When a catalog is configured, commits go through the catalog's REST API
//! which provides:
//! - Atomic commits with conflict detection
//! - Automatic retries on conflicts (configurable, default 3 attempts)
//! - Multi-writer safety
//!
//! When no catalog is configured, commits write directly to storage,
//! which is suitable for single-writer scenarios.
//!
//! Note: The iceberg-rs Transaction API (v0.7.0) doesn't yet support
//! expire_snapshots or set_snapshot_ref actions, so we use HTTP calls
//! directly to the REST catalog API for these operations.

use std::time::Duration;

use iceberg::spec::{Snapshot, SnapshotReference, SnapshotRetention, TableMetadata, MAIN_BRANCH};
use iceberg::{NamespaceIdent, TableIdent, TableRequirement, TableUpdate};
use serde::{Deserialize, Serialize};

use super::CatalogConfig;
use crate::core::storage::traits::PutOptions;
use crate::core::storage::StorageBackendFactory;
use crate::error::{Error, Result};

/// Request body for committing table updates via REST API
#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct CommitTableRequest {
    identifier: TableIdentifier,
    requirements: Vec<TableRequirement>,
    updates: Vec<TableUpdate>,
}

/// Table identifier for REST API
#[derive(Debug, Serialize)]
struct TableIdentifier {
    namespace: Vec<String>,
    name: String,
}

/// Response from commit table request
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
struct CommitTableResponse {
    #[serde(default)]
    metadata_location: Option<String>,
}

/// Default number of retry attempts for conflict errors
const DEFAULT_MAX_RETRIES: u32 = 3;

/// Base delay between retries (with exponential backoff)
const RETRY_BASE_DELAY_MS: u64 = 100;

/// A committer that can use either catalog transactions or direct storage writes
pub struct TableCommitter {
    /// REST catalog configuration (if available)
    catalog_config: Option<CatalogConfig>,
    /// Table identifier (for catalog mode)
    table_ident: Option<TableIdent>,
    /// HTTP client for REST catalog calls
    http_client: reqwest::Client,
    /// Maximum retry attempts for conflicts
    max_retries: u32,
}

impl TableCommitter {
    /// Create a committer that writes directly to storage (single-writer mode)
    pub fn direct() -> Self {
        Self {
            catalog_config: None,
            table_ident: None,
            http_client: reqwest::Client::new(),
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }

    /// Create a committer that uses a REST catalog for commits (multi-writer safe)
    pub fn with_catalog(config: CatalogConfig, namespace: Vec<String>, table_name: String) -> Self {
        let ns_ident = NamespaceIdent::from_vec(namespace).expect("Invalid namespace");
        let table_ident = TableIdent::new(ns_ident, table_name);

        Self {
            catalog_config: Some(config),
            table_ident: Some(table_ident),
            http_client: reqwest::Client::new(),
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }

    /// Check if this committer uses a catalog
    pub fn uses_catalog(&self) -> bool {
        self.catalog_config.is_some()
    }

    /// Get the table identifier (namespace and name) if using catalog mode
    pub fn table_ident(&self) -> Option<&TableIdent> {
        self.table_ident.as_ref()
    }

    /// Get the catalog config if using catalog mode
    pub fn catalog_config(&self) -> Option<&CatalogConfig> {
        self.catalog_config.as_ref()
    }

    /// Commit snapshot removal (expire snapshots)
    ///
    /// When using catalog: Uses REST API with TableUpdate::RemoveSnapshots
    /// When direct: Writes new metadata to storage
    pub async fn commit_remove_snapshots(
        &self,
        table_path: &str,
        current_metadata: &TableMetadata,
        snapshot_ids: &[i64],
        current_version: i32,
    ) -> Result<i64> {
        if let (Some(config), Some(ident)) = (&self.catalog_config, &self.table_ident) {
            // Catalog mode: use REST API
            let updates = vec![TableUpdate::RemoveSnapshots {
                snapshot_ids: snapshot_ids.to_vec(),
            }];

            let requirements = vec![
                TableRequirement::UuidMatch {
                    uuid: current_metadata.uuid(),
                },
                TableRequirement::RefSnapshotIdMatch {
                    r#ref: MAIN_BRANCH.to_string(),
                    snapshot_id: current_metadata.current_snapshot_id(),
                },
            ];

            self.commit_via_rest(config, ident, updates, requirements)
                .await?;

            // Return incremented version; actual version is managed by catalog
            Ok(current_version as i64 + 1)
        } else {
            // Direct mode: build and write new metadata
            self.write_metadata_direct(table_path, current_metadata, snapshot_ids, current_version)
                .await
        }
    }

    /// Commit setting a new current snapshot (time-travel / rollback)
    ///
    /// When using catalog: Uses REST API with TableUpdate::SetSnapshotRef
    /// When direct: Writes new metadata to storage
    pub async fn commit_set_snapshot_ref(
        &self,
        table_path: &str,
        current_metadata: &TableMetadata,
        ref_name: &str,
        snapshot_id: i64,
        current_version: i32,
    ) -> Result<i64> {
        if let (Some(config), Some(ident)) = (&self.catalog_config, &self.table_ident) {
            // Catalog mode: use REST API
            let reference = SnapshotReference {
                snapshot_id,
                retention: SnapshotRetention::Branch {
                    min_snapshots_to_keep: None,
                    max_snapshot_age_ms: None,
                    max_ref_age_ms: None,
                },
            };

            let updates = vec![TableUpdate::SetSnapshotRef {
                ref_name: ref_name.to_string(),
                reference,
            }];

            let requirements = vec![TableRequirement::UuidMatch {
                uuid: current_metadata.uuid(),
            }];

            self.commit_via_rest(config, ident, updates, requirements)
                .await?;

            Ok(current_version as i64 + 1)
        } else {
            // Direct mode: build and write new metadata
            self.write_set_ref_direct(
                table_path,
                current_metadata,
                ref_name,
                snapshot_id,
                current_version,
            )
            .await
        }
    }

    /// Commit updates via REST catalog API with automatic retry on conflicts
    async fn commit_via_rest(
        &self,
        config: &CatalogConfig,
        ident: &TableIdent,
        updates: Vec<TableUpdate>,
        requirements: Vec<TableRequirement>,
    ) -> Result<()> {
        let namespace: Vec<String> = ident.namespace().as_ref().to_vec();
        let table_name = ident.name().to_string();

        // Build the endpoint URL
        // REST catalog spec: POST /v1/{prefix}/namespaces/{namespace}/tables/{table}
        let namespace_path = namespace.join("%1F"); // Use unit separator for multi-level namespaces
        let endpoint = format!(
            "{}/v1/namespaces/{}/tables/{}",
            config.uri.trim_end_matches('/'),
            namespace_path,
            table_name
        );

        let request_body = CommitTableRequest {
            identifier: TableIdentifier {
                namespace,
                name: table_name,
            },
            requirements,
            updates,
        };

        // Retry loop with exponential backoff
        let mut attempt = 0;
        loop {
            attempt += 1;

            let mut request = self.http_client.post(&endpoint).json(&request_body);

            // Add credential if configured
            if let Some(credential) = config.resolve_credential()? {
                // Basic auth or bearer token
                if credential.contains(':') {
                    let parts: Vec<&str> = credential.splitn(2, ':').collect();
                    request = request.basic_auth(parts[0], Some(parts[1]));
                } else {
                    request = request.bearer_auth(&credential);
                }
            }

            let response = request
                .send()
                .await
                .map_err(|e| Error::General(format!("Failed to send commit request: {}", e)))?;

            match response.status() {
                reqwest::StatusCode::OK => return Ok(()),
                reqwest::StatusCode::CONFLICT => {
                    if attempt >= self.max_retries {
                        return Err(Error::General(format!(
                            "Commit conflict: table was modified by another writer. \
                             Exhausted {} retry attempts.",
                            self.max_retries
                        )));
                    }
                    // Exponential backoff: 100ms, 200ms, 400ms...
                    let delay = Duration::from_millis(RETRY_BASE_DELAY_MS * (1 << (attempt - 1)));
                    tokio::time::sleep(delay).await;
                    continue;
                }
                reqwest::StatusCode::NOT_FOUND => {
                    return Err(Error::General(format!(
                        "Table not found: {}",
                        ident.name()
                    )));
                }
                status => {
                    let body = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "unknown".to_string());
                    return Err(Error::General(format!(
                        "Catalog commit failed ({}): {}",
                        status, body
                    )));
                }
            }
        }
    }

    /// Write new metadata directly to storage (single-writer mode)
    async fn write_metadata_direct(
        &self,
        table_path: &str,
        current_metadata: &TableMetadata,
        snapshot_ids: &[i64],
        _current_version: i32,
    ) -> Result<i64> {
        use crate::core::utils::{
            extract_version_from_path, find_latest_metadata, metadata_location_filename,
            new_metadata_location, next_metadata_location,
        };

        // Build new metadata with snapshots removed
        let build_result = current_metadata
            .clone()
            .into_builder(None)
            .remove_snapshots(snapshot_ids)
            .build()
            .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

        let new_metadata = build_result.metadata;

        let storage = StorageBackendFactory::create_backend(table_path).await?;
        let metadata_dir = format!("{}/metadata", table_path.trim_end_matches('/'));

        // Find current metadata to derive next version
        let current_metadata_path = find_latest_metadata(table_path, &storage).await?;

        // Generate next metadata location with standard naming
        let next_location = next_metadata_location(&current_metadata_path)
            .unwrap_or_else(|_| new_metadata_location(table_path));

        let new_version = extract_version_from_path(&next_location.to_string()).unwrap_or(0) as i64;
        let new_metadata_path = format!(
            "{}/{}",
            metadata_dir,
            metadata_location_filename(&next_location)
        );

        let new_metadata_bytes = serde_json::to_vec_pretty(&new_metadata)
            .map_err(|e| Error::General(format!("Failed to serialize metadata: {}", e)))?;

        storage
            .put(
                &new_metadata_path,
                bytes::Bytes::from(new_metadata_bytes),
                &PutOptions::default(),
            )
            .await?;

        Ok(new_version)
    }

    /// Write new metadata with updated ref directly to storage
    async fn write_set_ref_direct(
        &self,
        table_path: &str,
        current_metadata: &TableMetadata,
        ref_name: &str,
        snapshot_id: i64,
        _current_version: i32,
    ) -> Result<i64> {
        use crate::core::utils::{
            extract_version_from_path, find_latest_metadata, metadata_location_filename,
            new_metadata_location, next_metadata_location,
        };

        let branch_ref = SnapshotReference {
            snapshot_id,
            retention: SnapshotRetention::Branch {
                min_snapshots_to_keep: None,
                max_snapshot_age_ms: None,
                max_ref_age_ms: None,
            },
        };

        let build_result = current_metadata
            .clone()
            .into_builder(None)
            .set_ref(ref_name, branch_ref)
            .map_err(|e| Error::General(format!("Failed to set ref: {}", e)))?
            .build()
            .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

        let new_metadata = build_result.metadata;

        let storage = StorageBackendFactory::create_backend(table_path).await?;
        let metadata_dir = format!("{}/metadata", table_path.trim_end_matches('/'));

        let current_metadata_path = find_latest_metadata(table_path, &storage).await?;

        let next_location = next_metadata_location(&current_metadata_path)
            .unwrap_or_else(|_| new_metadata_location(table_path));

        let new_version = extract_version_from_path(&next_location.to_string()).unwrap_or(0) as i64;
        let new_metadata_path = format!(
            "{}/{}",
            metadata_dir,
            metadata_location_filename(&next_location)
        );

        let new_metadata_bytes = serde_json::to_vec_pretty(&new_metadata)
            .map_err(|e| Error::General(format!("Failed to serialize metadata: {}", e)))?;

        storage
            .put(
                &new_metadata_path,
                bytes::Bytes::from(new_metadata_bytes),
                &PutOptions::default(),
            )
            .await?;

        Ok(new_version)
    }

    /// Commit a new branch or tag reference
    ///
    /// When using catalog: Uses REST API with TableUpdate::SetSnapshotRef
    /// When direct: Writes new metadata to storage
    pub async fn commit_add_ref(
        &self,
        table_path: &str,
        current_metadata: &TableMetadata,
        ref_name: &str,
        reference: SnapshotReference,
        current_version: i32,
    ) -> Result<i64> {
        if let (Some(config), Some(ident)) = (&self.catalog_config, &self.table_ident) {
            // Catalog mode: use REST API
            let updates = vec![TableUpdate::SetSnapshotRef {
                ref_name: ref_name.to_string(),
                reference,
            }];

            let requirements = vec![TableRequirement::UuidMatch {
                uuid: current_metadata.uuid(),
            }];

            self.commit_via_rest(config, ident, updates, requirements)
                .await?;

            Ok(current_version as i64 + 1)
        } else {
            // Direct mode: build and write new metadata
            self.write_add_ref_direct(table_path, current_metadata, ref_name, reference, current_version)
                .await
        }
    }

    /// Commit removal of a branch or tag reference
    ///
    /// When using catalog: Uses REST API with TableUpdate::RemoveSnapshotRef
    /// When direct: Writes new metadata to storage
    pub async fn commit_remove_ref(
        &self,
        table_path: &str,
        current_metadata: &TableMetadata,
        ref_name: &str,
        current_version: i32,
    ) -> Result<i64> {
        if let (Some(config), Some(ident)) = (&self.catalog_config, &self.table_ident) {
            // Catalog mode: use REST API
            let updates = vec![TableUpdate::RemoveSnapshotRef {
                ref_name: ref_name.to_string(),
            }];

            let requirements = vec![TableRequirement::UuidMatch {
                uuid: current_metadata.uuid(),
            }];

            self.commit_via_rest(config, ident, updates, requirements)
                .await?;

            Ok(current_version as i64 + 1)
        } else {
            // Direct mode: build and write new metadata
            self.write_remove_ref_direct(table_path, current_metadata, ref_name, current_version)
                .await
        }
    }

    /// Write new metadata with added ref directly to storage
    async fn write_add_ref_direct(
        &self,
        table_path: &str,
        current_metadata: &TableMetadata,
        ref_name: &str,
        reference: SnapshotReference,
        _current_version: i32,
    ) -> Result<i64> {
        use crate::core::utils::{
            extract_version_from_path, find_latest_metadata, metadata_location_filename,
            new_metadata_location, next_metadata_location,
        };

        let build_result = current_metadata
            .clone()
            .into_builder(None)
            .set_ref(ref_name, reference)
            .map_err(|e| Error::General(format!("Failed to set ref: {}", e)))?
            .build()
            .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

        let new_metadata = build_result.metadata;

        let storage = StorageBackendFactory::create_backend(table_path).await?;
        let metadata_dir = format!("{}/metadata", table_path.trim_end_matches('/'));

        let current_metadata_path = find_latest_metadata(table_path, &storage).await?;

        let next_location = next_metadata_location(&current_metadata_path)
            .unwrap_or_else(|_| new_metadata_location(table_path));

        let new_version = extract_version_from_path(&next_location.to_string()).unwrap_or(0) as i64;
        let new_metadata_path = format!(
            "{}/{}",
            metadata_dir,
            metadata_location_filename(&next_location)
        );

        let new_metadata_bytes = serde_json::to_vec_pretty(&new_metadata)
            .map_err(|e| Error::General(format!("Failed to serialize metadata: {}", e)))?;

        storage
            .put(
                &new_metadata_path,
                bytes::Bytes::from(new_metadata_bytes),
                &PutOptions::default(),
            )
            .await?;

        Ok(new_version)
    }

    /// Write new metadata with removed ref directly to storage
    async fn write_remove_ref_direct(
        &self,
        table_path: &str,
        current_metadata: &TableMetadata,
        ref_name: &str,
        _current_version: i32,
    ) -> Result<i64> {
        use crate::core::utils::{
            extract_version_from_path, find_latest_metadata, metadata_location_filename,
            new_metadata_location, next_metadata_location,
        };

        let build_result = current_metadata
            .clone()
            .into_builder(None)
            .remove_ref(ref_name)
            .build()
            .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

        let new_metadata = build_result.metadata;

        let storage = StorageBackendFactory::create_backend(table_path).await?;
        let metadata_dir = format!("{}/metadata", table_path.trim_end_matches('/'));

        let current_metadata_path = find_latest_metadata(table_path, &storage).await?;

        let next_location = next_metadata_location(&current_metadata_path)
            .unwrap_or_else(|_| new_metadata_location(table_path));

        let new_version = extract_version_from_path(&next_location.to_string()).unwrap_or(0) as i64;
        let new_metadata_path = format!(
            "{}/{}",
            metadata_dir,
            metadata_location_filename(&next_location)
        );

        let new_metadata_bytes = serde_json::to_vec_pretty(&new_metadata)
            .map_err(|e| Error::General(format!("Failed to serialize metadata: {}", e)))?;

        storage
            .put(
                &new_metadata_path,
                bytes::Bytes::from(new_metadata_bytes),
                &PutOptions::default(),
            )
            .await?;

        Ok(new_version)
    }

    /// Commit renaming a reference (remove old + add new atomically)
    ///
    /// When using catalog: Uses REST API with RemoveSnapshotRef + SetSnapshotRef in one commit
    /// When direct: Writes new metadata to storage
    pub async fn commit_rename_ref(
        &self,
        table_path: &str,
        current_metadata: &TableMetadata,
        old_name: &str,
        new_name: &str,
        reference: SnapshotReference,
        current_version: i32,
    ) -> Result<i64> {
        if let (Some(config), Some(ident)) = (&self.catalog_config, &self.table_ident) {
            // Catalog mode: use REST API with both updates atomically
            let updates = vec![
                TableUpdate::RemoveSnapshotRef {
                    ref_name: old_name.to_string(),
                },
                TableUpdate::SetSnapshotRef {
                    ref_name: new_name.to_string(),
                    reference,
                },
            ];

            let requirements = vec![TableRequirement::UuidMatch {
                uuid: current_metadata.uuid(),
            }];

            self.commit_via_rest(config, ident, updates, requirements)
                .await?;

            Ok(current_version as i64 + 1)
        } else {
            // Direct mode: build and write new metadata
            self.write_rename_ref_direct(table_path, current_metadata, old_name, new_name, reference, current_version)
                .await
        }
    }

    /// Write new metadata with renamed ref directly to storage
    async fn write_rename_ref_direct(
        &self,
        table_path: &str,
        current_metadata: &TableMetadata,
        old_name: &str,
        new_name: &str,
        reference: SnapshotReference,
        _current_version: i32,
    ) -> Result<i64> {
        use crate::core::utils::{
            extract_version_from_path, find_latest_metadata, metadata_location_filename,
            new_metadata_location, next_metadata_location,
        };

        let build_result = current_metadata
            .clone()
            .into_builder(None)
            .remove_ref(old_name)
            .set_ref(new_name, reference)
            .map_err(|e| Error::General(format!("Failed to set ref: {}", e)))?
            .build()
            .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

        let new_metadata = build_result.metadata;

        let storage = StorageBackendFactory::create_backend(table_path).await?;
        let metadata_dir = format!("{}/metadata", table_path.trim_end_matches('/'));

        let current_metadata_path = find_latest_metadata(table_path, &storage).await?;

        let next_location = next_metadata_location(&current_metadata_path)
            .unwrap_or_else(|_| new_metadata_location(table_path));

        let new_version = extract_version_from_path(&next_location.to_string()).unwrap_or(0) as i64;
        let new_metadata_path = format!(
            "{}/{}",
            metadata_dir,
            metadata_location_filename(&next_location)
        );

        let new_metadata_bytes = serde_json::to_vec_pretty(&new_metadata)
            .map_err(|e| Error::General(format!("Failed to serialize metadata: {}", e)))?;

        storage
            .put(
                &new_metadata_path,
                bytes::Bytes::from(new_metadata_bytes),
                &PutOptions::default(),
            )
            .await?;

        Ok(new_version)
    }

    /// Commit a new snapshot with data file changes (used by optimize, repair, etc.)
    ///
    /// When using catalog: Uses REST API with AddSnapshot + SetSnapshotRef
    /// When direct: Returns None to indicate direct write should be used
    ///
    /// Note: For optimize/repair, the snapshot and manifest list must be created
    /// before calling this method. This method only commits the existing snapshot
    /// through the catalog.
    pub async fn commit_add_snapshot(
        &self,
        current_metadata: &TableMetadata,
        snapshot: Snapshot,
        ref_name: &str,
    ) -> Result<Option<i64>> {
        if let (Some(config), Some(ident)) = (&self.catalog_config, &self.table_ident) {
            // Catalog mode: use REST API with AddSnapshot + SetSnapshotRef
            let snapshot_id = snapshot.snapshot_id();

            let reference = SnapshotReference {
                snapshot_id,
                retention: SnapshotRetention::Branch {
                    min_snapshots_to_keep: None,
                    max_snapshot_age_ms: None,
                    max_ref_age_ms: None,
                },
            };

            let updates = vec![
                TableUpdate::AddSnapshot { snapshot },
                TableUpdate::SetSnapshotRef {
                    ref_name: ref_name.to_string(),
                    reference,
                },
            ];

            // Get the current snapshot ID for the ref
            let ref_snapshot_id = if ref_name == MAIN_BRANCH {
                current_metadata.current_snapshot_id()
            } else {
                current_metadata
                    .snapshot_for_ref(ref_name)
                    .map(|s| Some(s.snapshot_id()))
                    .unwrap_or(current_metadata.current_snapshot_id())
            };

            let requirements = vec![
                TableRequirement::UuidMatch {
                    uuid: current_metadata.uuid(),
                },
                TableRequirement::RefSnapshotIdMatch {
                    r#ref: ref_name.to_string(),
                    snapshot_id: ref_snapshot_id,
                },
            ];

            self.commit_via_rest(config, ident, updates, requirements)
                .await?;

            // Return a value indicating catalog commit was successful
            Ok(Some(1))
        } else {
            // Direct mode: return None to indicate caller should handle direct writes
            Ok(None)
        }
    }
}
