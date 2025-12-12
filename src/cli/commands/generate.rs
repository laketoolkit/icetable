//! Generate command implementation
//!
//! Generates synthetic test data into Iceberg tables via REST catalogs.

use std::io::{self, Write};
use std::sync::Arc;

use colored::Colorize;

use super::common::{no_namespace_error, no_table_error, print_json, resolve_catalog};
use crate::cli::parser::{CliTableContext, GenerateArgs, SchemaTemplate as CliSchemaTemplate};
use crate::core::format_bytes;
use crate::core::operations::generate::{
    ExistingTableInfo, GenerateOperation, GenerateResult, SchemaTemplate, parse_schema_string,
};
use crate::error::Result;

/// Handler for generate command
pub struct GenerateCommand;

impl GenerateCommand {
    /// Execute generate command
    pub async fn execute(args: GenerateArgs, ctx: &CliTableContext) -> Result<()> {
        // Resolve catalog context (error propagates with full context)
        let catalog = resolve_catalog(ctx, args.catalog.as_deref()).await?;

        // Must have namespace
        let namespace = catalog.namespace().ok_or_else(no_namespace_error)?;

        // Must have table
        let table_name = catalog.table().ok_or_else(no_table_error)?;

        // Resolve schema from args
        let arrow_schema = Self::resolve_schema(&args)?;

        if args.dry_run {
            return Self::print_dry_run(&args, namespace, table_name, &arrow_schema);
        }

        // Check if table exists
        let table_exists = catalog.table_exists(table_name).await?;

        // Get or create table via catalog
        let table = if table_exists {
            // Load existing table
            let table = catalog.load_table(table_name).await?;
            let location = table.metadata().location().to_string();

            // Check existing table info for append confirmation
            if !args.force {
                let existing_info = GenerateOperation::table_exists(&location).await;
                if let Some(ref info) = existing_info
                    && !Self::confirm_append(info)?
                {
                    println!("{}", "Operation cancelled.".yellow());
                    return Ok(());
                }
            }
            table
        } else {
            // Create new table via catalog
            let iceberg_schema = Self::arrow_to_iceberg_schema(&arrow_schema)?;
            catalog
                .create_table(table_name, iceberg_schema, None, Default::default())
                .await?
        };

        let table_location = table.metadata().location().to_string();

        let action = if table_exists {
            "Appending to"
        } else {
            "Generating"
        };

        println!(
            "{} {} {}.{} ...",
            "->".cyan().bold(),
            action,
            namespace.cyan(),
            table_name.cyan()
        );
        println!("  Location: {}", table_location.dimmed());
        println!("  Schema: {} columns", arrow_schema.fields().len());
        println!("  Rows: {}", args.rows);
        println!("  Files: {}", args.files);

        // Execute with catalog for proper commits
        let result = GenerateOperation::execute_with_catalog(
            table,
            catalog.catalog(),
            Arc::new(arrow_schema),
            args.rows,
            args.files,
            args.seed.unwrap_or(42),
        )
        .await?;

        Self::print_result(&result, &args.output);

        Ok(())
    }

    /// Prompt user for confirmation before appending to an existing table
    fn confirm_append(table_info: &ExistingTableInfo) -> Result<bool> {
        println!();
        println!(
            "{} An Iceberg table already exists at this location:",
            "!".yellow().bold()
        );
        println!(
            "  Snapshots: {}",
            table_info.snapshot_count.to_string().cyan()
        );
        println!(
            "  Data files: {}",
            table_info.data_file_count.to_string().cyan()
        );
        println!(
            "  Total records: {}",
            table_info.total_records.to_string().cyan()
        );
        println!();
        println!(
            "{}",
            "You are about to append new data to this table.".yellow()
        );
        print!("Do you want to continue? [y/N] ");
        io::stdout().flush().map_err(crate::error::Error::Io)?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(crate::error::Error::Io)?;

        let input = input.trim().to_lowercase();
        Ok(input == "y" || input == "yes")
    }

    fn resolve_schema(args: &GenerateArgs) -> Result<arrow::datatypes::Schema> {
        if let Some(schema_str) = &args.schema {
            parse_schema_string(schema_str)
        } else if let Some(template) = &args.template {
            Ok(Self::cli_template_to_core(*template).to_schema())
        } else {
            // Default: events template
            Ok(SchemaTemplate::Events.to_schema())
        }
    }

    fn cli_template_to_core(cli_template: CliSchemaTemplate) -> SchemaTemplate {
        match cli_template {
            CliSchemaTemplate::Events => SchemaTemplate::Events,
            CliSchemaTemplate::Transactions => SchemaTemplate::Transactions,
            CliSchemaTemplate::Sensors => SchemaTemplate::Sensors,
            CliSchemaTemplate::Users => SchemaTemplate::Users,
            CliSchemaTemplate::WebLogs => SchemaTemplate::WebLogs,
        }
    }

