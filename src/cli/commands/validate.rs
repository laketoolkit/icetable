//! Validate command implementation
//!
//! This command validates table formats (Delta Lake, Iceberg).

use std::path::Path;

use crate::cli::parser::ValidateArgs;
use crate::core::formats::{FormatHandler, FormatHandlerRegistry};
use crate::core::operations::validate::ValidateOperation;
use crate::core::storage::StorageBackendFactory;
use crate::core::validation::{Severity, ValidationEngine};
use crate::error::Result;
use crate::utils::progress::ProgressTracker;

/// Handler for validate command
pub struct ValidateCommand;

impl ValidateCommand {
    /// Execute validate command
    pub async fn execute(args: ValidateArgs) -> Result<()> {
        // 1. Create storage backend based on path
        let storage = StorageBackendFactory::create_backend(&args.path).await?;

        // 2. Create format handler (forced format if specified)
        let path = Path::new(&args.path);
        let handler: Box<dyn FormatHandler> = if let Some(format) = &args.format {
            // Force specific format
            match format.to_lowercase().as_str() {
                #[cfg(feature = "delta")]
                "delta" => Box::new(crate::core::formats::DeltaHandler::new(path, storage.clone())?),
                #[cfg(feature = "iceberg")]
                "iceberg" => Box::new(crate::core::formats::IcebergHandler::new(path, storage.clone())?),
                _ => {
                    return Err(crate::error::Error::InvalidFormat {
                        message: format!("Unsupported format: {}", format),
                    });
                }
            }
        } else {
            // Auto-detect format
            FormatHandlerRegistry::global()
                .create_handler(path, storage)
                .await?
        };

        // 3. Execute basic validation
        let show_progress = !args.quiet && args.output != "json";
        let progress = if show_progress {
            Some(ProgressTracker::spinner("Validating table structure..."))
        } else {
            None
        };

        let operation = ValidateOperation::new(handler.into());
        let result = operation.execute(args.quick).await?;

        if let Some(p) = progress {
            p.finish_and_clear();
        }

        // 4. Execute custom rules if provided
        let rules_results = if let Some(rules_path) = &args.rules {
            let progress = if show_progress {
                Some(ProgressTracker::spinner(
                    "Running custom validation rules...",
                ))
            } else {
                None
            };

            // Need to recreate handler for rules engine
            let storage2 = StorageBackendFactory::create_backend(&args.path).await?;
            let handler2 = FormatHandlerRegistry::global()
                .create_handler(Path::new(&args.path), storage2)
                .await?;

            let rules = ValidationEngine::load_rules(rules_path.to_str().unwrap()).await?;
            let engine = ValidationEngine::new(handler2.into(), rules);
            let rules_result = engine.execute().await?;

            if let Some(p) = progress {
                p.finish_and_clear();
            }

            Some(rules_result)
        } else {
            None
        };

        // 5. Determine overall validation status
        let mut overall_valid = result.is_valid;

        if let Some(rules_res) = &rules_results {
            // Check if any error-level rules failed
            let has_error_failures = rules_res
                .iter()
                .any(|r| !r.passed && r.severity == Severity::Error);

            // In strict mode, warnings also fail validation
            let has_warning_failures = args.strict
                && rules_res
                    .iter()
                    .any(|r| !r.passed && r.severity == Severity::Warning);

            overall_valid = overall_valid && !has_error_failures && !has_warning_failures;
        }

        // 6. Format and display output based on output format
        let filename = Path::new(&args.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&args.path);

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

            println!(
                "{}",
                serde_json::to_string_pretty(&json_output)
                    .map_err(|e| crate::error::Error::General(e.to_string()))?
            );
        } else {
            // Simple text output: just filename and status with format
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

        // 7. Exit with appropriate code based on validation result
        // In relax mode, never fail (always exit 0)
        if !args.relax && !overall_valid {
            std::process::exit(1);
        }

        Ok(())
    }
}
