//! Unit tests for CLI commands
//!
//! Tests for utility functions, argument parsing, and command behavior.

#[cfg(test)]
mod common_tests {
    use super::super::common::extract_table_name;

    #[test]
    fn test_extract_table_name_simple() {
        assert_eq!(extract_table_name("s3://bucket/database/events"), "events");
    }

    #[test]
    fn test_extract_table_name_with_trailing_slash() {
        assert_eq!(extract_table_name("s3://bucket/database/events/"), "events");
    }

    #[test]
    fn test_extract_table_name_nested_path() {
        assert_eq!(
            extract_table_name("s3://bucket/warehouse/database/schema/orders"),
            "orders"
        );
    }

    #[test]
    fn test_extract_table_name_local_path() {
        assert_eq!(extract_table_name("/home/user/data/my_table"), "my_table");
    }

    #[test]
    fn test_extract_table_name_single_component() {
        assert_eq!(extract_table_name("table_name"), "table_name");
    }

    #[test]
    fn test_extract_table_name_empty_returns_empty() {
        // Empty string returns empty (rsplit returns empty for empty input)
        assert_eq!(extract_table_name(""), "");
    }

    #[test]
    fn test_extract_table_name_only_slashes() {
        // After trimming slashes, empty string remains
        assert_eq!(extract_table_name("///"), "");
    }
}

#[cfg(test)]
mod parser_tests {
    use crate::cli::parser::parse_key_value;

    #[test]
    fn test_parse_key_value_simple() {
        let (key, value) = parse_key_value("foo=bar").unwrap();
        assert_eq!(key, "foo");
        assert_eq!(value, "bar");
    }

    #[test]
    fn test_parse_key_value_with_equals_in_value() {
        let (key, value) = parse_key_value("url=https://example.com?a=b").unwrap();
        assert_eq!(key, "url");
        assert_eq!(value, "https://example.com?a=b");
    }

    #[test]
    fn test_parse_key_value_empty_value() {
        let (key, value) = parse_key_value("key=").unwrap();
        assert_eq!(key, "key");
        assert_eq!(value, "");
    }

    #[test]
    fn test_parse_key_value_no_equals_fails() {
        let result = parse_key_value("no_equals");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no `=` found"));
    }

    #[test]
    fn test_parse_key_value_whitespace_preserved() {
        let (key, value) = parse_key_value("key = value with spaces").unwrap();
        assert_eq!(key, "key ");
        assert_eq!(value, " value with spaces");
    }
}

#[cfg(test)]
mod constants_tests {
    use super::super::constants::*;

    #[test]
    fn test_memory_constants_ordering() {
        // Memory constants should be in ascending order
        // These are compile-time constants, so we verify the ordering explicitly
        // rather than relying on assert! which gets optimized away
        let values = [
            ("CONFIG_OPS", MEMORY_CONFIG_OPS),
            ("INIT_OPS", MEMORY_INIT_OPS),
            ("LIGHT_OPS", MEMORY_LIGHT_OPS),
            ("MEDIUM_OPS", MEMORY_MEDIUM_OPS),
            ("HEAVY_OPS", MEMORY_HEAVY_OPS),
            ("INTENSIVE_OPS", MEMORY_INTENSIVE_OPS),
        ];

        for window in values.windows(2) {
            assert!(
                window[0].1 < window[1].1,
                "{} ({}) should be less than {} ({})",
                window[0].0,
                window[0].1,
                window[1].0,
                window[1].1
            );
        }
    }

    #[test]
    fn test_memory_constants_values() {
        // Verify specific values
        assert_eq!(MEMORY_CONFIG_OPS, 8 * 1024 * 1024); // 8 MB
        assert_eq!(MEMORY_INIT_OPS, 16 * 1024 * 1024); // 16 MB
        assert_eq!(MEMORY_LIGHT_OPS, 32 * 1024 * 1024); // 32 MB
        assert_eq!(MEMORY_MEDIUM_OPS, 64 * 1024 * 1024); // 64 MB
        assert_eq!(MEMORY_HEAVY_OPS, 128 * 1024 * 1024); // 128 MB
        assert_eq!(MEMORY_INTENSIVE_OPS, 256 * 1024 * 1024); // 256 MB
    }
}

