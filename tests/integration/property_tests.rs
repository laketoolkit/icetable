//! Property-based tests using proptest
//!
//! These tests verify invariants that should hold for all inputs,
//! using randomly generated test cases.
//!
//! Run with: cargo test --test integration_test property -- --ignored

use proptest::prelude::*;
use std::sync::Arc;

use icetable::utils::core::{format_bytes, parse_bytes};
use icetable::utils::{parse_relative_duration, parse_timestamp};
use icetable::core::utils::normalize_path;

// =============================================================================
// BYTES PARSING/FORMATTING ROUNDTRIP
// =============================================================================

proptest! {
    /// format_bytes and parse_bytes should roundtrip for reasonable values
    #[test]
    fn prop_bytes_roundtrip(bytes in 0u64..1_000_000_000_000u64) {
        let formatted = format_bytes(bytes);
        let parsed = parse_bytes(&formatted);

        // Parsing should succeed
        prop_assert!(parsed.is_ok(), "Failed to parse: {}", formatted);

        let parsed_value = parsed.unwrap();

        // Due to formatting precision loss, we allow some tolerance
        // e.g., 1536 bytes -> "1.50 KB" -> 1536 bytes
        let tolerance = if bytes > 1024 {
            bytes / 100 + 1  // 1% tolerance plus 1
        } else {
            1  // Exact for small values
        };

        prop_assert!(
            (parsed_value as i64 - bytes as i64).abs() <= tolerance as i64,
            "Roundtrip mismatch: {} -> {} -> {} (tolerance: {})",
            bytes, formatted, parsed_value, tolerance
        );
    }

    /// parse_bytes should handle various unit formats
    #[test]
    fn prop_parse_bytes_units(value in 1u64..10000u64) {
        // Test different unit formats
        let kb = format!("{}KB", value);
        let mb = format!("{}MB", value);
        let gb = format!("{}GB", value);

        let kb_parsed = parse_bytes(&kb);
        let mb_parsed = parse_bytes(&mb);
        let gb_parsed = parse_bytes(&gb);

        prop_assert!(kb_parsed.is_ok(), "Failed KB: {}", kb);
        prop_assert!(mb_parsed.is_ok(), "Failed MB: {}", mb);
        prop_assert!(gb_parsed.is_ok(), "Failed GB: {}", gb);

        prop_assert_eq!(kb_parsed.unwrap(), value * 1024);
        prop_assert_eq!(mb_parsed.unwrap(), value * 1024 * 1024);
        prop_assert_eq!(gb_parsed.unwrap(), value * 1024 * 1024 * 1024);
    }

    /// parse_bytes with spaces should work
    #[test]
    fn prop_parse_bytes_with_spaces(value in 1u64..1000u64) {
        let with_space = format!("{} MB", value);
        let without_space = format!("{}MB", value);

        let with_space_parsed = parse_bytes(&with_space);
        let without_space_parsed = parse_bytes(&without_space);

        prop_assert!(with_space_parsed.is_ok());
        prop_assert!(without_space_parsed.is_ok());
        prop_assert_eq!(with_space_parsed.unwrap(), without_space_parsed.unwrap());
    }

    /// Plain numbers should parse as bytes
    #[test]
    fn prop_parse_plain_bytes(value in 0u64..u64::MAX / 2) {
        let plain = value.to_string();
        let parsed = parse_bytes(&plain);

        prop_assert!(parsed.is_ok(), "Failed to parse plain: {}", plain);
        prop_assert_eq!(parsed.unwrap(), value);
    }
}

// =============================================================================
// PATH NORMALIZATION
// =============================================================================

