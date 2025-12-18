//! Integration tests for maintenance services
//!
//! Tests maintenance service analyze methods using local fixtures.
//! Note: Execute methods are not tested as they would modify the fixtures.

use icetable::core::maintenance::{
    MaintenanceConfig, OptimizeService, VacuumConfig, VacuumService,
};
use icetable::core::metadata::DataFileInfo;

// ============================================================================
// OptimizeService Tests
// ============================================================================

#[test]
fn test_optimize_analyze_no_small_files() {
    // The fixture has a single file that's larger than default min_size
    let service = OptimizeService::new();

    // Create a file that's larger than the default min_size (target_size / 16)
    let files = vec![DataFileInfo {
        path: "data/file1.parquet".to_string(),
        size: 100_000_000, // 100MB - larger than default min_size
        record_count: 10000,
        partition: std::collections::HashMap::new(),
    }];

    let groups = service.analyze(&files);

    // No groups should need compaction since file is large
    assert!(groups.is_empty(), "Large file should not need compaction");
}

#[test]
fn test_optimize_analyze_small_files() {
    let service = OptimizeService::new();

    // Create multiple small files that need compaction
    let files = vec![
        DataFileInfo {
            path: "data/file1.parquet".to_string(),
            size: 1_000_000, // 1MB
            record_count: 1000,
            partition: std::collections::HashMap::new(),
        },
        DataFileInfo {
            path: "data/file2.parquet".to_string(),
            size: 2_000_000, // 2MB
            record_count: 2000,
            partition: std::collections::HashMap::new(),
        },
        DataFileInfo {
            path: "data/file3.parquet".to_string(),
            size: 3_000_000, // 3MB
            record_count: 3000,
            partition: std::collections::HashMap::new(),
        },
    ];

    let groups = service.analyze(&files);

    // Should identify one group needing compaction
    assert_eq!(groups.len(), 1, "Should find one group to compact");
    assert_eq!(
        groups[0].files.len(),
        3,
        "Group should contain all 3 small files"
    );
}

#[test]
fn test_optimize_analyze_with_partition_filter() {
    let config = MaintenanceConfig {
        partition_filter: Some("year=2024".to_string()),
        ..Default::default()
    };
    let service = OptimizeService::with_config(config);

    let mut part_2024 = std::collections::HashMap::new();
    part_2024.insert("year".to_string(), "2024".to_string());

    let mut part_2023 = std::collections::HashMap::new();
    part_2023.insert("year".to_string(), "2023".to_string());

    let files = vec![
        DataFileInfo {
            path: "data/year=2024/file1.parquet".to_string(),
            size: 1_000_000,
            record_count: 1000,
            partition: part_2024.clone(),
        },
        DataFileInfo {
            path: "data/year=2024/file2.parquet".to_string(),
            size: 2_000_000,
            record_count: 2000,
            partition: part_2024,
        },
        DataFileInfo {
            path: "data/year=2023/file1.parquet".to_string(),
            size: 1_000_000,
            record_count: 1000,
            partition: part_2023,
        },
    ];

    let groups = service.analyze(&files);

    // Should only find year=2024 partition
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].files.len(), 2);
}

#[test]
fn test_optimize_with_dry_run_config() {
    let config = MaintenanceConfig {
        dry_run: true,
        target_size: 512 * 1024 * 1024, // 512MB
        parallelism: 4,
        ..Default::default()
    };

    let service = OptimizeService::with_config(config);

    // Service should be created with the config
    // Need at least 2 small files for compaction (single file doesn't need compaction)
    let files = vec![
        DataFileInfo {
            path: "data/file1.parquet".to_string(),
            size: 1_000_000, // 1MB
            record_count: 1000,
            partition: std::collections::HashMap::new(),
        },
        DataFileInfo {
            path: "data/file2.parquet".to_string(),
            size: 2_000_000, // 2MB
            record_count: 2000,
            partition: std::collections::HashMap::new(),
        },
    ];

    let groups = service.analyze(&files);
    // Two small files should be identified for compaction
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].files.len(), 2);
}

