//! icetable CLI entry point

use clap::Parser;
use colored::Colorize;
use std::process;

use icetable::cli::commands::*;
use icetable::cli::parser::{Cli, Commands, ImportCommands};
use icetable::utils::{ResourceLimits, init_resource_limits, is_cancelled};

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

    // Build catalog config from CLI options (if any)
    let catalog_config = cli.catalog_config();

    // Register cloud storage handlers (required for Delta Lake S3/GCS/Azure support)
    #[cfg(feature = "delta")]
    {
        deltalake::aws::register_handlers(None);
        deltalake::gcp::register_handlers(None);
        deltalake::azure::register_handlers(None);
    }

    // Execute command and handle errors
    let result = match cli.command {
        Commands::Analyze(args) => AnalyzeCommand::execute(args, catalog_config.clone()).await,
        Commands::Init(args) => InitCommand::execute(args).await,
        Commands::Inspect(args) => InspectCommand::execute(args, catalog_config.clone()).await,
        Commands::Validate(args) => ValidateCommand::execute(args, catalog_config.clone()).await,
        Commands::Diff(args) => DiffCommand::execute(args).await,
        Commands::Stats(args) => StatsCommand::execute(args, catalog_config.clone()).await,
        Commands::History(args) => HistoryCommand::execute(args).await,
        Commands::Vacuum(args) => VacuumCommand::execute(args, catalog_config.clone()).await,
        Commands::Optimize(args) => OptimizeCommand::execute(args, catalog_config.clone()).await,
        Commands::Snapshot(args) => SnapshotCommand::execute(args, catalog_config.clone()).await,
        Commands::Repair(args) => RepairCommand::execute(args, catalog_config.clone()).await,
        Commands::Import(cmd) => match cmd {
            #[cfg(feature = "delta")]
            ImportCommands::Delta(args) => ImportCommand::delta(args).await,
            ImportCommands::Parquet(args) => ImportCommand::parquet(args).await,
        },
        Commands::Branch(args) => BranchCommand::execute(args, catalog_config.clone()).await,
        Commands::Tag(args) => TagCommand::execute(args, catalog_config.clone()).await,
        Commands::Config(args) => ConfigCommand::execute(args).await,
        Commands::Catalog(args) => CatalogCommand::execute(args).await,
        Commands::Completions(args) => {
            args.generate();
            Ok(())
        }
        Commands::Doctor(args) => DoctorCommand::execute(args).await,
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
            eprintln!("{}", "Error:".red().bold());
            eprintln!("{}", e.user_message());

            if std::env::var("RUST_BACKTRACE").is_ok() {
                eprintln!("\n{}", "Backtrace:".yellow());
                eprintln!("{:?}", e);
            }

            1
        }
    }
}