#[cfg(test)]
mod cli_parse_tests {
    use crate::cli::parser::Cli;
    use clap::Parser;

    #[test]
    fn test_cli_parse_help() {
        // Help flag should trigger help action (we can't test the actual output easily)
        let result = Cli::try_parse_from(["icetable", "--help"]);
        // Help flag causes early exit, so this should error with a specific kind
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_parse_version() {
        let result = Cli::try_parse_from(["icetable", "--version"]);
        assert!(result.is_err()); // Version also causes early exit
    }

    #[test]
    fn test_cli_parse_global_table_option() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "describe"]).unwrap();

        assert_eq!(cli.table, Some("my_table".to_string()));
    }

    #[test]
    fn test_cli_parse_global_namespace_option() {
        let cli = Cli::try_parse_from([
            "icetable",
            "-n",
            "my_namespace",
            "-t",
            "my_table",
            "describe",
        ])
        .unwrap();

        assert_eq!(cli.namespace, Some("my_namespace".to_string()));
        assert_eq!(cli.table, Some("my_table".to_string()));
    }

    #[test]
    fn test_cli_parse_global_catalog_option() {
        let cli = Cli::try_parse_from([
            "icetable",
            "-c",
            "production",
            "-n",
            "analytics",
            "-t",
            "events",
            "describe",
        ])
        .unwrap();

        assert_eq!(cli.catalog, Some("production".to_string()));
        assert_eq!(cli.namespace, Some("analytics".to_string()));
        assert_eq!(cli.table, Some("events".to_string()));
    }

    #[test]
    fn test_cli_parse_quiet_flag() {
        let cli = Cli::try_parse_from(["icetable", "-q", "ls"]).unwrap();

        assert!(cli.quiet);
    }

    #[test]
    fn test_cli_parse_long_options() {
        let cli = Cli::try_parse_from([
            "icetable",
            "--table",
            "orders",
            "--namespace",
            "sales",
            "--catalog",
            "prod",
            "--warehouse",
            "us-east",
            "describe",
        ])
        .unwrap();

        assert_eq!(cli.table, Some("orders".to_string()));
        assert_eq!(cli.namespace, Some("sales".to_string()));
        assert_eq!(cli.catalog, Some("prod".to_string()));
        assert_eq!(cli.warehouse, Some("us-east".to_string()));
    }

    #[test]
    fn test_cli_table_context() {
        let cli = Cli::try_parse_from([
            "icetable",
            "-c",
            "my_catalog",
            "-n",
            "my_namespace",
            "-t",
            "my_table",
            "describe",
        ])
        .unwrap();

        let ctx = cli.table_context();
        assert_eq!(ctx.table, Some("my_table".to_string()));
        assert_eq!(ctx.namespace, Some("my_namespace".to_string()));
        assert_eq!(ctx.catalog, Some("my_catalog".to_string()));
    }

    #[test]
    fn test_cli_catalog_config_with_uri() {
        let cli = Cli::try_parse_from([
            "icetable",
            "--catalog-uri",
            "https://polaris.example.com",
            "--catalog-warehouse",
            "my_warehouse",
            "ls",
        ])
        .unwrap();

        let config = cli.catalog_config();
        assert!(config.is_some());
        let config = config.unwrap();
        assert_eq!(config.uri, "https://polaris.example.com");
    }

    #[test]
    fn test_cli_catalog_config_without_uri() {
        let cli = Cli::try_parse_from(["icetable", "ls"]).unwrap();

        let config = cli.catalog_config();
        assert!(config.is_none());
    }
}

