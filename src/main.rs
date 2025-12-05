//! icetable CLI entry point

use clap::Parser;
use colored::Colorize;
use std::process;

use icetable::cli::commands::*;
use icetable::cli::parser::{Cli, Commands, ImportCommands};

#[tokio::main]
async fn main() {
    // Parse command-line arguments
    let cli = Cli::parse();

    // Initialize logger with settings from CLI
    icetable::utils::init_logger(cli.log_level);

    // Register cloud storage handlers (required for Delta Lake S3/GCS/Azure support)
    #[cfg(feature = "delta")]
    {
        deltalake::aws::register_handlers(None);
        deltalake::gcp::register_handlers(None);
        deltalake::azure::register_handlers(None);
    }

    // Execute command and handle errors
    let result = match cli.command {
        Commands::Analyze(args) => AnalyzeCommand::execute(args).await,
        Commands::Init(args) => InitCommand::execute(args).await,
        Commands::Inspect(args) => InspectCommand::execute(args).await,
        Commands::Validate(args) => ValidateCommand::execute(args).await,
        Commands::Diff(args) => DiffCommand::execute(args).await,
        Commands::Stats(args) => StatsCommand::execute(args).await,
        Commands::History(args) => HistoryCommand::execute(args).await,
        Commands::Vacuum(args) => VacuumCommand::execute(args).await,
        Commands::Optimize(args) => OptimizeCommand::execute(args).await,
        Commands::Snapshot(args) => SnapshotCommand::execute(args).await,
        Commands::Repair(args) => RepairCommand::execute(args).await,
        Commands::Import(cmd) => match cmd {
            #[cfg(feature = "delta")]
            ImportCommands::Delta(args) => ImportCommand::delta(args).await,
            ImportCommands::Parquet(args) => ImportCommand::parquet(args).await,
        },
        Commands::Branch(args) => BranchCommand::execute(args).await,
        Commands::Tag(args) => TagCommand::execute(args).await,
        Commands::Config(args) => ConfigCommand::execute(args).await,
        Commands::Completions(args) => {
            args.generate();
            Ok(())
        }
        Commands::Doctor(args) => DoctorCommand::execute(args).await,
        #[cfg(feature = "tui")]
        Commands::Tui(args) => TuiCommand::execute(args).await,
    };

    // Handle errors with user-friendly messages
    if let Err(e) = result {
        eprintln!("{}", "Error:".red().bold());
        eprintln!("{}", e.user_message());

        if std::env::var("RUST_BACKTRACE").is_ok() {
            eprintln!("\n{}", "Backtrace:".yellow());
            eprintln!("{:?}", e);
        }

        process::exit(1);
    }
}
