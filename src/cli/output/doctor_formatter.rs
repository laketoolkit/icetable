//! Doctor command formatting utilities

use colored::Colorize;
use comfy_table::{Cell, CellAlignment, Color};

use super::formatter::create_styled_table;
use crate::core::maintenance::{CheckResult, CheckStatus, CheckSummary};

/// Formatter for doctor command results
pub struct DoctorFormatter;

impl DoctorFormatter {
    /// Format check results as a table
    pub fn format_table(checks: &[CheckResult]) -> String {
        let mut output = Vec::new();

        let mut table = create_styled_table();

        table.set_header(vec![
            Cell::new("Status")
                .fg(Color::Cyan)
                .set_alignment(CellAlignment::Center),
            Cell::new("Check")
                .fg(Color::Cyan)
                .set_alignment(CellAlignment::Center),
            Cell::new("Result")
                .fg(Color::Cyan)
                .set_alignment(CellAlignment::Center),
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
                Cell::new(symbol).set_alignment(CellAlignment::Center),
                Cell::new(&check.name),
                Cell::new(&check.message).fg(color),
            ]);
        }

        output.push(table.to_string());

        // Print suggestions for issues
        let issues: Vec<_> = checks
            .iter()
            .filter(|c| c.suggestion.is_some() && c.status != CheckStatus::Ok)
            .collect();

        if !issues.is_empty() {
            output.push(String::new());
            output.push(format!("{}", "Suggestions:".bold()));
            for check in issues {
                if let Some(ref suggestion) = check.suggestion {
                    output.push(format!(
                        "  {} {}: {}",
                        "->".dimmed(),
                        check.name.cyan(),
                        suggestion
                    ));
                }
            }
        }

        output.join("\n")
    }

    /// Format check results as JSON
    pub fn format_json(checks: &[CheckResult]) -> Result<String, serde_json::Error> {
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

        serde_json::to_string_pretty(&json)
    }

    /// Format summary line
    pub fn format_summary(checks: &[CheckResult]) -> String {
        let summary = CheckSummary::from_checks(checks);
        format!(
            "{}: {} passed, {} warnings, {} errors",
            "Summary".bold(),
            summary.ok_count.to_string().green(),
            summary.warning_count.to_string().yellow(),
            summary.error_count.to_string().red()
        )
    }

    /// Format final status message based on check results
    pub fn format_status_message(checks: &[CheckResult]) -> Option<String> {
        let summary = CheckSummary::from_checks(checks);
        if summary.has_errors() {
            Some(format!(
                "{}",
                "Some checks failed. Review the suggestions above to fix issues.".yellow()
            ))
        } else {
            None
        }
    }

    /// Format table integrity result message
    pub fn format_table_integrity_message(checks: &[CheckResult]) -> String {
        let summary = CheckSummary::from_checks(checks);
        if summary.has_errors() {
            format!(
                "{}",
                "Table integrity issues found. Review the errors above.".red()
            )
        } else if summary.has_warnings() {
            format!(
                "{}",
                "Table appears healthy but has warnings worth reviewing.".yellow()
            )
        } else {
            format!("{}", "Table integrity verified. No issues found.".green())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_checks() -> Vec<CheckResult> {
        vec![
            CheckResult::ok("Version", "v0.1.0"),
            CheckResult::ok("Config file", "/home/user/.config/icetable/config.toml"),
            CheckResult::warning("AWS credentials", "Not configured", "Set AWS_ACCESS_KEY_ID"),
        ]
    }

    fn checks_with_errors() -> Vec<CheckResult> {
        vec![
            CheckResult::ok("Version", "v0.1.0"),
            CheckResult::error("Metadata", "File not found", "Check table path"),
        ]
    }

    #[test]
    fn test_format_table() {
        let checks = sample_checks();
        let result = DoctorFormatter::format_table(&checks);

        assert!(result.contains("Version"));
        assert!(result.contains("Config file"));
        assert!(result.contains("AWS credentials"));
        assert!(result.contains("Suggestions"));
        assert!(result.contains("Set AWS_ACCESS_KEY_ID"));
    }

    #[test]
    fn test_format_table_no_suggestions() {
        let checks = vec![
            CheckResult::ok("Version", "v0.1.0"),
            CheckResult::ok("Config", "OK"),
        ];
        let result = DoctorFormatter::format_table(&checks);

        assert!(result.contains("Version"));
        assert!(!result.contains("Suggestions"));
    }

    #[test]
    fn test_format_json() {
        let checks = sample_checks();
        let result = DoctorFormatter::format_json(&checks).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["checks"].as_array().unwrap().len(), 3);
        assert_eq!(parsed["summary"]["ok"], 2);
        assert_eq!(parsed["summary"]["warnings"], 1);
        assert_eq!(parsed["summary"]["errors"], 0);
    }

    #[test]
    fn test_format_json_with_errors() {
        let checks = checks_with_errors();
        let result = DoctorFormatter::format_json(&checks).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["summary"]["ok"], 1);
        assert_eq!(parsed["summary"]["errors"], 1);
    }

    #[test]
    fn test_format_summary() {
        let checks = sample_checks();
        let result = DoctorFormatter::format_summary(&checks);

        assert!(result.contains("Summary"));
        assert!(result.contains("2 passed"));
        assert!(result.contains("1 warnings"));
        assert!(result.contains("0 errors"));
    }

    #[test]
    fn test_format_status_message_with_errors() {
        let checks = checks_with_errors();
        let result = DoctorFormatter::format_status_message(&checks);

        assert!(result.is_some());
        assert!(result.unwrap().contains("Some checks failed"));
    }

    #[test]
    fn test_format_status_message_no_errors() {
        let checks = sample_checks();
        let result = DoctorFormatter::format_status_message(&checks);

        // No message when no errors (only warnings)
        assert!(result.is_none());
    }

    #[test]
    fn test_format_table_integrity_healthy() {
        let checks = vec![CheckResult::ok("Metadata", "Valid")];
        let result = DoctorFormatter::format_table_integrity_message(&checks);

        assert!(result.contains("No issues found"));
    }

    #[test]
    fn test_format_table_integrity_warnings() {
        let checks = vec![CheckResult::warning(
            "Snapshots",
            "Many old snapshots",
            "Consider expiring",
        )];
        let result = DoctorFormatter::format_table_integrity_message(&checks);

        assert!(result.contains("warnings worth reviewing"));
    }

    #[test]
    fn test_format_table_integrity_errors() {
        let checks = checks_with_errors();
        let result = DoctorFormatter::format_table_integrity_message(&checks);

        assert!(result.contains("integrity issues found"));
    }
}
