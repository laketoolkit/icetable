//! Doctor command implementation
//!
//! Thin wrapper that delegates to DoctorService in core.
//!
//! Two modes:
//! 1. Environment check (no table path): Checks credentials, config, connectivity
//! 2. Table integrity check (with --table): Validates Iceberg table structure

use colored::Colorize;
use comfy_table::{Cell, Color};

use super::common::{create_table, print_json};
use crate::cli::parser::DoctorArgs;
use crate::core::maintenance::{
    CheckResult, CheckStatus, CheckSummary, DoctorConfig, DoctorService,
};
use crate::error::Result;
use crate::utils::with_resource_limits;

/// Handler for doctor command
pub struct DoctorCommand;

impl DoctorCommand {
    /// Execute doctor command
    pub async fn execute(args: DoctorArgs) -> Result<()> {
        // Apply resource limits (timeout, cancellation, memory tracking)
        const ESTIMATED_MEMORY: u64 = 64 * 1024 * 1024; // 64MB for doctor checks
        with_resource_limits(ESTIMATED_MEMORY, Self::execute_inner(args)).await
    }

    async fn execute_inner(args: DoctorArgs) -> Result<()> {
        use crate::config::ResolvePath;

        match args.path.resolve() {
            Ok(path) => Self::execute_table_check(&args, &path).await,
            Err(_) => Self::execute_environment_check(&args).await,
        }
    }

    /// Run environment health checks
    async fn execute_environment_check(args: &DoctorArgs) -> Result<()> {
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

        // Run all diagnostic checks
        checks.push(DoctorService::check_version());
        checks.push(Self::check_config_file());
        checks.extend(Self::check_configuration(args).await);
        checks.push(DoctorService::check_aws_credentials());
        checks.push(DoctorService::check_aws_endpoint());
        checks.push(DoctorService::check_gcs_credentials());
        checks.push(DoctorService::check_azure_credentials());

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
            let summary = CheckSummary::from_checks(&checks);
            println!();
            if summary.has_errors() {
                println!(
                    "{}",
                    "Table integrity issues found. Review the errors above.".red()
                );
            } else if summary.has_warnings() {
                println!(
                    "{}",
                    "Table appears healthy but has warnings worth reviewing.".yellow()
                );
            } else {
                println!("{}", "Table integrity verified. No issues found.".green());
            }
        }

