//! Unified reference (branch/tag) command implementation
//!
//! Manages branches and tags for Iceberg tables using shared code.

use super::common::resolve_table_from_context;
use crate::cli::output::{FormatterRefInfo, RefsFormatter};
use crate::cli::parser::CatalogContext;
use crate::core::maintenance::{BranchRetention, RefConfig, RefService};
use crate::core::metadata::IcebergMetadataService;
use crate::error::Result;

/// Reference type (branch or tag)
#[derive(Clone, Copy)]
pub enum RefType {
    /// A branch reference (mutable, can be updated)
    Branch,
    /// A tag reference (immutable snapshot marker)
    Tag,
}

impl RefType {
    /// Returns the capitalized name of the reference type
    pub fn name(&self) -> &'static str {
        match self {
            RefType::Branch => "Branch",
            RefType::Tag => "Tag",
        }
    }

    /// Returns the lowercase name of the reference type
    pub fn name_lower(&self) -> &'static str {
        match self {
            RefType::Branch => "branch",
            RefType::Tag => "tag",
        }
    }

    /// Returns the filter string used to match refs of this type
    pub fn filter_type(&self) -> &'static str {
        self.name_lower()
    }
}

/// Shared reference operations
pub struct RefCommands;

impl RefCommands {
    /// List references of the given type
    pub async fn list(
        metadata_service: &IcebergMetadataService,
        ref_type: RefType,
        output: &str,
        show_current: bool,
    ) -> Result<()> {
        let refs = metadata_service.list_refs().await?;

        let current_snapshot_id = if show_current {
            let (metadata, _) = metadata_service.load_metadata().await?;
            metadata.current_snapshot_id()
        } else {
            None
        };

        // Filter by ref type and convert to formatter types
        let filtered: Vec<FormatterRefInfo> = refs
            .iter()
            .filter(|r| r.ref_type == ref_type.filter_type())
            .map(|r| FormatterRefInfo::new(r.name.clone(), r.snapshot_id, r.ref_type.clone()))
            .collect();

        if output == "json" {
            let json_str =
                RefsFormatter::format_list_json(&filtered, show_current, current_snapshot_id)
                    .map_err(|e| crate::error::Error::Serialization {
                        message: e.to_string(),
                    })?;
            println!("{}", json_str);
        } else {
            let table_str = RefsFormatter::format_list_table(
                &filtered,
                ref_type.name(),
                show_current,
                current_snapshot_id,
            );
            println!("{}", table_str);
        }

        Ok(())
    }

    /// Create a branch
    pub async fn create_branch(
        metadata_service: &IcebergMetadataService,
        name: &str,
        from_snapshot: Option<i64>,
        retention: BranchRetention,
        output: &str,
    ) -> Result<()> {
        let ref_service = RefService::new();
        let result = ref_service
            .create_branch(metadata_service, name, from_snapshot, retention)
            .await?;

        Self::print_create_result(
            RefType::Branch,
            &result.name,
            result.snapshot_id,
            result.new_version,
            output,
        )
    }

    /// Create a tag
    pub async fn create_tag(
        metadata_service: &IcebergMetadataService,
        name: &str,
        snapshot_id: Option<i64>,
        max_ref_age_ms: Option<i64>,
        output: &str,
    ) -> Result<()> {
        let ref_service = RefService::new();
        let result = ref_service
            .create_tag(metadata_service, name, snapshot_id, max_ref_age_ms)
            .await?;

        Self::print_create_result(
            RefType::Tag,
            &result.name,
            result.snapshot_id,
            result.new_version,
            output,
        )
    }