// ============================================================================
// VacuumService Tests
// ============================================================================

#[test]
fn test_vacuum_config_defaults() {
    let config = VacuumConfig::default();

    // Verify sensible defaults
    assert!(config.retention_hours > 0, "Should have positive retention");
    assert!(!config.dry_run, "Should not be dry-run by default");
    assert!(config.parallelism > 0, "Should have positive parallelism");
}

#[test]
fn test_vacuum_service_creation() {
    let _service = VacuumService::new();
    // Service should be created without error

    let config = VacuumConfig {
        retention_hours: 24,
        dry_run: true,
        parallelism: 8,
    };
    let service_with_config = VacuumService::with_config(config);
    // Should accept custom config

    // Verify dry_run is respected
    assert!(
        service_with_config
            .to_maintenance_result(&icetable::core::maintenance::VacuumResult {
                deleted_count: 0,
                deleted_bytes: 0,
                errors: vec![],
                dry_run: true,
                analysis: icetable::core::maintenance::VacuumAnalysis {
                    orphan_files: vec![],
                    orphan_bytes: 0,
                    referenced_count: 10,
                    retention_hours: 24,
                },
            })
            .operation
            .contains("dry-run")
    );
}

// ============================================================================
// MaintenanceConfig Tests
// ============================================================================

#[test]
fn test_maintenance_config_defaults() {
    let config = MaintenanceConfig::default();

    // Verify sensible defaults
    assert!(config.target_size > 0, "Should have positive target size");
    assert!(config.min_size > 0, "Should have positive min size");
    assert!(config.max_size > 0, "Should have positive max size");
    assert!(config.parallelism > 0, "Should have positive parallelism");
    assert!(!config.dry_run, "Should not be dry-run by default");
    assert!(
        config.partition_filter.is_none(),
        "No partition filter by default"
    );
    assert!(config.max_files.is_none(), "No max files limit by default");
    assert!(config.max_bytes.is_none(), "No max bytes limit by default");
}

#[test]
fn test_maintenance_config_custom() {
    let config = MaintenanceConfig {
        target_size: 1024 * 1024 * 1024,  // 1GB
        min_size: 64 * 1024 * 1024,       // 64MB
        max_size: 2 * 1024 * 1024 * 1024, // 2GB
        dry_run: true,
        parallelism: 8,
        partition_filter: Some("date>=2024-01-01".to_string()),
        max_files: Some(100),
        max_bytes: Some(10 * 1024 * 1024 * 1024), // 10GB
    };

    assert_eq!(config.target_size, 1024 * 1024 * 1024);
    assert_eq!(config.min_size, 64 * 1024 * 1024);
    assert_eq!(config.max_size, 2 * 1024 * 1024 * 1024);
    assert!(config.dry_run);
    assert_eq!(config.parallelism, 8);
    assert_eq!(
        config.partition_filter,
        Some("date>=2024-01-01".to_string())
    );
    assert_eq!(config.max_files, Some(100));
    assert_eq!(config.max_bytes, Some(10 * 1024 * 1024 * 1024));
}

// ============================================================================
// Partition Filter Tests
// ============================================================================

#[test]
fn test_partition_filter_exact_match() {
    use icetable::core::maintenance::matches_partition_filter;

    assert!(matches_partition_filter("year=2024", "year=2024"));
    assert!(!matches_partition_filter("year=2024", "year=2023"));
}

#[test]
fn test_partition_filter_multiple_parts() {
    use icetable::core::maintenance::matches_partition_filter;

    assert!(matches_partition_filter("year=2024/month=12", "year=2024"));
    assert!(matches_partition_filter("year=2024/month=12", "month=12"));
    assert!(!matches_partition_filter("year=2024/month=12", "month=11"));
}

#[test]
fn test_partition_filter_wildcard() {
    use icetable::core::maintenance::matches_partition_filter;

    assert!(matches_partition_filter("year=2024", "year=*"));
    assert!(matches_partition_filter("year=2024/month=12", "year=202*"));
}

