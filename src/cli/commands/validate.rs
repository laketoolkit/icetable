//! Validate command implementation
//!
//! Validates Iceberg table structure and metadata.

use std::path::Path;

use super::common::{create_spinner, print_json, resolve_table_path};
use crate::cli::parser::ValidateArgs;
use crate::core::CatalogConfig;
use crate::core::formats::FormatHandlerRegistry;
use crate::core::operations::validate::ValidateOperation;
use crate::core::storage::create_object_store;
use crate::core::validation::{Severity, ValidationEngine};
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Handler for validate command
pub struct ValidateCommand;

impl ValidateCommand {
    /// Execute validate command
    pub async fn execute(args: ValidateArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        const ESTIMATED_MEMORY: u64 = 64 * 1024 * 1024; // 64MB for validation
        with_resource_limits(ESTIMATED_MEMORY, Self::execute_inner(args, catalog_config)).await
    }

    async fn execute_inner(args: ValidateArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        let table_path = resolve_table_path(&args.path, catalog_config.as_ref()).await?;

        // 1. Create storage backend based on path
        let storage = create_object_store(&table_path).await?;

        // 2. Only Iceberg is supported
        if let Some(format) = &args.format
            && format.to_lowercase() != "iceberg"
        {
            return Err(Error::UnsupportedFeature {
                feature: "Only Iceberg tables are supported.".to_string(),
            });
        }

        // 3. Create Iceberg handler
        let path = Path::new(&table_path);
        let handler = FormatHandlerRegistry::global()
            .create_handler(path, storage)
            .await?;

        // Verify it's Iceberg
        if handler.format_name() != "Apache Iceberg" {
            return Err(Error::General(format!(
                "Path '{}' is not an Iceberg table.",
                table_path
            )));
        }

        // 4. Execute basic validation
        let show_progress = !args.quiet && args.output != "json";
        let pb = if show_progress {
            Some(create_spinner("Validating table structure"))
        } else {
            None
        };

        let operation = ValidateOperation::new(handler.into());
        let result = operation.execute(args.quick).await?;

        if let Some(p) = pb {
            p.finish_and_clear();
        }

        // 5. Execute custom rules if provided
        let rules_results = if let Some(rules_path) = &args.rules {
            let pb = if show_progress {
                Some(create_spinner("Running custom validation rules"))
            } else {
                None
            };

            let storage2 = create_object_store(&table_path).await?;
            let handler2 = FormatHandlerRegistry::global()
                .create_handler(Path::new(&table_path), storage2)
                .await?;

            let rules_path_str = rules_path.to_str().ok_or_else(|| {
                crate::error::Error::General("Rules path contains invalid UTF-8".to_string())
            })?;
            let rules = ValidationEngine::load_rules(rules_path_str).await?;
            let engine = ValidationEngine::new(handler2.into(), rules);
            let rules_result = engine.execute().await?;

            if let Some(p) = pb {
                p.finish_and_clear();
            }

            Some(rules_result)
        } else {
            None
        };

        // 6. Determine overall validation status
        let mut overall_valid = result.is_valid;

        if let Some(rules_res) = &rules_results {
            let has_error_failures = rules_res
                .iter()
                .any(|r| !r.passed && r.severity == Severity::Error);

            let has_warning_failures = args.strict
                && rules_res
                    .iter()
                    .any(|r| !r.passed && r.severity == Severity::Warning);

            overall_valid = overall_valid && !has_error_failures && !has_warning_failures;
        }

        // 7. Format and display output
        let filename = Path::new(&table_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&table_path);

        if args.quiet {
            // Quiet mode: no output, just exit code
        } else if args.output == "json" {
            let mut json_output = serde_json::json!({
                "format": result.format_name,
                "is_valid": result.is_valid,
                "errors": result.errors,
                "warnings": result.warnings,
                "recommendations": result.recommendations,
                "num_rows": result.num_rows,
                "file_size": result.file_size,
                "quick_mode": result.quick_mode,
            });

            if let Some(rules_res) = &rules_results {
                json_output["rules"] = serde_json::json!({
                    "total": rules_res.len(),
                    "passed": rules_res.iter().filter(|r| r.passed).count(),
                    "failed": rules_res.iter().filter(|r| !r.passed).count(),
                    "results": rules_res.iter().map(|r| serde_json::json!({
                        "rule": r.rule_name,
                        "passed": r.passed,
                        "severity": format!("{:?}", r.severity),
                        "message": r.message,
                        "details": r.details,
                    })).collect::<Vec<_>>(),
                });
            }

            print_json(&json_output)?;
        } else {
            use colored::Colorize;

            if overall_valid {
                println!(
                    "{} {} is a valid {} table",
                    "✓".green(),
                    filename,
                    result.format_name
                );
            } else {
                println!(
                    "{} {} is an invalid {} table",
                    "✗".red(),
                    filename,
                    result.format_name
                );
            }
        }

        // 8. Exit with appropriate code
        if !args.relax && !overall_valid {
            std::process::exit(1);
        }

        Ok(())
    }
}