    /// Delete a reference (branch or tag)
    pub async fn delete(
        metadata_service: &IcebergMetadataService,
        ref_type: RefType,
        name: &str,
        dry_run: bool,
        output: &str,
    ) -> Result<()> {
        let config = RefConfig { dry_run };
        let ref_service = RefService::with_config(config);
        let result = ref_service.delete_ref(metadata_service, name).await?;

        if output == "json" {
            let json_str = RefsFormatter::format_delete_json(
                &result.name,
                result.snapshot_id,
                result.new_version,
                result.dry_run,
            )
            .map_err(|e| crate::error::Error::Serialization {
                message: e.to_string(),
            })?;
            println!("{}", json_str);
        } else if result.dry_run {
            println!(
                "{}",
                RefsFormatter::format_delete_dry_run(
                    ref_type.name(),
                    &result.name,
                    result.snapshot_id
                )
            );
        } else {
            println!(
                "{}",
                RefsFormatter::format_delete_table(
                    ref_type.name(),
                    &result.name,
                    result.new_version
                )
            );
        }

        Ok(())
    }

    /// Rename a branch
    pub async fn rename_branch(
        metadata_service: &IcebergMetadataService,
        old_name: &str,
        new_name: &str,
        output: &str,
    ) -> Result<()> {
        let ref_service = RefService::new();
        let result = ref_service
            .rename_branch(metadata_service, old_name, new_name)
            .await?;

        Self::print_rename_result(
            RefType::Branch,
            old_name,
            &result.name,
            result.snapshot_id,
            result.new_version,
            output,
        )
    }

    /// Rename a tag
    pub async fn rename_tag(
        metadata_service: &IcebergMetadataService,
        old_name: &str,
        new_name: &str,
        output: &str,
    ) -> Result<()> {
        let ref_service = RefService::new();
        let result = ref_service
            .rename_tag(metadata_service, old_name, new_name)
            .await?;

        Self::print_rename_result(
            RefType::Tag,
            old_name,
            &result.name,
            result.snapshot_id,
            result.new_version,
            output,
        )
    }

    /// Fast-forward a branch (branch-only operation)
    pub async fn fast_forward_branch(
        metadata_service: &IcebergMetadataService,
        name: &str,
        to: &str,
        output: &str,
    ) -> Result<()> {
        let ref_service = RefService::new();
        let result = ref_service
            .fast_forward_branch(metadata_service, name, to)
            .await?;

        if output == "json" {
            let json_str = RefsFormatter::format_fast_forward_json(
                &result.name,
                result.snapshot_id,
                result.new_version,
            )
            .map_err(|e| crate::error::Error::Serialization {
                message: e.to_string(),
            })?;
            println!("{}", json_str);
        } else {
            println!(
                "{}",
                RefsFormatter::format_fast_forward_table(
                    &result.name,
                    result.snapshot_id,
                    result.new_version
                )
            );
        }

        Ok(())
    }

    // Helper: print create result
    fn print_create_result(
        ref_type: RefType,
        name: &str,
        snapshot_id: i64,
        new_version: Option<i64>,
        output: &str,
    ) -> Result<()> {
        if output == "json" {
            let json_str = RefsFormatter::format_create_json(name, snapshot_id, new_version)
                .map_err(|e| crate::error::Error::Serialization {
                    message: e.to_string(),
                })?;
            println!("{}", json_str);
        } else {
            println!(
                "{}",
                RefsFormatter::format_create_table(ref_type.name(), name, snapshot_id, new_version)
            );
        }
        Ok(())
    }

    // Helper: print rename result
    fn print_rename_result(
        ref_type: RefType,
        old_name: &str,
        new_name: &str,
        _snapshot_id: i64,
        new_version: Option<i64>,
        output: &str,
    ) -> Result<()> {
        if output == "json" {
            let json_str =
                RefsFormatter::format_rename_json(old_name, new_name, _snapshot_id, new_version)
                    .map_err(|e| crate::error::Error::Serialization {
                        message: e.to_string(),
                    })?;
            println!("{}", json_str);
        } else {
            println!(
                "{}",
                RefsFormatter::format_rename_table(
                    ref_type.name(),
                    old_name,
                    new_name,
                    new_version
                )
            );
        }
        Ok(())
    }
}

// ============================================================================
// Branch Command (thin wrapper)
// ============================================================================

use crate::cli::parser::{BranchArgs, BranchCommands};
use crate::utils::with_resource_limits;

/// Handler for branch command
pub struct BranchCommand;