#[test]
fn test_partition_filter_range() {
    use icetable::core::maintenance::matches_partition_filter;

    assert!(matches_partition_filter("year=2024", "year>=2020"));
    assert!(matches_partition_filter("year=2024", "year<=2025"));
    assert!(!matches_partition_filter("year=2019", "year>=2020"));
}

// ============================================================================
// ManifestService Tests
// ============================================================================

#[test]
fn test_manifest_config_defaults() {
    use icetable::core::maintenance::ManifestConfig;

    let config = ManifestConfig::default();

    assert_eq!(
        config.target_size,
        8 * 1024 * 1024,
        "Default target size should be 8MB"
    );
    assert_eq!(config.min_manifests, 3, "Default min manifests should be 3");
    assert!(!config.dry_run, "Should not be dry-run by default");
    assert!(config.branch.is_none(), "No branch by default");
}

#[test]
fn test_manifest_config_custom() {
    use icetable::core::maintenance::ManifestConfig;

    let config = ManifestConfig {
        target_size: 16 * 1024 * 1024,
        min_manifests: 5,
        dry_run: true,
        branch: Some("feature".to_string()),
    };

    assert_eq!(config.target_size, 16 * 1024 * 1024);
    assert_eq!(config.min_manifests, 5);
    assert!(config.dry_run);
    assert_eq!(config.branch, Some("feature".to_string()));
}

#[test]
fn test_manifest_service_creation() {
    use icetable::core::maintenance::{ManifestConfig, ManifestService};

    let _service = ManifestService::new();

    let config = ManifestConfig {
        target_size: 16 * 1024 * 1024,
        min_manifests: 10,
        dry_run: true,
        branch: Some("develop".to_string()),
    };
    let _service_with_config = ManifestService::with_config(config);
}

#[test]
fn test_manifest_analysis_struct() {
    use icetable::core::maintenance::ManifestAnalysis;

    let analysis = ManifestAnalysis {
        current_manifests: 10,
        data_manifests: 8,
        delete_manifests: 2,
        total_entries: 1000,
        estimated_after: 3,
        should_rewrite: true,
        skip_reason: None,
    };

    assert_eq!(analysis.current_manifests, 10);
    assert_eq!(analysis.data_manifests, 8);
    assert_eq!(analysis.delete_manifests, 2);
    assert_eq!(analysis.total_entries, 1000);
    assert_eq!(analysis.estimated_after, 3);
    assert!(analysis.should_rewrite);
    assert!(analysis.skip_reason.is_none());
}

// ============================================================================
// SnapshotService Tests
// ============================================================================

#[test]
fn test_snapshot_config_defaults() {
    use icetable::core::maintenance::SnapshotConfig;

    let config = SnapshotConfig::default();

    assert!(!config.dry_run, "Should not be dry-run by default");
}

#[test]
fn test_snapshot_config_custom() {
    use icetable::core::maintenance::SnapshotConfig;

    let config = SnapshotConfig { dry_run: true };

    assert!(config.dry_run);
}

#[test]
fn test_snapshot_service_creation() {
    use icetable::core::maintenance::{SnapshotConfig, SnapshotService};

    let _service = SnapshotService::new();

    let config = SnapshotConfig { dry_run: true };
    let _service_with_config = SnapshotService::with_config(config);
}

#[test]
fn test_snapshot_details_struct() {
    use icetable::core::maintenance::SnapshotDetails;

    let details = SnapshotDetails {
        id: 123456789,
        timestamp: None,
        parent_id: Some(123456788),
        is_current: true,
        operation: Some("append".to_string()),
    };

    assert_eq!(details.id, 123456789);
    assert!(details.timestamp.is_none());
    assert_eq!(details.parent_id, Some(123456788));
    assert!(details.is_current);
    assert_eq!(details.operation, Some("append".to_string()));
}

// ============================================================================
// RefService Tests
// ============================================================================

#[test]
fn test_ref_config_defaults() {
    use icetable::core::maintenance::RefConfig;

    let config = RefConfig::default();

    assert!(!config.dry_run, "Should not be dry-run by default");
}

