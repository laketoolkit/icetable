//! icetable CLI entry point

use clap::Parser;
use colored::Colorize;
use std::process;

use icetable::cli::commands::*;
use icetable::cli::parser::{Cli, Commands, ImportCommands};
use icetable::utils::{ResourceLimits, init_resource_limits, is_cancelled};

/// Print all global options (kubectl style)
fn print_global_options() {
    println!("The following options can be passed to any command:\n");

    println!("{}", "Global Options:".bold());
    println!(
        "  -t, --table <TABLE>            Table name or path (e.g., \"namespace.table\" or \"s3://bucket/path\")"
    );
    println!("  -n, --namespace <NAMESPACE>    Namespace (e.g., \"db.schema\")");
    println!(
        "  -c, --catalog <CATALOG>        Catalog name (from config, e.g., \"polaris\", \"nessie\")"
    );
    println!("  -w, --warehouse <WAREHOUSE>    Warehouse within catalog (e.g., for Polaris)");
    println!("  -q, --quiet                    Suppress non-error output");
    println!(
        "      --log-level <LEVEL>        Log level: off, error, warn, info, debug, trace [default: off]"
    );
    println!("      --log-file <PATH>          Log to file");
    println!();

    println!("{}", "Catalog Override (ad-hoc):".bold());
    println!("      --catalog-uri <URI>        REST Catalog URI [env: ICETABLE_CATALOG_URI]");
    println!(
        "      --catalog-warehouse <WH>   Catalog warehouse [env: ICETABLE_CATALOG_WAREHOUSE]"
    );
    println!(
        "      --catalog-credential <C>   Credential as client_id:secret [env: ICETABLE_CATALOG_CREDENTIAL]"
    );
    println!(
        "      --catalog-credential-env   Credential from env var [env: ICETABLE_CATALOG_CREDENTIAL_ENV]"
    );
    println!(
        "      --catalog-credential-file  Credential from file [env: ICETABLE_CATALOG_CREDENTIAL_FILE]"
    );
    println!("      --catalog-use-iam-role     Use IAM role for auth (AWS, GCP, Azure)");
    println!("      --catalog-use-oauth2       Use OAuth2 client credentials flow");
    println!();

    println!("{}", "Resource Limits:".bold());
    println!(
        "      --max-memory <SIZE>        Max memory (e.g., 2GB). 0 = unlimited [env: ICETABLE_MAX_MEMORY]"
    );
    println!(
        "      --timeout <SECS>           Operation timeout. 0 = none [env: ICETABLE_TIMEOUT]"
    );
    println!("      --max-concurrency <N>      Max concurrent ops [env: ICETABLE_MAX_CONCURRENCY]");
    println!(
        "      --max-threads <N>          Max worker threads. 0 = auto [env: ICETABLE_MAX_THREADS]"
    );
}

fn main() {
    // Parse command-line arguments first (before runtime setup)
    let cli = Cli::parse();

    // Build tokio runtime with thread limits
    let mut runtime_builder = tokio::runtime::Builder::new_multi_thread();
    runtime_builder.enable_all();

    if cli.max_threads > 0 {
        runtime_builder.worker_threads(cli.max_threads);
    }

    let runtime = runtime_builder.build().unwrap_or_else(|e| {
        eprintln!("{} Failed to create runtime: {}", "Error:".red().bold(), e);
        process::exit(1);
    });

    // Run async main on the runtime with signal handling
    let exit_code = runtime.block_on(async {
        // Setup signal handlers for graceful shutdown
        let _cts = icetable::utils::setup_signal_handlers().await;

        // Run the main application
        let result = async_main(cli).await;

        // Check if cancelled
        if is_cancelled() {
            eprintln!("\n{}", "Operation cancelled by user".yellow());
            130 // Standard SIGINT exit code
        } else {
            result
        }
    });

    process::exit(exit_code);
}

async fn async_main(cli: Cli) -> i32 {
    // Initialize logger with settings from CLI
    icetable::utils::init_logger(cli.log_level);

    // Initialize resource limits from CLI options
    match ResourceLimits::from_cli(&cli.max_memory, cli.timeout, cli.max_concurrency) {
        Ok(limits) => init_resource_limits(limits),
        Err(e) => {
            eprintln!("{} {}", "Error:".red().bold(), e.user_message());
            return 1;
        }
    }

    // Build table context from CLI global options
    let ctx = cli.table_context();

    // Register cloud storage handlers (required for Delta Lake S3/GCS/Azure support)
    deltalake::aws::register_handlers(None);
    deltalake::gcp::register_handlers(None);
    deltalake::azure::register_handlers(None);

    // Execute command and handle errors
    let result = match cli.command {
        Commands::Ls(args) => LsCommand::execute(args, &ctx).await,
        Commands::Create(args) => CreateCommand::execute(args, &ctx).await,
        Commands::Delete(args) => DeleteCommand::execute(args, &ctx).await,
        Commands::Analyze(args) => AnalyzeCommand::execute(args, &ctx).await,
        Commands::Init(args) => InitCommand::execute(args).await,
        Commands::Inspect(args) => InspectCommand::execute(args, &ctx).await,
        Commands::Validate(args) => ValidateCommand::execute(args, &ctx).await,
        Commands::Diff(args) => DiffCommand::execute(args, &ctx).await,
        Commands::Stats(args) => StatsCommand::execute(args, &ctx).await,
        Commands::History(args) => HistoryCommand::execute(args, &ctx).await,
        Commands::Vacuum(args) => VacuumCommand::execute(args, &ctx).await,
        Commands::Optimize(args) => OptimizeCommand::execute(args, &ctx).await,
        Commands::Snapshot(args) => SnapshotCommand::execute(args, &ctx).await,
        Commands::Repair(args) => RepairCommand::execute(args, &ctx).await,
        Commands::Import(cmd) => match cmd {
            ImportCommands::Delta(args) => ImportCommand::delta(args, &ctx).await,
            ImportCommands::Parquet(args) => ImportCommand::parquet(args, &ctx).await,
        },
        Commands::Branch(args) => BranchCommand::execute(args, &ctx).await,
        Commands::Tag(args) => TagCommand::execute(args, &ctx).await,
        Commands::Config(args) => ConfigCommand::execute(args).await,
        Commands::Generate(args) => GenerateCommand::execute(args, &ctx).await,
        Commands::Completions(args) => {
            args.generate();
            Ok(())
        }
        Commands::Doctor(args) => DoctorCommand::execute(args).await,
        Commands::Admin(args) => AdminCommand::execute(args, &ctx).await,
        Commands::Options => {
            print_global_options();
            Ok(())
        }
        #[cfg(feature = "tui")]
        Commands::Tui(args) => TuiCommand::execute(args).await,
    };

    // Handle errors with user-friendly messages
    match result {
        Ok(()) => 0,
        Err(icetable::error::Error::Cancelled) => {
            // Cancelled errors are handled by signal handler output
            130
        }
        Err(e) => {
            eprintln!("{} {}", "Error:".red().bold(), e.user_message());

            if std::env::var("RUST_BACKTRACE").is_ok() {
                eprintln!("\n{}", "Debug:".yellow());
                eprintln!("{:?}", e);
            }

            1
        }
    }
}
