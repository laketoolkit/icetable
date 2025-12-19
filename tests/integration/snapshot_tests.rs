//! Snapshot tests for CLI output formatting
//!
//! Uses insta for snapshot testing to ensure CLI output doesn't regress.
//! Snapshots are stored in tests/integration/snapshots/
//!
//! Run with: cargo test --test integration_test snapshot
//! Update snapshots with: cargo insta review

use insta::{assert_snapshot, assert_yaml_snapshot};

use icetable::cli::output::{
    DiffFormatter, HistoryFormatter, LsFormatter, HistoryEntryInfo,
};
use icetable::core::operations::{SnapshotDiffResult, SnapshotRef};

// =============================================================================
// HISTORY FORMATTER SNAPSHOTS
// =============================================================================

fn create_sample_history_entries() -> Vec<HistoryEntryInfo> {
    use chrono::{TimeZone, Utc};

    vec![
        HistoryEntryInfo {
            version: 1702987654321012345,
            timestamp: Utc.with_ymd_and_hms(2023, 12, 19, 12, 0, 0).unwrap(),
            is_current: true,
            operation: "append".to_string(),
            details: vec![
                "added-data-files: 5".to_string(),
                "added-records: 50000".to_string(),
            ],
        },
        HistoryEntryInfo {
            version: 1702987654321012344,
            timestamp: Utc.with_ymd_and_hms(2023, 12, 18, 12, 0, 0).unwrap(),
            is_current: false,
            operation: "overwrite".to_string(),
            details: vec![
                "added-data-files: 3".to_string(),
                "deleted-data-files: 10".to_string(),
            ],
        },
        HistoryEntryInfo {
            version: 1702987654321012343,
            timestamp: Utc.with_ymd_and_hms(2023, 12, 17, 12, 0, 0).unwrap(),
            is_current: false,
            operation: "append".to_string(),
            details: vec![
                "added-data-files: 10".to_string(),
                "added-records: 100000".to_string(),
            ],
        },
    ]
}

#[test]
fn test_history_format_table() {
    let entries = create_sample_history_entries();
    let output = HistoryFormatter::format_table(&entries);
    let clean_output = strip_ansi(&output);

    assert_snapshot!("history_table", clean_output);
}

#[test]
fn test_history_format_json() {
    let entries = create_sample_history_entries();
    let json_output = HistoryFormatter::format_json(&entries).expect("JSON formatting failed");

    // Parse and re-serialize for consistent formatting
    let parsed: serde_json::Value = serde_json::from_str(&json_output).expect("Invalid JSON");

    assert_yaml_snapshot!("history_json", parsed);
}

#[test]
fn test_history_format_empty() {
    let entries: Vec<HistoryEntryInfo> = vec![];
    let output = HistoryFormatter::format_table(&entries);
    let clean_output = strip_ansi(&output);

    assert_snapshot!("history_empty", clean_output);
}

// =============================================================================
// DIFF FORMATTER SNAPSHOTS
// =============================================================================

fn create_sample_diff_result() -> SnapshotDiffResult {
    SnapshotDiffResult {
        base: SnapshotRef {
            label: "parent".to_string(),
            snapshot_id: 1234567890,
            timestamp_ms: 1702915200000,
            manifest_count: 2,
        },
        reference: SnapshotRef {
            label: "current".to_string(),
            snapshot_id: 1234567899,
            timestamp_ms: 1703001600000,
            manifest_count: 3,
        },
        is_identical: false,
        manifests_added: vec![
            "metadata/manifest-001.avro".to_string(),
        ],
        manifests_removed: vec![],
    }
}

#[test]
fn test_diff_format_text() {
    let result = create_sample_diff_result();
    let output = DiffFormatter::format_diff_text(&result, "current", "previous");
    let clean_output = strip_ansi(&output);

    assert_snapshot!("diff_text", clean_output);
}

#[test]
fn test_diff_format_json() {
    let result = create_sample_diff_result();
    let json_output = DiffFormatter::format_diff_json(&result).expect("JSON formatting failed");

    let parsed: serde_json::Value = serde_json::from_str(&json_output).expect("Invalid JSON");

    assert_yaml_snapshot!("diff_json", parsed);
}

#[test]
fn test_diff_format_identical() {
    let result = SnapshotDiffResult {
        base: SnapshotRef {
            label: "main".to_string(),
            snapshot_id: 1234567890,
            timestamp_ms: 1702915200000,
            manifest_count: 2,
        },
        reference: SnapshotRef {
            label: "main".to_string(),
            snapshot_id: 1234567890,
            timestamp_ms: 1702915200000,
            manifest_count: 2,
        },
        is_identical: true,
        manifests_added: vec![],
        manifests_removed: vec![],
    };

    let output = DiffFormatter::format_diff_text(&result, "current", "parent");
    let clean_output = strip_ansi(&output);

    assert_snapshot!("diff_identical", clean_output);
}