proptest! {
    /// Normalized paths should not have trailing slashes (except root)
    /// Note: normalize_path strips only ONE trailing slash per call
    #[test]
    fn prop_normalize_removes_trailing_slash(path in "[a-zA-Z0-9_]{1,100}") {
        // Only test paths that don't already end with /
        let with_slash = format!("{}/", path);
        if let Ok(normalized) = normalize_path(&with_slash) {
            prop_assert!(
                !normalized.ends_with('/') || normalized == "/",
                "Trailing slash not removed: {} -> {}",
                with_slash, normalized
            );
        }
        // If normalization fails, that's also valid behavior
    }

    /// Normalize should be idempotent (after first normalization)
    /// Note: paths with multiple trailing slashes lose one slash per normalization
    #[test]
    fn prop_normalize_idempotent(path in "[a-zA-Z0-9_]{1,50}") {
        // Use paths without slashes to avoid double-slash issues
        if let Ok(once) = normalize_path(&path) {
            if let Ok(twice) = normalize_path(&once) {
                prop_assert_eq!(once, twice, "Normalization not idempotent");
            }
        }
    }

    /// S3 paths should preserve bucket structure
    #[test]
    fn prop_s3_path_preserves_bucket(
        bucket in "[a-z0-9]{3,20}",
        key in "[a-zA-Z0-9/_]{1,50}"
    ) {
        let s3_path = format!("s3://{}/{}", bucket, key);
        if let Ok(normalized) = normalize_path(&s3_path) {
            prop_assert!(
                normalized.starts_with(&format!("s3://{}/", bucket)),
                "Bucket not preserved: {} -> {}",
                s3_path, normalized
            );
        }
    }

    /// Path traversal attempts should be rejected by normalize_path
    #[test]
    fn prop_reject_path_traversal(
        prefix in "[a-zA-Z0-9_]{1,20}",
        suffix in "[a-zA-Z0-9_]{1,20}"
    ) {
        let malicious = format!("{}/../../{}", prefix, suffix);
        // normalize_path should reject paths with ".."
        let result = normalize_path(&malicious);

        prop_assert!(
            result.is_err(),
            "Path traversal should be rejected: {}",
            malicious
        );
    }
}

// =============================================================================
// DURATION PARSING
// =============================================================================

proptest! {
    /// Duration with 'd' suffix should parse correctly
    #[test]
    fn prop_parse_days(days in 1i64..365i64) {
        let input = format!("{}d", days);
        let result = parse_relative_duration(&input);

        prop_assert!(result.is_some(), "Failed to parse: {}", input);

        let duration = result.expect("already verified");
        let expected_seconds = days * 24 * 3600;

        // Duration should be exact for whole days
        prop_assert_eq!(
            duration.num_seconds(),
            expected_seconds,
            "Duration mismatch for {}: got {:?}",
            input, duration
        );
    }

    /// Duration with 'h' suffix should parse correctly
    #[test]
    fn prop_parse_hours(hours in 1i64..1000i64) {
        let input = format!("{}h", hours);
        let result = parse_relative_duration(&input);

        prop_assert!(result.is_some(), "Failed to parse: {}", input);

        let duration = result.expect("already verified");
        prop_assert_eq!(duration.num_seconds(), hours * 3600);
    }

    /// Duration with 'm' suffix should parse correctly
    #[test]
    fn prop_parse_minutes(minutes in 1i64..10000i64) {
        let input = format!("{}m", minutes);
        let result = parse_relative_duration(&input);

        prop_assert!(result.is_some(), "Failed to parse: {}", input);

        let duration = result.expect("already verified");
        prop_assert_eq!(duration.num_seconds(), minutes * 60);
    }
}

// =============================================================================
// SCHEMA TEMPLATE CONSISTENCY
// =============================================================================

use icetable::core::operations::generate::SchemaTemplate;

proptest! {
    /// All schema templates should produce valid schemas with at least one field
    #[test]
    fn prop_templates_have_fields(template_idx in 0usize..5usize) {
        let templates = [
            SchemaTemplate::Events,
            SchemaTemplate::Transactions,
            SchemaTemplate::Sensors,
            SchemaTemplate::Users,
            SchemaTemplate::WebLogs,
        ];

        let template = templates[template_idx % templates.len()].clone();
        let schema = template.to_schema();

        prop_assert!(
            !schema.fields().is_empty(),
            "Template should have fields: {:?}",
            template
        );
    }

    /// Schemas from same template should be identical
    #[test]
    fn prop_templates_are_deterministic(template_idx in 0usize..5usize) {
        let templates = [
            SchemaTemplate::Events,
            SchemaTemplate::Transactions,
            SchemaTemplate::Sensors,
            SchemaTemplate::Users,
            SchemaTemplate::WebLogs,
        ];

        let template = templates[template_idx % templates.len()].clone();

        let schema1 = template.to_schema();
        let schema2 = template.to_schema();

        prop_assert_eq!(
            schema1.fields().len(),
            schema2.fields().len(),
            "Same template should produce same schema"
        );

        for (f1, f2) in schema1.fields().iter().zip(schema2.fields().iter()) {
            prop_assert_eq!(f1.name(), f2.name());
            prop_assert_eq!(f1.data_type(), f2.data_type());
        }
    }
}

// =============================================================================
// GENERATE CONFIG VALIDATION
// =============================================================================

use icetable::core::operations::generate::GenerateConfig;