#[test]
fn test_ref_config_custom() {
    use icetable::core::maintenance::RefConfig;

    let config = RefConfig { dry_run: true };

    assert!(config.dry_run);
}

#[test]
fn test_ref_service_creation() {
    use icetable::core::maintenance::{RefConfig, RefService};

    let _service = RefService::new();

    let config = RefConfig { dry_run: true };
    let _service_with_config = RefService::with_config(config);
}

#[test]
fn test_branch_retention_defaults() {
    use icetable::core::maintenance::BranchRetention;

    let retention = BranchRetention::default();

    assert!(retention.min_snapshots_to_keep.is_none());
    assert!(retention.max_snapshot_age_ms.is_none());
    assert!(retention.max_ref_age_ms.is_none());
}

#[test]
fn test_branch_retention_custom() {
    use icetable::core::maintenance::BranchRetention;

    let retention = BranchRetention {
        min_snapshots_to_keep: Some(5),
        max_snapshot_age_ms: Some(86400000), // 1 day
        max_ref_age_ms: Some(604800000),     // 7 days
    };

    assert_eq!(retention.min_snapshots_to_keep, Some(5));
    assert_eq!(retention.max_snapshot_age_ms, Some(86400000));
    assert_eq!(retention.max_ref_age_ms, Some(604800000));
}

#[test]
fn test_ref_result_struct() {
    use icetable::core::maintenance::RefResult;

    let result = RefResult {
        name: "feature-branch".to_string(),
        snapshot_id: 123456789,
        new_version: Some(5),
        dry_run: false,
    };

    assert_eq!(result.name, "feature-branch");
    assert_eq!(result.snapshot_id, 123456789);
    assert_eq!(result.new_version, Some(5));
    assert!(!result.dry_run);
}

// ============================================================================
// FileGroup Tests
// ============================================================================

#[test]
fn test_file_group_creation() {
    use icetable::core::maintenance::FileGroup;

    let group = FileGroup::new("year=2024".to_string());

    assert!(group.files.is_empty());
    assert_eq!(group.total_size, 0);
    assert_eq!(group.total_records, 0);
    assert_eq!(group.partition_key, "year=2024");
}

#[test]
fn test_file_group_add() {
    use icetable::core::maintenance::FileGroup;

    let mut group = FileGroup::new(String::new());

    group.add(DataFileInfo {
        path: "file1.parquet".to_string(),
        size: 1000,
        record_count: 100,
        partition: std::collections::HashMap::new(),
    });

    assert_eq!(group.files.len(), 1);
    assert_eq!(group.total_size, 1000);
    assert_eq!(group.total_records, 100);

    group.add(DataFileInfo {
        path: "file2.parquet".to_string(),
        size: 2000,
        record_count: 200,
        partition: std::collections::HashMap::new(),
    });

    assert_eq!(group.files.len(), 2);
    assert_eq!(group.total_size, 3000);
    assert_eq!(group.total_records, 300);
}

#[test]
fn test_file_group_needs_compaction() {
    use icetable::core::maintenance::FileGroup;

    let mut group = FileGroup::new(String::new());

    // Single file - no compaction needed
    group.add(DataFileInfo {
        path: "file1.parquet".to_string(),
        size: 1000,
        record_count: 100,
        partition: std::collections::HashMap::new(),
    });
    assert!(
        !group.needs_compaction(10000),
        "Single file shouldn't need compaction"
    );

    // Two small files - compaction needed
    group.add(DataFileInfo {
        path: "file2.parquet".to_string(),
        size: 2000,
        record_count: 200,
        partition: std::collections::HashMap::new(),
    });
    assert!(
        group.needs_compaction(10000),
        "Two small files should need compaction"
    );
}