#[cfg(test)]
mod describe_args_tests {
    use crate::cli::parser::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn test_describe_default_args() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "describe"]).unwrap();

        if let Commands::Describe(args) = cli.command {
            assert!(!args.schema);
            assert!(!args.stats);
            assert!(!args.health);
            assert_eq!(args.output, "text");
        } else {
            panic!("Expected Describe command");
        }
    }

    #[test]
    fn test_describe_schema_flag() {
        let cli =
            Cli::try_parse_from(["icetable", "-t", "my_table", "describe", "--schema"]).unwrap();

        if let Commands::Describe(args) = cli.command {
            assert!(args.schema);
        } else {
            panic!("Expected Describe command");
        }
    }

    #[test]
    fn test_describe_json_output() {
        let cli =
            Cli::try_parse_from(["icetable", "-t", "my_table", "describe", "--output", "json"])
                .unwrap();

        if let Commands::Describe(args) = cli.command {
            assert_eq!(args.output, "json");
        } else {
            panic!("Expected Describe command");
        }
    }

    #[test]
    fn test_describe_alias_d() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "d"]).unwrap();

        assert!(matches!(cli.command, Commands::Describe(_)));
    }

    #[test]
    fn test_describe_with_snapshot() {
        let cli = Cli::try_parse_from([
            "icetable",
            "-t",
            "my_table",
            "describe",
            "--snapshot",
            "12345",
        ])
        .unwrap();

        if let Commands::Describe(args) = cli.command {
            assert_eq!(args.snapshot, Some(12345));
        } else {
            panic!("Expected Describe command");
        }
    }
}

#[cfg(test)]
mod ls_args_tests {
    use crate::cli::parser::{Cli, Commands, LsCommands};
    use clap::Parser;

    #[test]
    fn test_ls_default_args() {
        let cli = Cli::try_parse_from(["icetable", "ls"]).unwrap();

        if let Commands::Ls(args) = cli.command {
            assert!(args.command.is_none());
            assert_eq!(args.output, "text");
        } else {
            panic!("Expected Ls command");
        }
    }

    #[test]
    fn test_ls_namespaces() {
        let cli = Cli::try_parse_from(["icetable", "ls", "namespaces"]).unwrap();

        if let Commands::Ls(args) = cli.command {
            assert!(matches!(args.command, Some(LsCommands::Namespaces)));
        } else {
            panic!("Expected Ls command");
        }
    }

    #[test]
    fn test_ls_tables() {
        let cli = Cli::try_parse_from(["icetable", "ls", "tables"]).unwrap();

        if let Commands::Ls(args) = cli.command {
            assert!(matches!(args.command, Some(LsCommands::Tables)));
        } else {
            panic!("Expected Ls command");
        }
    }

    #[test]
    fn test_ls_json_output() {
        let cli = Cli::try_parse_from(["icetable", "ls", "--output", "json"]).unwrap();

        if let Commands::Ls(args) = cli.command {
            assert_eq!(args.output, "json");
        } else {
            panic!("Expected Ls command");
        }
    }
}

#[cfg(test)]
mod optimize_args_tests {
    use crate::cli::parser::{Cli, Commands, OptimizeCommands};
    use clap::Parser;

    #[test]
    fn test_optimize_data_default() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "optimize", "data"]).unwrap();

        if let Commands::Optimize(OptimizeCommands::Data(args)) = cli.command {
            assert!(!args.dry_run); // dry_run defaults to false
            assert_eq!(args.target_size, 268435456); // 256MB default
        } else {
            panic!("Expected Optimize Data command");
        }
    }

    #[test]
    fn test_optimize_data_dry_run() {
        let cli = Cli::try_parse_from([
            "icetable",
            "-t",
            "my_table",
            "optimize",
            "data",
            "--dry-run",
        ])
        .unwrap();

        if let Commands::Optimize(OptimizeCommands::Data(args)) = cli.command {
            assert!(args.dry_run);
        } else {
            panic!("Expected Optimize Data command");
        }
    }

    #[test]
    fn test_optimize_vacuum_default() {
        let cli =
            Cli::try_parse_from(["icetable", "-t", "my_table", "optimize", "vacuum"]).unwrap();

        if let Commands::Optimize(OptimizeCommands::Vacuum(args)) = cli.command {
            assert!(!args.dry_run); // dry_run defaults to false
            assert_eq!(args.retention_hours, 168); // 7 days default
        } else {
            panic!("Expected Optimize Vacuum command");
        }
    }

    #[test]
    fn test_optimize_vacuum_with_retention() {
        let cli = Cli::try_parse_from([
            "icetable",
            "-t",
            "my_table",
            "optimize",
            "vacuum",
            "--retention-hours",
            "24",
        ])
        .unwrap();

        if let Commands::Optimize(OptimizeCommands::Vacuum(args)) = cli.command {
            assert_eq!(args.retention_hours, 24);
        } else {
            panic!("Expected Optimize Vacuum command");
        }
    }

    #[test]
    fn test_optimize_alias_o() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "o", "data"]).unwrap();

        assert!(matches!(cli.command, Commands::Optimize(_)));
    }
}