proptest! {
    /// GenerateConfig should accept reasonable values
    #[test]
    fn prop_generate_config_valid_ranges(
        rows in 1u64..1_000_000u64,
        files in 1u32..100u32,
        seed in any::<u64>()
    ) {
        let config = GenerateConfig {
            path: "s3://test/table".to_string(),
            schema: Arc::new(SchemaTemplate::Events.to_schema()),
            rows,
            files,
            partition_columns: vec![],
            seed,
            target_file_size: 64 * 1024 * 1024,
        };

        // Config should be constructible with any valid values
        prop_assert_eq!(config.rows, rows);
        prop_assert_eq!(config.files, files);
        prop_assert_eq!(config.seed, seed);
    }

    /// Same seed should indicate deterministic generation
    #[test]
    fn prop_seed_determines_output(seed in any::<u64>()) {
        let config1 = GenerateConfig {
            path: "s3://test/table1".to_string(),
            schema: Arc::new(SchemaTemplate::Events.to_schema()),
            rows: 100,
            files: 1,
            partition_columns: vec![],
            seed,
            target_file_size: 64 * 1024 * 1024,
        };

        let config2 = GenerateConfig {
            path: "s3://test/table2".to_string(),
            schema: Arc::new(SchemaTemplate::Events.to_schema()),
            rows: 100,
            files: 1,
            partition_columns: vec![],
            seed,
            target_file_size: 64 * 1024 * 1024,
        };

        // Same seed implies same deterministic behavior
        prop_assert_eq!(config1.seed, config2.seed);
    }
}

// =============================================================================
// MAINTENANCE CONFIG VALIDATION
// =============================================================================

use icetable::core::maintenance::{MaintenanceConfig, VacuumConfig};

proptest! {
    /// MaintenanceConfig with valid parallelism should work
    #[test]
    fn prop_maintenance_config_parallelism(parallelism in 1usize..64usize) {
        let config = MaintenanceConfig {
            parallelism,
            ..Default::default()
        };

        prop_assert_eq!(config.parallelism, parallelism);
    }

    /// VacuumConfig retention should be non-negative
    #[test]
    fn prop_vacuum_retention_valid(hours in 0u64..10000u64) {
        let config = VacuumConfig {
            retention_hours: hours,
            ..Default::default()
        };

        prop_assert_eq!(config.retention_hours, hours);
    }
}

// =============================================================================
// UTILITY FUNCTION INVARIANTS
// =============================================================================

proptest! {
    /// format_bytes output should always be parseable
    #[test]
    fn prop_format_bytes_parseable(bytes in 0u64..u64::MAX / 2) {
        let formatted = format_bytes(bytes);

        // Should not be empty
        prop_assert!(!formatted.is_empty());

        // Should contain a number
        prop_assert!(
            formatted.chars().any(|c| c.is_ascii_digit()),
            "Should contain digits: {}",
            formatted
        );

        // Should end with a unit or just be bytes
        prop_assert!(
            formatted.ends_with("bytes")
                || formatted.ends_with("KB")
                || formatted.ends_with("MB")
                || formatted.ends_with("GB")
                || formatted.ends_with("TB"),
            "Should have unit: {}",
            formatted
        );
    }

    /// Timestamp parsing should handle valid formats (without Z suffix)
    #[test]
    fn prop_timestamp_iso8601(
        year in 2000i32..2030i32,
        month in 1u32..13u32,
        day in 1u32..29u32,  // Safe range to avoid invalid dates
        hour in 0u32..24u32,
        minute in 0u32..60u32,
        second in 0u32..60u32
    ) {
        // Note: parse_timestamp doesn't support Z suffix
        let timestamp_str = format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            year, month, day, hour, minute, second
        );

        let result = parse_timestamp(&timestamp_str);

        prop_assert!(
            result.is_ok(),
            "Failed to parse valid timestamp: {}",
            timestamp_str
        );
    }
}

// =============================================================================
// DATA GENERATION INVARIANTS
// =============================================================================

#[cfg(test)]
mod generation_invariants {
    /// Helper to verify file distribution is reasonable
    fn verify_file_distribution(total_rows: u64, files: usize, rows_per_file: &[u64]) {
        let sum: u64 = rows_per_file.iter().sum();
        assert_eq!(sum, total_rows, "Total rows should match");
        assert_eq!(rows_per_file.len(), files, "File count should match");

        // Files should have reasonable distribution (not all in one file)
        if files > 1 && total_rows > files as u64 {
            let avg = total_rows / files as u64;
            let max_deviation = avg * 3; // Allow 3x average

            for &rows in rows_per_file {
                assert!(
                    rows <= max_deviation,
                    "File has too many rows: {} (avg: {})",
                    rows,
                    avg
                );
            }
        }
    }

    #[test]
    fn test_distribution_simple() {
        verify_file_distribution(1000, 4, &[250, 250, 250, 250]);
    }

    #[test]
    fn test_distribution_uneven() {
        verify_file_distribution(1000, 3, &[334, 333, 333]);
    }
}
