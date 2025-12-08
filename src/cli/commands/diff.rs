//! Diff command implementation
//!
//! Compares snapshots, branches, or tags within a table.

use colored::Colorize;

use crate::cli::parser::DiffArgs;
use crate::core::TableContext;
use crate::core::metadata::IcebergMetadataService;
use crate::error::{Error, Result};

/// Handler for diff command
pub struct DiffCommand;

impl DiffCommand {
    /// Execute diff command
    pub async fn execute(args: DiffArgs) -> Result<()> {
        let ctx = TableContext::from_path(args.path).await?;
        ctx.require_iceberg()?;

        let service = ctx.iceberg_service().await?;
        let (metadata, _) = service.load_metadata().await?;

        let current_id = metadata
            .current_snapshot_id()
            .ok_or_else(|| Error::General("Table has no current snapshot".to_string()))?;

        // Resolve reference (default to current)
        let ref_id = if let Some(ref reference) = args.reference {
            Self::resolve_ref(&metadata, reference)?
        } else {
            current_id
        };

        // Resolve base (default to parent of reference, or error if no base specified and no parent)
        let base_id = if let Some(ref base_ref) = args.base {
            Self::resolve_ref(&metadata, base_ref)?
        } else {
            // Try to get parent snapshot
            let ref_snapshot = metadata
                .snapshot_by_id(ref_id)
                .ok_or_else(|| Error::General(format!("Snapshot {} not found", ref_id)))?;
            ref_snapshot.parent_snapshot_id().ok_or_else(|| {
                Error::General(
                    "No parent snapshot. Use --base to specify a base reference.".to_string(),
                )
            })?
        };

        if ref_id == base_id {
            if args.output == "json" {
                let json = serde_json::json!({
                    "reference": ref_id,
                    "base": base_id,
                    "identical": true,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json)
                        .map_err(|e| Error::General(e.to_string()))?
                );
            } else {
                println!("{}", "References point to the same snapshot".yellow());
            }
            return Ok(());
        }

        // Get snapshots
        let base_snapshot = metadata
            .snapshot_by_id(base_id)
            .ok_or_else(|| Error::General(format!("Snapshot {} not found", base_id)))?;
        let ref_snapshot = metadata
            .snapshot_by_id(ref_id)
            .ok_or_else(|| Error::General(format!("Snapshot {} not found", ref_id)))?;

        // Get manifest files for both
        let base_manifests = Self::get_manifest_files(&service, base_snapshot).await?;
        let ref_manifests = Self::get_manifest_files(&service, ref_snapshot).await?;

        // Calculate diff (what changed from base to ref)
        let added: Vec<_> = ref_manifests
            .iter()
            .filter(|m| !base_manifests.contains(m))
            .collect();
        let removed: Vec<_> = base_manifests
            .iter()
            .filter(|m| !ref_manifests.contains(m))
            .collect();

        // Format timestamps
        let base_ts = chrono::DateTime::from_timestamp_millis(base_snapshot.timestamp_ms())
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| base_snapshot.timestamp_ms().to_string());
        let ref_ts = chrono::DateTime::from_timestamp_millis(ref_snapshot.timestamp_ms())
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| ref_snapshot.timestamp_ms().to_string());

        // Labels for display
        let ref_label = args.reference.as_deref().unwrap_or("current");
        let base_label = args.base.as_deref().unwrap_or("parent");

        if args.output == "json" {
            let json = serde_json::json!({
                "base": {
                    "ref": base_label,
                    "snapshot_id": base_id,
                    "timestamp": base_ts,
                    "manifest_count": base_manifests.len(),
                },
                "reference": {
                    "ref": ref_label,
                    "snapshot_id": ref_id,
                    "timestamp": ref_ts,
                    "manifest_count": ref_manifests.len(),
                },
                "diff": {
                    "manifests_added": added.len(),
                    "manifests_removed": removed.len(),
                    "added_paths": added,
                    "removed_paths": removed,
                }
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!(
                "{} {} (base: {})",
                "Comparing".green(),
                ref_label,
                base_label
            );
            println!();
            println!(
                "{:<20} {:<20} {:<20} {}",
                "REF".cyan(),
                "SNAPSHOT".cyan(),
                "TIMESTAMP".cyan(),
                "MANIFESTS".cyan()
            );
            println!("{}", "-".repeat(75));
            println!(
                "{:<20} {:<20} {:<20} {}",
                base_label,
                base_id,
                base_ts,
                base_manifests.len()
            );
            println!(
                "{:<20} {:<20} {:<20} {}",
                ref_label,
                ref_id,
                ref_ts,
                ref_manifests.len()
            );
            println!();

            if added.is_empty() && removed.is_empty() {
                println!("{}", "No manifest changes".yellow());
            } else {
                println!("Changes:");
                for path in &added {
                    let filename = path.rsplit('/').next().unwrap_or(path);
                    println!("  {} {}", "+".green(), filename);
                }
                for path in &removed {
                    let filename = path.rsplit('/').next().unwrap_or(path);
                    println!("  {} {}", "-".red(), filename);
                }
                println!();
                println!(
                    "Summary: {} added, {} removed",
                    added.len().to_string().green(),
                    removed.len().to_string().red()
                );
            }
        }

        Ok(())
    }

    /// Resolve a reference (snapshot ID, branch name, or tag name) to a snapshot ID
    fn resolve_ref(
        metadata: &std::sync::Arc<iceberg::spec::TableMetadata>,
        reference: &str,
    ) -> Result<i64> {
        // Try parsing as snapshot ID first
        if let Ok(id) = reference.parse::<i64>() {
            if metadata.snapshot_by_id(id).is_some() {
                return Ok(id);
            }
            // ID format but doesn't exist - still report as snapshot not found
            return Err(Error::General(format!("Snapshot {} not found", id)));
        }

        // Try as branch/tag name
        if let Some(snapshot) = metadata.snapshot_for_ref(reference) {
            return Ok(snapshot.snapshot_id());
        }

        Err(Error::General(format!(
            "Reference '{}' not found (not a valid snapshot ID, branch, or tag)",
            reference
        )))
    }

    /// Get manifest file paths from a snapshot
    async fn get_manifest_files(
        service: &IcebergMetadataService,
        snapshot: &iceberg::spec::Snapshot,
    ) -> Result<Vec<String>> {
        let file_io = service.file_io();
        let manifest_list_path = snapshot.manifest_list();

        let manifest_list_content = file_io
            .new_input(manifest_list_path)
            .map_err(|e| Error::General(format!("Failed to create input: {}", e)))?
            .read()
            .await
            .map_err(|e| Error::General(format!("Failed to read manifest list: {}", e)))?;

        let manifest_list = iceberg::spec::ManifestList::parse_with_version(
            &manifest_list_content,
            iceberg::spec::FormatVersion::V2,
        )
        .map_err(|e| Error::General(format!("Failed to parse manifest list: {}", e)))?;

        Ok(manifest_list
            .entries()
            .iter()
            .map(|e| e.manifest_path.clone())
            .collect())
    }
}