#[cfg(test)]
mod snapshot_args_tests {
    use crate::cli::parser::{Cli, Commands, SnapshotCommands};
    use clap::Parser;

    #[test]
    fn test_snapshot_list_default() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "snapshot", "ls"]).unwrap();

        if let Commands::Snapshot(args) = cli.command {
            if let SnapshotCommands::Ls(ls_args) = args.command {
                assert_eq!(ls_args.limit, 10);
                assert!(!ls_args.all);
            } else {
                panic!("Expected Snapshot List command");
            }
        } else {
            panic!("Expected Snapshot command");
        }
    }

    #[test]
    fn test_snapshot_list_with_limit() {
        let cli = Cli::try_parse_from([
            "icetable", "-t", "my_table", "snapshot", "ls", "--limit", "50",
        ])
        .unwrap();

        if let Commands::Snapshot(args) = cli.command {
            if let SnapshotCommands::Ls(ls_args) = args.command {
                assert_eq!(ls_args.limit, 50);
            } else {
                panic!("Expected Snapshot List command");
            }
        } else {
            panic!("Expected Snapshot command");
        }
    }

    #[test]
    fn test_snapshot_alias_s() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "s", "ls"]).unwrap();

        assert!(matches!(cli.command, Commands::Snapshot(_)));
    }
}

#[cfg(test)]
mod history_args_tests {
    use crate::cli::parser::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn test_history_default() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "history"]).unwrap();

        if let Commands::History(args) = cli.command {
            assert_eq!(args.limit, 10); // Default limit
        } else {
            panic!("Expected History command");
        }
    }

    #[test]
    fn test_history_with_limit() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "history", "--limit", "50"])
            .unwrap();

        if let Commands::History(args) = cli.command {
            assert_eq!(args.limit, 50);
        } else {
            panic!("Expected History command");
        }
    }

    #[test]
    fn test_history_alias_h() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "h"]).unwrap();

        assert!(matches!(cli.command, Commands::History(_)));
    }
}

#[cfg(test)]
mod branch_tag_args_tests {
    use crate::cli::parser::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn test_branch_list() {
        let cli = Cli::try_parse_from([
            "icetable", "-t", "my_table", "branch", "ls", // Correct subcommand is 'ls'
        ])
        .unwrap();

        assert!(matches!(cli.command, Commands::Branch(_)));
    }

    #[test]
    fn test_tag_list() {
        let cli = Cli::try_parse_from([
            "icetable", "-t", "my_table", "tag", "ls", // Correct subcommand is 'ls'
        ])
        .unwrap();

        assert!(matches!(cli.command, Commands::Tag(_)));
    }
}

#[cfg(test)]
mod repair_args_tests {
    use crate::cli::parser::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn test_repair_sync_metadata() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "repair", "--sync-metadata"])
            .unwrap();

        if let Commands::Repair(args) = cli.command {
            assert!(args.sync_metadata);
            assert!(!args.dry_run); // dry_run defaults to false
        } else {
            panic!("Expected Repair command");
        }
    }

    #[test]
    fn test_repair_dry_run() {
        let cli = Cli::try_parse_from([
            "icetable",
            "-t",
            "my_table",
            "repair",
            "--sync-metadata",
            "--dry-run",
        ])
        .unwrap();

        if let Commands::Repair(args) = cli.command {
            assert!(args.dry_run);
        } else {
            panic!("Expected Repair command");
        }
    }

    #[test]
    fn test_repair_add_orphans() {
        let cli =
            Cli::try_parse_from(["icetable", "-t", "my_table", "repair", "--add-orphans"]).unwrap();

        if let Commands::Repair(args) = cli.command {
            assert!(args.add_orphans);
        } else {
            panic!("Expected Repair command");
        }
    }

    #[test]
    fn test_repair_remove_missing() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "repair", "--remove-missing"])
            .unwrap();

        if let Commands::Repair(args) = cli.command {
            assert!(args.remove_missing);
        } else {
            panic!("Expected Repair command");
        }
    }
}

