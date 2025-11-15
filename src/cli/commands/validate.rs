//! Validate command implementation

use std::path::Path;

use crate::cli::output::{OutputFormatter, SeverityIcon, StatusIcon};
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

        // 2. Create format handler
        let path = Path::new(&args.path);
        let handler = FormatHandlerRegistry::global()
            .create_handler(path, storage)
            .await?;

        // 3. Execute basic validation
        let show_progress = args.output != "quiet" && args.output != "json";
        let progress = if show_progress {
            Some(ProgressTracker::spinner("Validating file structure..."))
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
        match args.output.as_str() {
            "quiet" => {
                // Quiet mode: no output, just exit code
            }
            "json" => {
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
            }
            _ => {
                // Text output - Format and status box
                let mut format_lines = vec![
                    format!("Format: {}", result.format_name),
                    String::new(),
                    if result.is_valid {
                        format!("{} File is valid", StatusIcon::Success)
                    } else {
                        format!("{} File is invalid", StatusIcon::Error)
                    },
                ];

                if result.quick_mode {
                    format_lines.push("(Quick validation mode - structure only)".to_string());
                }

                println!(
                    "{}\n",
                    OutputFormatter::framed_box(None, format_lines, None)
                );

                // Get additional metadata for more informative output
                let storage = StorageBackendFactory::create_backend(&args.path).await?;
                let path = Path::new(&args.path);
                let handler = FormatHandlerRegistry::global()
                    .create_handler(path, storage)
                    .await?;

                if let Ok(schema) = handler.read_schema().await {
                    // Schema box
                    let mut schema_lines = vec![format!("{} fields", schema.fields().len())];
                    for field in schema.fields() {
                        let nullable = if field.is_nullable() {
                            " (nullable)"
                        } else {
                            ""
                        };
                        schema_lines.push(format!(
                            "  • {} → {:?}{}",
                            field.name(),
                            field.data_type(),
                            nullable
                        ));
                    }
                    println!(
                        "{}\n",
                        OutputFormatter::framed_box(Some("Schema"), schema_lines, None)
                    );
                }

                if let Ok(metadata) = handler.read_metadata().await {
                    // Metadata box
                    let mut metadata_lines = Vec::new();

                    if let Some(rows) = metadata.num_rows {
                        metadata_lines.push(format!("Rows: {}", rows));
                    }
                    if let Some(size) = metadata.compressed_size {
                        metadata_lines.push(format!(
                            "File size: {} bytes ({:.2} KB)",
                            size,
                            size as f64 / 1024.0
                        ));
                    }
                    if let Some(uncompressed) = metadata.uncompressed_size {
                        if let Some(compressed) = metadata.compressed_size {
                            let ratio = (compressed as f64 / uncompressed as f64) * 100.0;
                            metadata_lines.push(format!("Compression ratio: {:.2}%", ratio));
                        }
                    }
                    if let Some(compression) = &metadata.compression {
                        metadata_lines.push(format!("Compression: {}", compression));
                    }
                    if let Some(version) = &metadata.format_version {
                        metadata_lines.push(format!("Format version: {}", version));
                    }
                    if let Some(created_at) = metadata.created_at {
                        metadata_lines.push(format!(
                            "Created at: {}",
                            created_at.format("%Y-%m-%d %H:%M:%S UTC")
                        ));
                    }

                    // Show format-specific metadata if available
                    if !metadata.metadata.is_empty() {
                        metadata_lines.push(String::new());
                        metadata_lines.push("Additional:".to_string());
                        for (key, value) in &metadata.metadata {
                            metadata_lines.push(format!("  • {}: {}", key, value));
                        }
                    }

                    println!(
                        "{}\n",
                        OutputFormatter::framed_box(Some("Metadata"), metadata_lines, None)
                    );
                }

                if !result.errors.is_empty() {
                    println!("Errors:");
                    for error in &result.errors {
                        println!("  {}  {}", SeverityIcon::Error, error);
                    }
                    println!();
                }

                if !result.warnings.is_empty() {
                    println!("Warnings:");
                    for warning in &result.warnings {
                        println!("  {}  {}", SeverityIcon::Warning, warning);
                    }
                    println!();
                }

                if !result.recommendations.is_empty() {
                    println!("Recommendations:");
                    for rec in &result.recommendations {
                        println!("  → {}", rec);
                    }
                    println!();
                }

                // Display custom rules results
                if let Some(rules_res) = &rules_results {
                    let mut rules_lines = Vec::new();

                    let passed = rules_res.iter().filter(|r| r.passed).count();

                    let mut errors = Vec::new();
                    let mut warnings = Vec::new();
                    let mut infos = Vec::new();

                    for rule_result in rules_res {
                        match rule_result.severity {
                            Severity::Error => errors.push(rule_result),
                            Severity::Warning => warnings.push(rule_result),
                            Severity::Info => infos.push(rule_result),
                        }
                    }

                    let failed_errors = errors.iter().filter(|r| !r.passed).count();
                    let failed_warnings = warnings.iter().filter(|r| !r.passed).count();

                    // Summary with bullet points
                    rules_lines.push(format!("  • Passed: {} {}", StatusIcon::Success, passed));
                    if failed_errors > 0 {
                        rules_lines.push(format!(
                            "  • Errors: {} {}",
                            StatusIcon::Error,
                            failed_errors
                        ));
                    }
                    if failed_warnings > 0 {
                        rules_lines.push(format!(
                            "  • Warnings: {} {}",
                            StatusIcon::Warning,
                            failed_warnings
                        ));
                    }
                    rules_lines.push(String::new());

                    // Only show failed rules
                    let failed_errors: Vec<_> = errors.iter().filter(|r| !r.passed).collect();
                    if !failed_errors.is_empty() {
                        rules_lines.push("Errors:".to_string());
                        for r in &failed_errors {
                            rules_lines.push(format!(
                                "   {}  {} - {}",
                                SeverityIcon::Error,
                                r.rule_name,
                                r.message
                            ));
                            if let Some(details) = &r.details {
                                rules_lines.push(format!("      {}", details));
                            }
                        }
                        rules_lines.push(String::new());
                    }

                    let failed_warnings: Vec<_> = warnings.iter().filter(|r| !r.passed).collect();
                    if !failed_warnings.is_empty() {
                        rules_lines.push("Warnings:".to_string());
                        for r in &failed_warnings {
                            rules_lines.push(format!(
                                "   {}  {} - {}",
                                SeverityIcon::Warning,
                                r.rule_name,
                                r.message
                            ));
                            if let Some(details) = &r.details {
                                rules_lines.push(format!("      {}", details));
                            }
                        }
                        rules_lines.push(String::new());
                    }

                    let failed_infos: Vec<_> = infos.iter().filter(|r| !r.passed).collect();
                    if !failed_infos.is_empty() && args.output != "quiet" {
                        rules_lines.push("Info:".to_string());
                        for r in &failed_infos {
                            rules_lines.push(format!(
                                "   {}  {} - {}",
                                SeverityIcon::Info,
                                r.rule_name,
                                r.message
                            ));
                        }
                    }

                    println!(
                        "{}",
                        OutputFormatter::framed_box(
                            Some("Custom Validation Rules"),
                            rules_lines,
                            None
                        )
                    );
                }
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