        Ok(())
    }

    /// Check config file
    fn check_config_file() -> CheckResult {
        use crate::config::Config;

        match Config::config_path() {
            Ok(path) => {
                if path.exists() {
                    match Config::load() {
                        Ok(_) => CheckResult::ok("Config file", path.display().to_string()),
                        Err(e) => CheckResult::warning(
                            "Config file",
                            format!("Parse error: {}", e),
                            "Check config file syntax",
                        ),
                    }
                } else {
                    CheckResult::ok("Config file", "Not created yet (will use defaults)")
                }
            }
            Err(e) => CheckResult::warning(
                "Config file",
                format!("Cannot determine path: {}", e),
                "Check HOME environment variable",
            ),
        }
    }

    /// Check configuration (context, aliases, catalogs)
    async fn check_configuration(args: &DoctorArgs) -> Vec<CheckResult> {
        use crate::config::Config;

        let mut checks = Vec::new();
        let config = match Config::load() {
            Ok(cfg) => cfg,
            Err(_) => return checks,
        };

        // Check current context
        if let Some(context) = config.get_current_context() {
            match config.resolve_table(context) {
                Ok(resolved) => {
                    let msg = match resolved {
                        crate::config::ResolvedTable::Path(path) => {
                            format!("{} -> {}", context, path)
                        }
                        crate::config::ResolvedTable::Catalog {
                            catalog_name,
                            table_name,
                            ..
                        } => {
                            format!("{} -> {}.{}", context, catalog_name, table_name)
                        }
                    };
                    checks.push(CheckResult::ok("Current context", msg));
                }
                Err(e) => {
                    checks.push(CheckResult::warning(
                        "Current context",
                        format!("Invalid: {}", e),
                        "Run 'icetable config use <table>' to set a valid context",
                    ));
                }
            }
        }

        if !config.tables.is_empty() {
            checks.push(CheckResult::ok(
                "Table aliases",
                format!("{} configured", config.tables.len()),
            ));
        }

        if !config.catalogs.is_empty() {
            checks.push(CheckResult::ok(
                "Catalogs",
                format!("{} configured", config.catalogs.len()),
            ));
        }

        // Validate specific catalog if requested
        if let Some(catalog_name) = &args.catalog {
            if let Some(catalog) = config.catalogs.get(catalog_name) {
                checks.push(CheckResult::ok(
                    format!("Catalog '{}'", catalog_name),
                    "Found in config",
                ));

                match DoctorService::test_catalog_connectivity(catalog_name, catalog).await {
                    Ok(_) => {
                        checks.push(CheckResult::ok(
                            format!("Catalog '{}' connectivity", catalog_name),
                            "Connected successfully",
                        ));
                    }
                    Err(e) => {
                        checks.push(CheckResult::error(
                            format!("Catalog '{}' connectivity", catalog_name),
                            format!("Connection failed: {}", e),
                            "Check catalog URI and credentials",
                        ));
                    }
                }
            } else {
                checks.push(CheckResult::error(
                    format!("Catalog '{}'", catalog_name),
                    "Not found in config",
                    "Run 'icetable config add-catalog' to add it",
                ));
            }
        }

        checks
    }

    /// Output results in appropriate format
    fn output_results(checks: &[CheckResult], output_format: &str) {
        if output_format == "json" {
            Self::display_json(checks);
        } else {
            Self::display_table(checks);
        }

        let summary = CheckSummary::from_checks(checks);
        if output_format != "json" {
            println!();
            println!(
                "{}: {} passed, {} warnings, {} errors",
                "Summary".bold(),
                summary.ok_count.to_string().green(),
                summary.warning_count.to_string().yellow(),
                summary.error_count.to_string().red()
            );

            if summary.has_errors() {
                println!();
                println!(
                    "{}",
                    "Some checks failed. Review the suggestions above to fix issues.".yellow()
                );
            }
        }
    }

    /// Display results as a table
    fn display_table(checks: &[CheckResult]) {
        let mut table = create_table();

        table.set_header(vec![
            Cell::new("Status")
                .fg(Color::Cyan)
                .set_alignment(comfy_table::CellAlignment::Center),
            Cell::new("Check")
                .fg(Color::Cyan)
                .set_alignment(comfy_table::CellAlignment::Center),
            Cell::new("Result")
                .fg(Color::Cyan)
                .set_alignment(comfy_table::CellAlignment::Center),
        ]);

        for check in checks {
            let (symbol, color) = match check.status {
                CheckStatus::Ok => (
                    "✓".green().to_string(),
                    Color::Rgb {
                        r: 80,
                        g: 200,
                        b: 120,
                    },
                ),
                CheckStatus::Warning => (
                    "⚠".yellow().to_string(),
                    Color::Rgb {
                        r: 220,
                        g: 180,
                        b: 60,
                    },
                ),
                CheckStatus::Error => (
                    "✗".red().to_string(),
                    Color::Rgb {
                        r: 220,
                        g: 90,
                        b: 90,
                    },
                ),
            };

            table.add_row(vec![
                Cell::new(symbol).set_alignment(comfy_table::CellAlignment::Center),
                Cell::new(&check.name),
                Cell::new(&check.message).fg(color),
            ]);
        }

        println!("{}", table);

        // Print suggestions for issues
        let issues: Vec<_> = checks
            .iter()
            .filter(|c| c.suggestion.is_some() && c.status != CheckStatus::Ok)
            .collect();

        if !issues.is_empty() {
            println!();
            println!("{}", "Suggestions:".bold());
            for check in issues {
                if let Some(ref suggestion) = check.suggestion {
                    println!("  {} {}: {}", "->".dimmed(), check.name.cyan(), suggestion);
                }
            }
        }
    }

    /// Display results as JSON
    fn display_json(checks: &[CheckResult]) {
        let json_checks: Vec<_> = checks
            .iter()
            .map(|c| {
                serde_json::json!({
                    "name": c.name,
                    "status": match c.status {
                        CheckStatus::Ok => "ok",
                        CheckStatus::Warning => "warning",
                        CheckStatus::Error => "error",
                    },
                    "message": c.message,
                    "suggestion": c.suggestion,
                })
            })
            .collect();

        let summary = CheckSummary::from_checks(checks);
        let json = serde_json::json!({
            "checks": json_checks,
            "summary": {
                "ok": summary.ok_count,
                "warnings": summary.warning_count,
                "errors": summary.error_count,
            }
        });

        // Note: print_json returns Result, but display_json doesn't propagate errors
        // This is acceptable since JSON serialization of simple values shouldn't fail
        let _ = print_json(&json);
    }
}