#[cfg(test)]
mod generate_args_tests {
    use crate::cli::parser::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn test_generate_default_rows() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "generate"]).unwrap();

        if let Commands::Generate(args) = cli.command {
            assert_eq!(args.rows, 10000); // Default rows is 10000
            assert_eq!(args.files, 1); // Default files is 1
        } else {
            panic!("Expected Generate command");
        }
    }

    #[test]
    fn test_generate_custom_rows() {
        let cli =
            Cli::try_parse_from(["icetable", "-t", "my_table", "generate", "--rows", "50000"])
                .unwrap();

        if let Commands::Generate(args) = cli.command {
            assert_eq!(args.rows, 50000);
        } else {
            panic!("Expected Generate command");
        }
    }

    #[test]
    fn test_generate_multiple_files() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "generate", "--files", "5"])
            .unwrap();

        if let Commands::Generate(args) = cli.command {
            assert_eq!(args.files, 5);
        } else {
            panic!("Expected Generate command");
        }
    }
}

#[cfg(test)]
mod create_delete_args_tests {
    use crate::cli::parser::{Cli, Commands, CreateCommands, DeleteCommands};
    use clap::Parser;

    #[test]
    fn test_create_namespace() {
        let cli =
            Cli::try_parse_from(["icetable", "-c", "prod", "create", "namespace", "analytics"])
                .unwrap();

        if let Commands::Create(args) = cli.command {
            assert!(matches!(args.command, CreateCommands::Namespace(_)));
        } else {
            panic!("Expected Create command");
        }
    }

    #[test]
    fn test_delete_namespace_with_force() {
        let cli = Cli::try_parse_from([
            "icetable",
            "-c",
            "prod",
            "delete",
            "namespace",
            "old_ns",
            "--force",
        ])
        .unwrap();

        if let Commands::Delete(args) = cli.command {
            if let DeleteCommands::Namespace(ns_args) = args.command {
                assert!(ns_args.force);
                assert_eq!(ns_args.name, "old_ns");
            } else {
                panic!("Expected Delete Namespace command");
            }
        } else {
            panic!("Expected Delete command");
        }
    }

    #[test]
    fn test_delete_table() {
        let cli = Cli::try_parse_from([
            "icetable",
            "-c",
            "prod",
            "-n",
            "analytics",
            "delete",
            "table",
            "old_table",
        ])
        .unwrap();

        if let Commands::Delete(args) = cli.command {
            if let DeleteCommands::Table(table_args) = args.command {
                assert_eq!(table_args.names, vec!["old_table"]);
            } else {
                panic!("Expected Delete Table command");
            }
        } else {
            panic!("Expected Delete command");
        }
    }
}

#[cfg(test)]
mod diff_args_tests {
    use crate::cli::parser::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn test_diff_with_reference() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "diff", "123"]).unwrap();

        if let Commands::Diff(args) = cli.command {
            assert_eq!(args.reference, Some("123".to_string()));
        } else {
            panic!("Expected Diff command");
        }
    }

    #[test]
    fn test_diff_with_base() {
        let cli =
            Cli::try_parse_from(["icetable", "-t", "my_table", "diff", "456", "--base", "123"])
                .unwrap();

        if let Commands::Diff(args) = cli.command {
            assert_eq!(args.reference, Some("456".to_string()));
            assert_eq!(args.base, Some("123".to_string()));
        } else {
            panic!("Expected Diff command");
        }
    }
}

