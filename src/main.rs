//! TableTools CLI entry point

use clap::Parser;
use colored::Colorize;
use std::process;

use tabletools::cli::commands::*;
use tabletools::cli::parser::{Cli, Commands};

#[tokio::main]
async fn main() {
    // Parse command-line arguments
    let cli = Cli::parse();

    // Initialize logger with settings from CLI
    tabletools::utils::init_logger(cli.log_level);

    // Execute command and handle errors
    let result = match cli.command {
        Commands::Inspect(args) => InspectCommand::execute(args).await,
        Commands::Validate(args) => ValidateCommand::execute(args).await,
        Commands::Diff(args) => DiffCommand::execute(args).await,
        Commands::Convert(args) => ConvertCommand::execute(args).await,
        Commands::Stats(args) => StatsCommand::execute(args).await,
        Commands::Query(args) => QueryCommand::execute(args).await,
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
