//! TableTools CLI entry point

use clap::Parser;
use colored::Colorize;
use std::process;

use tabletools::cli::parser::{Cli, Commands};
use tabletools::cli::commands::*;

#[tokio::main]
async fn main() {
    // Initialize logger
    env_logger::init();

    // Parse command-line arguments
    let cli = Cli::parse();

    // Execute command and handle errors
    let result = match cli.command {
        Commands::Inspect(args) => InspectCommand::execute(args).await,
        Commands::Validate(args) => ValidateCommand::execute(args).await,
        Commands::Diff(args) => DiffCommand::execute(args).await,
        Commands::Convert(args) => ConvertCommand::execute(args).await,
        Commands::Stats(args) => StatsCommand::execute(args).await,
        Commands::Query(args) => QueryCommand::execute(args).await,
        #[cfg(feature = "serve")]
        Commands::Serve(args) => ServeCommand::execute(args).await,
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

