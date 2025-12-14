//! Generate command arguments

use clap::Parser;

/// Arguments for generate command
///
/// Note: --catalog and --warehouse are global options (use -c and -w)
#[derive(Parser, Debug)]
pub struct GenerateArgs {
    /// Schema definition (col:type,col:type)
    #[arg(long)]
    pub schema: Option<String>,

    /// Predefined template (events, transactions, sensors, users, web-logs)
    #[arg(long, value_enum, hide_possible_values = true)]
    pub template: Option<SchemaTemplate>,

    /// Rows per file [default: 10000]
    #[arg(long, default_value = "10000", hide_default_value = true)]
    pub rows: u64,

    /// Files to create [default: 1]
    #[arg(long, default_value = "1", hide_default_value = true)]
    pub files: u32,

    /// Partition columns (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub partition_by: Option<Vec<String>>,

    /// Random seed for reproducibility
    #[arg(long)]
    pub seed: Option<u64>,

    /// Target file size [default: 64MB]
    #[arg(long, default_value = "67108864", hide_default_value = true)]
    pub target_file_size: u64,

    /// Preview without creating data
    #[arg(long)]
    pub dry_run: bool,

    /// Skip confirmation for existing table
    #[arg(short = 'f', long)]
    pub force: bool,

    /// Output format [default: text]
    #[arg(short, long, default_value = "text", hide_default_value = true)]
    pub output: String,
}

/// Predefined schema templates for common use cases
#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum SchemaTemplate {
    /// Event data
    Events,
    /// Financial transactions
    Transactions,
    /// IoT sensor data
    Sensors,
    /// User profiles
    Users,
    /// Web access logs
    WebLogs,
}

/// Arguments for completions command
#[derive(Parser, Debug)]
pub struct CompletionsArgs {
    /// Target shell
    #[arg(value_enum)]
    pub shell: clap_complete::Shell,
}

impl CompletionsArgs {
    /// Generate completions and print to stdout
    pub fn generate(&self) {
        use clap::CommandFactory;
        let mut cmd = super::Cli::command();
        clap_complete::generate(self.shell, &mut cmd, "icetable", &mut std::io::stdout());
    }
}

/// Arguments for tui command
#[cfg(feature = "tui")]
#[derive(Parser, Debug)]
pub struct TuiArgs {
    /// Table path
    pub path: String,

    /// Read-only mode
    #[arg(long)]
    pub readonly: bool,

    /// Refresh interval (seconds)
    #[arg(short, long, default_value = "5")]
    pub refresh: u64,
}
