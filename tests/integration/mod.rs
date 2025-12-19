//! Integration test modules
//!
//! Test categories:
//! - `catalog_commit_tests`: Catalog committer unit tests
//! - `core_tests`: Core utility and format detection tests
//! - `maintenance_tests`: Maintenance operations tests
//! - `minio_tests`: MinIO S3 integration tests
//! - `cli_commands_tests`: E2E tests for CLI commands
//! - `error_handling_tests`: Error path and edge case tests
//! - `property_tests`: Property-based tests with proptest
//! - `snapshot_tests`: Snapshot tests for output formatting
//! - `stress_tests`: Performance and stress tests

mod catalog_commit_tests;
mod cli_commands_tests;
mod core_tests;
mod error_handling_tests;
mod maintenance_tests;
mod minio_tests;
mod property_tests;
mod snapshot_tests;
mod stress_tests;
