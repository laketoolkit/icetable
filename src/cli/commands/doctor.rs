//! Doctor command implementation
//!
//! Thin wrapper that delegates to DoctorService in core.
//!
//! Two modes:
//! 1. Environment check (no table path): Checks credentials, config, connectivity
//! 2. Table integrity check (with --table): Validates Iceberg table structure

use colored::Colorize;

use crate::cli::output::DoctorFormatter;
use crate::cli::parser::DoctorArgs;
use crate::core::maintenance::{CheckResult, DoctorConfig, DoctorService};
use crate::error::Result;
use crate::utils::with_resource_limits;

/// Handler for doctor command
pub struct DoctorCommand;

impl DoctorCommand {
    /// Execute doctor command
    pub async fn execute(args: DoctorArgs) -> Result<()> {
        use super::constants::MEMORY_MEDIUM_OPS;
        with_resource_limits(MEMORY_MEDIUM_OPS, Self::execute_inner(args)).await
    }

    async fn execute_inner(args: DoctorArgs) -> Result<()> {
        use crate::config::Config;

        // Try to get a table from current context for table integrity check
        let config = Config::load().ok();
        let table_path = config.as_ref().and_then(|c| {
            c.get_current_table()
                .and_then(|_| c.get_current_context())
                .and_then(|ctx| c.resolve_table(ctx).ok())
                .map(|resolved| match resolved {
                    crate::config::ResolvedTable::Path(path) => path,
                    crate::config::ResolvedTable::Catalog { table_name, .. } => table_name,
                })
        });

        match table_path {
            Some(path) => Self::execute_table_check(&args, &path).await,
            None => Self::execute_environment_check(&args).await,
        }
    }

    /// Run environment health checks
    async fn execute_environment_check(args: &DoctorArgs) -> Result<()> {
        use crate::config::Config;

        if args.check_files {
            println!(
                "{}: --check-files requires a table path (-t or configured via 'icetable config use'). Ignoring.",
                "Warning".yellow()
            );
            println!();
        }

        println!(
            "{}",
            "icetable doctor - Environment Health Check".bold().cyan()
        );
        println!();

        let mut checks = Vec::new();

        // Run basic checks - delegated to DoctorService
        checks.push(DoctorService::check_version());
        checks.push(DoctorService::check_config_file());
        checks.extend(DoctorService::check_configuration(args.catalog.as_deref()).await);

        // Determine which storage types are in use from config
        let storage_types = DoctorService::detect_configured_storage_types();

        // Only check credentials for storage types actually in use
        if storage_types.uses_s3 {
            checks.push(DoctorService::check_aws_credentials());
            checks.push(DoctorService::check_aws_endpoint());
        }
        if storage_types.uses_gcs {
            checks.push(DoctorService::check_gcs_credentials());
        }
        if storage_types.uses_azure {
            checks.push(DoctorService::check_azure_credentials());
        }

        // If no cloud storage configured, note that
        if !storage_types.uses_s3
            && !storage_types.uses_gcs
            && !storage_types.uses_azure
            && let Ok(config) = Config::load()
            && config.tables.is_empty()
            && config.catalogs.is_empty()
        {
            checks.push(CheckResult::ok(
                "Cloud credentials",
                "No cloud storage configured (local paths only)",
            ));
        }

        if args.storage {
            checks.push(DoctorService::check_storage_connectivity().await);
        }

        Self::output_results(&checks, &args.output);
        Ok(())
    }

    /// Run table integrity checks
    async fn execute_table_check(args: &DoctorArgs, table_path: &str) -> Result<()> {
        let is_json = args.output == "json";

        if !is_json {
            println!(
                "{}",
                "icetable doctor - Table Integrity Check".bold().cyan()
            );
            println!("Table: {}", table_path.cyan());
            println!();
        }

        let config = DoctorConfig {
            check_files: args.check_files,
            test_storage: args.storage,
            catalog: args.catalog.clone(),
        };

        let service = DoctorService::with_config(config);
        let checks = service.check_table_integrity(table_path).await?;

        Self::output_results(&checks, &args.output);

        // Additional summary for table checks
        if !is_json {
            println!();
            println!(
                "{}",
                DoctorFormatter::format_table_integrity_message(&checks)
            );
        }

        Ok(())
    }

    /// Output results in appropriate format
    fn output_results(checks: &[CheckResult], output_format: &str) {
        if output_format == "json" {
            match DoctorFormatter::format_json(checks) {
                Ok(json) => println!("{}", json),
                Err(e) => eprintln!("Error serializing JSON: {}", e),
            }
        } else {
            println!("{}", DoctorFormatter::format_table(checks));
            println!();
            println!("{}", DoctorFormatter::format_summary(checks));

            if let Some(msg) = DoctorFormatter::format_status_message(checks) {
                println!();
                println!("{}", msg);
            }
        }
    }
}
