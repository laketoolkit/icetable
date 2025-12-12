//! Generate command arguments

use clap::Parser;

/// Arguments for generate command
#[derive(Parser, Debug)]
pub struct GenerateArgs {
    /// Table name (uses current from context if not specified)
    #[arg(short = 't', long = "table")]
    pub table: Option<String>,

    /// Catalog name (uses current if not specified)
    #[arg(short = 'c', long)]
    pub catalog: Option<String>,

    /// Namespace (uses current if not specified)
    #[arg(short = 'n', long)]
    pub namespace: Option<String>,

    /// Schema definition as "col:type,col:type" (e.g., "id:int,name:string,ts:timestamp")
    #[arg(long)]
    pub schema: Option<String>,

    /// Use a predefined schema template
    #[arg(long, value_enum)]
    pub template: Option<SchemaTemplate>,

    /// Number of rows to generate
    #[arg(long, default_value = "10000")]
    pub rows: u64,

    /// Number of data files to create
    #[arg(long, default_value = "4")]
    pub files: u32,

    /// Partition columns (comma-separated, e.g., "year,month")
    #[arg(long, value_delimiter = ',')]
    pub partition_by: Option<Vec<String>>,

    /// Random seed for reproducible data generation
    #[arg(long)]
    pub seed: Option<u64>,

    /// Target file size in bytes (default: 64MB)
    #[arg(long, default_value = "67108864")]
    pub target_file_size: u64,

    /// Dry run - show what would be generated without creating data
    #[arg(long)]
    pub dry_run: bool,

    /// Skip confirmation when appending to existing table
    #[arg(short = 'f', long)]
    pub force: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Predefined schema templates for common use cases
#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum SchemaTemplate {
    /// Simple events: id, timestamp, event_type, user_id, value
    Events,
    /// Financial transactions: id, timestamp, amount, currency, account_from, account_to, status
    Transactions,
    /// IoT sensor data: sensor_id, timestamp, temperature, humidity, pressure, location
    Sensors,
    /// User profiles: user_id, created_at, name, email, country, age, active
    Users,
    /// Web logs: request_id, timestamp, method, path, status_code, response_time_ms, user_agent
    WebLogs,
}

/// Arguments for completions command
#[derive(Parser, Debug)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
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
    /// Path to table file or directory
    pub path: String,

    /// Read-only mode
    #[arg(long)]
    pub readonly: bool,

    /// Refresh interval in seconds
    #[arg(short, long, default_value = "5")]
    pub refresh: u64,
}