#[test]
fn test_group_files_by_partition() {
    use icetable::core::maintenance::group_files_by_partition;

    let mut part_2024 = std::collections::HashMap::new();
    part_2024.insert("year".to_string(), "2024".to_string());

    let mut part_2023 = std::collections::HashMap::new();
    part_2023.insert("year".to_string(), "2023".to_string());

    let files = vec![
        DataFileInfo {
            path: "2024/file1.parquet".to_string(),
            size: 1000,
            record_count: 100,
            partition: part_2024.clone(),
        },
        DataFileInfo {
            path: "2024/file2.parquet".to_string(),
            size: 2000,
            record_count: 200,
            partition: part_2024.clone(),
        },
        DataFileInfo {
            path: "2023/file1.parquet".to_string(),
            size: 3000,
            record_count: 300,
            partition: part_2023.clone(),
        },
    ];

    let groups = group_files_by_partition(files);

    assert_eq!(groups.len(), 2, "Should have 2 partition groups");
    assert_eq!(groups.get("year=2024").unwrap().files.len(), 2);
    assert_eq!(groups.get("year=2023").unwrap().files.len(), 1);
}

// ============================================================================
// MaintenanceConfig Validation Tests
// ============================================================================

#[test]
fn test_config_validation_valid() {
    let config = MaintenanceConfig::default();
    assert!(config.validate().is_ok(), "Default config should be valid");
}

#[test]
fn test_config_validation_custom_valid() {
    let config = MaintenanceConfig {
        target_size: 256 * 1024 * 1024,
        min_size: 16 * 1024 * 1024,
        max_size: 512 * 1024 * 1024,
        dry_run: false,
        parallelism: 8,
        partition_filter: None,
        max_files: Some(100),
        max_bytes: Some(10 * 1024 * 1024 * 1024),
    };
    assert!(config.validate().is_ok(), "Valid custom config should pass");
}

#[test]
fn test_config_validation_min_size_too_large() {
    let config = MaintenanceConfig {
        target_size: 100,
        min_size: 200, // min_size > target_size
        max_size: 300,
        dry_run: false,
        parallelism: 4,
        partition_filter: None,
        max_files: None,
        max_bytes: None,
    };
    let result = config.validate();
    assert!(result.is_err(), "min_size >= target_size should fail");
    assert!(result.unwrap_err().to_string().contains("min_size"));
}

#[test]
fn test_config_validation_target_size_too_large() {
    let config = MaintenanceConfig {
        target_size: 300,
        min_size: 100,
        max_size: 200, // target_size > max_size
        dry_run: false,
        parallelism: 4,
        partition_filter: None,
        max_files: None,
        max_bytes: None,
    };
    let result = config.validate();
    assert!(result.is_err(), "target_size >= max_size should fail");
    assert!(result.unwrap_err().to_string().contains("target_size"));
}

#[test]
fn test_config_validation_zero_parallelism() {
    let config = MaintenanceConfig {
        target_size: 200,
        min_size: 100,
        max_size: 300,
        dry_run: false,
        parallelism: 0, // Invalid
        partition_filter: None,
        max_files: None,
        max_bytes: None,
    };
    let result = config.validate();
    assert!(result.is_err(), "parallelism == 0 should fail");
    assert!(result.unwrap_err().to_string().contains("parallelism"));
}

#[test]
fn test_config_validation_zero_max_files() {
    let config = MaintenanceConfig {
        target_size: 200,
        min_size: 100,
        max_size: 300,
        dry_run: false,
        parallelism: 4,
        partition_filter: None,
        max_files: Some(0), // Invalid
        max_bytes: None,
    };
    let result = config.validate();
    assert!(result.is_err(), "max_files == 0 should fail");
    assert!(result.unwrap_err().to_string().contains("max_files"));
}

#[test]
fn test_config_validation_zero_max_bytes() {
    let config = MaintenanceConfig {
        target_size: 200,
        min_size: 100,
        max_size: 300,
        dry_run: false,
        parallelism: 4,
        partition_filter: None,
        max_files: None,
        max_bytes: Some(0), // Invalid
    };
    let result = config.validate();
    assert!(result.is_err(), "max_bytes == 0 should fail");
    assert!(result.unwrap_err().to_string().contains("max_bytes"));
}