#[cfg(test)]
mod validate_args_tests {
    use crate::cli::parser::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn test_validate_default() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "validate"]).unwrap();

        if let Commands::Validate(args) = cli.command {
            assert!(!args.quick);
            assert!(!args.strict);
            assert!(!args.fix);
            assert_eq!(args.output, "text");
        } else {
            panic!("Expected Validate command");
        }
    }

    #[test]
    fn test_validate_quick() {
        let cli =
            Cli::try_parse_from(["icetable", "-t", "my_table", "validate", "--quick"]).unwrap();

        if let Commands::Validate(args) = cli.command {
            assert!(args.quick);
        } else {
            panic!("Expected Validate command");
        }
    }

    #[test]
    fn test_validate_strict() {
        let cli =
            Cli::try_parse_from(["icetable", "-t", "my_table", "validate", "--strict"]).unwrap();

        if let Commands::Validate(args) = cli.command {
            assert!(args.strict);
        } else {
            panic!("Expected Validate command");
        }
    }

    #[test]
    fn test_validate_with_fix() {
        let cli = Cli::try_parse_from(["icetable", "-t", "my_table", "validate", "--fix"]).unwrap();

        if let Commands::Validate(args) = cli.command {
            assert!(args.fix);
        } else {
            panic!("Expected Validate command");
        }
    }
}

#[cfg(test)]
mod import_args_tests {
    use crate::cli::parser::{Cli, Commands, ImportCommands};
    use clap::Parser;

    #[test]
    fn test_import_parquet() {
        let cli = Cli::try_parse_from([
            "icetable",
            "import",
            "parquet",
            "/path/to/data.parquet",
            "/path/to/target_table", // TARGET is required
        ])
        .unwrap();

        if let Commands::Import(ImportCommands::Parquet(args)) = cli.command {
            assert_eq!(args.source, "/path/to/data.parquet");
            assert_eq!(args.target, "/path/to/target_table");
        } else {
            panic!("Expected Import Parquet command");
        }
    }

    #[test]
    fn test_import_delta() {
        let cli = Cli::try_parse_from([
            "icetable",
            "import",
            "delta",
            "/path/to/delta_table",
            "/path/to/iceberg_table", // TARGET is required
        ])
        .unwrap();

        if let Commands::Import(ImportCommands::Delta(args)) = cli.command {
            assert_eq!(args.source, "/path/to/delta_table");
            assert_eq!(args.target, "/path/to/iceberg_table");
        } else {
            panic!("Expected Import Delta command");
        }
    }

    #[test]
    fn test_import_parquet_dry_run() {
        let cli = Cli::try_parse_from([
            "icetable",
            "import",
            "parquet",
            "/source",
            "/target",
            "--dry-run",
        ])
        .unwrap();

        if let Commands::Import(ImportCommands::Parquet(args)) = cli.command {
            assert!(args.dry_run);
        } else {
            panic!("Expected Import Parquet command");
        }
    }
}

#[cfg(test)]
mod admin_args_tests {
    use crate::cli::parser::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn test_admin_config_ls() {
        let cli = Cli::try_parse_from([
            "icetable", "admin", "config", "ls", // Config requires a subcommand
        ])
        .unwrap();

        assert!(matches!(cli.command, Commands::Admin(_)));
    }

    #[test]
    fn test_admin_warehouse_ls() {
        let cli = Cli::try_parse_from([
            "icetable",
            "admin",
            "warehouse",
            "ls", // Correct subcommand is 'ls'
        ])
        .unwrap();

        assert!(matches!(cli.command, Commands::Admin(_)));
    }

    #[test]
    fn test_admin_auth_status() {
        let cli = Cli::try_parse_from(["icetable", "admin", "auth", "status"]).unwrap();

        assert!(matches!(cli.command, Commands::Admin(_)));
    }
}

#[cfg(test)]
mod doctor_args_tests {
    use crate::cli::parser::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn test_doctor_command() {
        let cli = Cli::try_parse_from(["icetable", "doctor"]).unwrap();

        assert!(matches!(cli.command, Commands::Doctor(_)));
    }
}
