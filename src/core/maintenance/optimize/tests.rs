//! Tests for OptimizeService

use std::collections::HashMap;

use super::pipeline::parse_partition_key;
use super::*;

fn make_file(path: &str, size: u64, partition: HashMap<String, String>) -> DataFileInfo {
    DataFileInfo {
        path: path.to_string(),
        size,
        record_count: 100,
        partition,
    }
}

#[test]
fn test_service_creation() {
    let service = OptimizeService::new();
    assert_eq!(
        service.config.target_size,
        MaintenanceConfig::default().target_size
    );
    assert!(!service.config.dry_run);
}

#[test]
fn test_service_with_config() {
    let config = MaintenanceConfig {
        target_size: 512 * 1024 * 1024, // 512MB
        dry_run: true,
        ..Default::default()
    };
    let service = OptimizeService::with_config(config);
    assert_eq!(service.config.target_size, 512 * 1024 * 1024);
    assert!(service.config.dry_run);
}

#[test]
fn test_parse_partition_key_empty() {
    let result = parse_partition_key("");
    assert!(result.is_empty());
}

#[test]
fn test_parse_partition_key_single() {
    let result = parse_partition_key("year=2024");
    assert_eq!(result.len(), 1);
    assert_eq!(result.get("year"), Some(&"2024".to_string()));
}

#[test]
fn test_parse_partition_key_multiple() {
    let result = parse_partition_key("year=2024/month=12/day=15");
    assert_eq!(result.len(), 3);
    assert_eq!(result.get("year"), Some(&"2024".to_string()));
    assert_eq!(result.get("month"), Some(&"12".to_string()));
    assert_eq!(result.get("day"), Some(&"15".to_string()));
}

#[test]
fn test_optimal_subgroup_size_small_files() {
    // Small files (<1MB avg) should get groups of ~100
    let size = calculate_optimal_subgroup_size(1000, 500_000_000, 4);
    assert!((50..=125).contains(&size));
}

#[test]
fn test_optimal_subgroup_size_medium_files() {
    // Medium files (10-50MB avg) should get larger groups
    let size = calculate_optimal_subgroup_size(100, 2_500_000_000, 4);
    assert!(size >= 50);
}

#[test]
fn test_optimal_subgroup_size_respects_parallelism() {
    // With high parallelism, should ensure enough groups
    let size = calculate_optimal_subgroup_size(100, 100_000_000, 16);
    // Should have at least 32 groups (2x parallelism), so max 3 files per group
    // But min is 50, so should be 50
    assert_eq!(size, 50);
}

#[test]
fn test_subdivide_groups_small_group() {
    let files: Vec<DataFileInfo> = (0..10)
        .map(|i| make_file(&format!("file{}.parquet", i), 1000, HashMap::new()))
        .collect();
    let group = FileGroup {
        files,
        total_size: 10000,
        total_records: 1000,
        partition_key: "".to_string(),
    };

    let result = subdivide_groups(vec![group], 100);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].files.len(), 10);
}

#[test]
fn test_subdivide_groups_large_group() {
    let files: Vec<DataFileInfo> = (0..250)
        .map(|i| make_file(&format!("file{}.parquet", i), 1000, HashMap::new()))
        .collect();
    let group = FileGroup {
        files,
        total_size: 250000,
        total_records: 25000,
        partition_key: "".to_string(),
    };

    let result = subdivide_groups(vec![group], 100);
    assert_eq!(result.len(), 3); // 250/100 = 3 subgroups
    assert_eq!(result[0].files.len(), 100);
    assert_eq!(result[1].files.len(), 100);
    assert_eq!(result[2].files.len(), 50);
    assert!(result[0].partition_key.contains("__subgroup_0"));
    assert!(result[1].partition_key.contains("__subgroup_1"));
}

#[test]
fn test_subdivide_groups_with_partition() {
    let files: Vec<DataFileInfo> = (0..150)
        .map(|i| {
            let mut partition = HashMap::new();
            partition.insert("year".to_string(), "2024".to_string());
            make_file(&format!("file{}.parquet", i), 1000, partition)
        })
        .collect();
    let group = FileGroup {
        files,
        total_size: 150000,
        total_records: 15000,
        partition_key: "year=2024".to_string(),
    };

    let result = subdivide_groups(vec![group], 100);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].partition_key, "year=2024/__subgroup_0");
    assert_eq!(result[1].partition_key, "year=2024/__subgroup_1");
}

#[test]
fn test_analyze_empty_files() {
    let service = OptimizeService::new();
    let result = service.analyze(&[]);
    assert!(result.is_empty());
}

#[test]
fn test_analyze_no_small_files() {
    let service = OptimizeService::new();
    // Files larger than min_size shouldn't be compacted
    let files = vec![
        make_file("file1.parquet", 100_000_000, HashMap::new()), // 100MB
        make_file("file2.parquet", 200_000_000, HashMap::new()), // 200MB
    ];
    let result = service.analyze(&files);
    assert!(result.is_empty());
}

#[test]
fn test_analyze_finds_small_files() {
    let service = OptimizeService::new();
    // Files smaller than min_size (16MB default) should be compacted
    let files = vec![
        make_file("file1.parquet", 1_000_000, HashMap::new()), // 1MB
        make_file("file2.parquet", 2_000_000, HashMap::new()), // 2MB
        make_file("file3.parquet", 3_000_000, HashMap::new()), // 3MB
    ];
    let result = service.analyze(&files);
    assert_eq!(result.len(), 1); // One group (no partitions)
    assert_eq!(result[0].files.len(), 3);
}

#[test]
fn test_analyze_with_partition_filter() {
    let config = MaintenanceConfig {
        partition_filter: Some("year=2024".to_string()),
        ..Default::default()
    };
    let service = OptimizeService::with_config(config);

    let mut part_2024 = HashMap::new();
    part_2024.insert("year".to_string(), "2024".to_string());

    let mut part_2023 = HashMap::new();
    part_2023.insert("year".to_string(), "2023".to_string());

    let files = vec![
        make_file("data/year=2024/file1.parquet", 1_000_000, part_2024.clone()),
        make_file("data/year=2024/file2.parquet", 2_000_000, part_2024.clone()),
        make_file("data/year=2023/file1.parquet", 1_000_000, part_2023.clone()),
    ];

    let result = service.analyze(&files);
    // Should only find year=2024 partition (2 files), not year=2023
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].files.len(), 2);
}