// =============================================================================
// LS FORMATTER SNAPSHOTS
// =============================================================================

#[test]
fn test_ls_namespaces_tree() {
    let namespaces = vec![
        vec!["production".to_string()],
        vec!["production".to_string(), "analytics".to_string()],
        vec!["production".to_string(), "ml".to_string()],
        vec!["staging".to_string()],
        vec!["development".to_string()],
    ];

    let output = LsFormatter::format_namespaces_tree("my-catalog", &namespaces);
    let clean_output = strip_ansi(&output);

    assert_snapshot!("ls_namespaces_tree", clean_output);
}

#[test]
fn test_ls_tables_tree() {
    let tables = vec![
        "events".to_string(),
        "users".to_string(),
        "transactions".to_string(),
        "sessions".to_string(),
    ];

    let output = LsFormatter::format_tables_tree("my-catalog", "analytics", &tables);
    let clean_output = strip_ansi(&output);

    assert_snapshot!("ls_tables_tree", clean_output);
}

#[test]
fn test_ls_namespaces_json() {
    let namespaces = vec![
        vec!["prod".to_string()],
        vec!["staging".to_string()],
    ];

    let json_output = LsFormatter::format_namespaces_json("catalog", &namespaces)
        .expect("JSON formatting failed");

    let parsed: serde_json::Value = serde_json::from_str(&json_output).expect("Invalid JSON");

    assert_yaml_snapshot!("ls_namespaces_json", parsed);
}

#[test]
fn test_ls_tables_json() {
    let tables = vec!["table1".to_string(), "table2".to_string()];

    let json_output = LsFormatter::format_tables_json("catalog", "namespace", &tables)
        .expect("JSON formatting failed");

    let parsed: serde_json::Value = serde_json::from_str(&json_output).expect("Invalid JSON");

    assert_yaml_snapshot!("ls_tables_json", parsed);
}

// =============================================================================
// JSON OUTPUT CONSISTENCY
// =============================================================================

#[test]
fn test_json_outputs_are_valid() {
    // Verify all JSON outputs are valid JSON

    let history = create_sample_history_entries();
    let history_json = HistoryFormatter::format_json(&history).unwrap();
    let _: serde_json::Value = serde_json::from_str(&history_json)
        .expect("History JSON should be valid");

    let diff = create_sample_diff_result();
    let diff_json = DiffFormatter::format_diff_json(&diff).unwrap();
    let _: serde_json::Value = serde_json::from_str(&diff_json)
        .expect("Diff JSON should be valid");

    let namespaces = vec![vec!["ns".to_string()]];
    let ns_json = LsFormatter::format_namespaces_json("cat", &namespaces).unwrap();
    let _: serde_json::Value = serde_json::from_str(&ns_json)
        .expect("Namespaces JSON should be valid");

    let tables = vec!["t1".to_string()];
    let tables_json = LsFormatter::format_tables_json("cat", "ns", &tables).unwrap();
    let _: serde_json::Value = serde_json::from_str(&tables_json)
        .expect("Tables JSON should be valid");
}

// =============================================================================
// EDGE CASES
// =============================================================================

#[test]
fn test_format_with_unicode() {
    let namespaces = vec![
        vec!["日本語".to_string()],
        vec!["émojis_🎉".to_string()],
        vec!["中文".to_string()],
    ];

    let output = LsFormatter::format_namespaces_tree("catalog", &namespaces);

    // Should handle unicode without panicking
    assert!(output.contains("日本語") || output.len() > 0);
}

#[test]
fn test_format_empty_namespace_list() {
    let namespaces: Vec<Vec<String>> = vec![];
    let output = LsFormatter::format_namespaces_tree("empty-catalog", &namespaces);
    let clean_output = strip_ansi(&output);

    assert_snapshot!("ls_namespaces_empty", clean_output);
}

#[test]
fn test_format_empty_tables_list() {
    let tables: Vec<String> = vec![];
    let output = LsFormatter::format_tables_tree("catalog", "empty-ns", &tables);
    let clean_output = strip_ansi(&output);

    assert_snapshot!("ls_tables_empty", clean_output);
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Strip ANSI escape codes for consistent snapshot comparison
fn strip_ansi(s: &str) -> String {
    let ansi_regex = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    ansi_regex.replace_all(s, "").to_string()
}