impl BranchCommand {
    /// Execute the branch command with the given arguments
    pub async fn execute(args: BranchArgs, ctx: &CatalogContext) -> Result<()> {
        use super::constants::MEMORY_LIGHT_OPS;
        with_resource_limits(MEMORY_LIGHT_OPS, Self::execute_inner(args, ctx)).await
    }

    async fn execute_inner(args: BranchArgs, ctx: &CatalogContext) -> Result<()> {
        let resolution = resolve_table_from_context(ctx).await?;

        match args.command {
            BranchCommands::Ls(a) => {
                let metadata_service = resolution.to_readonly_service().await?;
                RefCommands::list(&metadata_service, RefType::Branch, &a.output, true).await
            }
            BranchCommands::Create(a) => {
                let metadata_service = resolution
                    .to_writable_service(ctx.catalog_config.as_ref(), None)
                    .await?;
                let retention = BranchRetention {
                    min_snapshots_to_keep: a.min_snapshots_to_keep,
                    max_snapshot_age_ms: a.max_snapshot_age_ms,
                    max_ref_age_ms: a.max_ref_age_ms,
                };
                RefCommands::create_branch(
                    &metadata_service,
                    &a.name,
                    a.from_snapshot,
                    retention,
                    &a.output,
                )
                .await
            }
            BranchCommands::Delete(a) => {
                let metadata_service = resolution
                    .to_writable_service(ctx.catalog_config.as_ref(), None)
                    .await?;
                RefCommands::delete(
                    &metadata_service,
                    RefType::Branch,
                    &a.name,
                    a.dry_run,
                    &a.output,
                )
                .await
            }
            BranchCommands::FastForward(a) => {
                let metadata_service = resolution
                    .to_writable_service(ctx.catalog_config.as_ref(), None)
                    .await?;
                RefCommands::fast_forward_branch(&metadata_service, &a.name, &a.to, &a.output).await
            }
            BranchCommands::Rename(a) => {
                let metadata_service = resolution
                    .to_writable_service(ctx.catalog_config.as_ref(), None)
                    .await?;
                RefCommands::rename_branch(&metadata_service, &a.old_name, &a.new_name, &a.output)
                    .await
            }
        }
    }
}

// ============================================================================
// Tag Command (thin wrapper)
// ============================================================================

use crate::cli::parser::{TagArgs, TagCommands};

/// Handler for tag command
pub struct TagCommand;

impl TagCommand {
    /// Execute the tag command with the given arguments
    pub async fn execute(args: TagArgs, ctx: &CatalogContext) -> Result<()> {
        use super::constants::MEMORY_LIGHT_OPS;
        with_resource_limits(MEMORY_LIGHT_OPS, Self::execute_inner(args, ctx)).await
    }

    async fn execute_inner(args: TagArgs, ctx: &CatalogContext) -> Result<()> {
        let resolution = resolve_table_from_context(ctx).await?;

        match args.command {
            TagCommands::Ls(a) => {
                let metadata_service = resolution.to_readonly_service().await?;
                RefCommands::list(&metadata_service, RefType::Tag, &a.output, false).await
            }
            TagCommands::Create(a) => {
                let metadata_service = resolution
                    .to_writable_service(ctx.catalog_config.as_ref(), None)
                    .await?;
                RefCommands::create_tag(
                    &metadata_service,
                    &a.name,
                    a.snapshot_id,
                    a.max_ref_age_ms,
                    &a.output,
                )
                .await
            }
            TagCommands::Delete(a) => {
                let metadata_service = resolution
                    .to_writable_service(ctx.catalog_config.as_ref(), None)
                    .await?;
                RefCommands::delete(
                    &metadata_service,
                    RefType::Tag,
                    &a.name,
                    a.dry_run,
                    &a.output,
                )
                .await
            }
            TagCommands::Rename(a) => {
                let metadata_service = resolution
                    .to_writable_service(ctx.catalog_config.as_ref(), None)
                    .await?;
                RefCommands::rename_tag(&metadata_service, &a.old_name, &a.new_name, &a.output)
                    .await
            }
        }
    }
}