    fn print_dry_run(
        args: &GenerateArgs,
        namespace: &str,
        table_name: &str,
        schema: &arrow::datatypes::Schema,
    ) -> Result<()> {
        let rows_per_file = (args.rows / args.files as u64).max(1);
        let partition_cols = args.partition_by.clone().unwrap_or_default();

        println!("{} Dry run - no data will be generated", "->".cyan().bold());
        println!();
        println!("Configuration:");
        println!("  Table: {}.{}", namespace, table_name);
        println!("  Total rows: {}", args.rows);
        println!("  Files: {}", args.files);
        println!("  Rows per file: ~{}", rows_per_file);
        println!(
            "  Seed: {}",
            args.seed
                .map(|s| s.to_string())
                .unwrap_or_else(|| "42 (default)".to_string())
        );
        println!();
        println!("Schema ({} columns):", schema.fields().len());
        for field in schema.fields() {
            println!(
                "  - {}: {} {}",
                field.name(),
                format!("{:?}", field.data_type()).to_lowercase(),
                if field.is_nullable() {
                    "(nullable)"
                } else {
                    "(required)"
                }
            );
        }

        if !partition_cols.is_empty() {
            println!();
            println!("Partitioning:");
            for col in &partition_cols {
                println!("  - {}", col);
            }
        }

        Ok(())
    }

    fn print_result(result: &GenerateResult, output: &str) {
        for file in &result.data_files {
            println!(
                "  {} Written {} ({} rows, {} bytes)",
                "✓".green(),
                file.path.split('/').next_back().unwrap_or(&file.path),
                file.record_count,
                file.size
            );
        }

        let metadata_action = if result.appended {
            "Created new snapshot"
        } else {
            "Created Iceberg metadata"
        };
        println!(
            "  {} {} (snapshot {})",
            "✓".green(),
            metadata_action,
            result.snapshot_id
        );

        let action = if result.appended {
            "Appended"
        } else {
            "Generated table with"
        };
        println!(
            "\n{} {} {} rows in {} files ({})",
            "✓".green().bold(),
            action,
            result.total_rows,
            result.files_created,
            format_bytes(result.total_bytes)
        );

        if output == "json" {
            let json_result = serde_json::json!({
                "status": "success",
                "location": result.table_path,
                "rows": result.total_rows,
                "files": result.files_created,
                "total_bytes": result.total_bytes,
                "data_files": result.data_files.iter().map(|f| &f.path).collect::<Vec<_>>(),
                "metadata_path": result.metadata_path,
                "snapshot_id": result.snapshot_id,
                "appended": result.appended
            });
            if let Err(e) = print_json(&json_result) {
                eprintln!("Error serializing JSON: {}", e);
            }
        }
    }

    /// Convert Arrow schema to Iceberg schema
    fn arrow_to_iceberg_schema(
        arrow_schema: &arrow::datatypes::Schema,
    ) -> Result<iceberg::spec::Schema> {
        use arrow::datatypes::DataType;
        use iceberg::spec::{NestedField, PrimitiveType, Schema, Type};

        let mut fields = Vec::new();
        for (idx, field) in arrow_schema.fields().iter().enumerate() {
            let iceberg_type = match field.data_type() {
                DataType::Boolean => Type::Primitive(PrimitiveType::Boolean),
                DataType::Int8 | DataType::Int16 | DataType::Int32 => {
                    Type::Primitive(PrimitiveType::Int)
                }
                DataType::Int64 => Type::Primitive(PrimitiveType::Long),
                DataType::Float32 => Type::Primitive(PrimitiveType::Float),
                DataType::Float64 => Type::Primitive(PrimitiveType::Double),
                DataType::Utf8 | DataType::LargeUtf8 => Type::Primitive(PrimitiveType::String),
                DataType::Binary | DataType::LargeBinary => Type::Primitive(PrimitiveType::Binary),
                DataType::Date32 | DataType::Date64 => Type::Primitive(PrimitiveType::Date),
                DataType::Timestamp(_, _) => Type::Primitive(PrimitiveType::Timestamp),
                DataType::Time32(_) | DataType::Time64(_) => Type::Primitive(PrimitiveType::Time),
                _ => Type::Primitive(PrimitiveType::String), // Fallback
            };

            let nested_field = if field.is_nullable() {
                NestedField::optional(idx as i32 + 1, field.name(), iceberg_type)
            } else {
                NestedField::required(idx as i32 + 1, field.name(), iceberg_type)
            };
            fields.push(nested_field.into());
        }

        Schema::builder().with_fields(fields).build().map_err(|e| {
            crate::error::Error::SchemaValidation {
                message: format!("Failed to build Iceberg schema: {}", e),
            }
        })
    }
}
