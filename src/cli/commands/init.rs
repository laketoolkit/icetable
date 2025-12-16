//! Init command implementation
//!
//! This command creates new Iceberg tables.
//! Thin wrapper that delegates to InitService in core.

use colored::Colorize;

use crate::cli::parser::InitArgs;
use crate::core::operations::{InitConfig, InitService};
use crate::error::Result;
use crate::utils::with_resource_limits;

/// Handler for init command
pub struct InitCommand;

impl InitCommand {
    /// Execute init command
    pub async fn execute(args: InitArgs) -> Result<()> {
        use super::constants::MEMORY_INIT_OPS;
        with_resource_limits(MEMORY_INIT_OPS, Self::execute_inner(args)).await
    }

    async fn execute_inner(args: InitArgs) -> Result<()> {
        // Load schema if provided
        let schema = if let Some(schema_path) = &args.schema {
            Some(InitService::load_schema_from_file(schema_path)?)
        } else {
            None
        };

        // Parse properties
        let properties = InitService::parse_properties(&args.properties);

        // Build config and delegate to service
        let config = InitConfig {
            path: args.path.clone(),
            schema,
            partition_by: args.partition_by.clone(),
            properties,
        };

        let result = InitService::create_table(config).await?;

        // Output result
        println!(
            "{} Created Apache Iceberg table at {}",
            "✓".green(),
            args.path
        );
        println!("  UUID: {}", result.table_uuid);
        println!("  Metadata: {}", result.metadata_path);

        Ok(())
    }
}
