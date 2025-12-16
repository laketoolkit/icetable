//! Describe command implementation
//!
//! Unified command that consolidates inspect, stats, and analyze.
//! Shows table info with optional sections.

use crate::cli::parser::{AnalyzeArgs, CatalogContext, DescribeArgs, InspectArgs, StatsArgs};
use crate::error::Result;

use super::{AnalyzeCommand, InspectCommand, StatsCommand};

/// Handler for describe command
pub struct DescribeCommand;

impl DescribeCommand {
    /// Execute describe command
    pub async fn execute(args: DescribeArgs, ctx: &CatalogContext) -> Result<()> {
        // If specific flags are set, only show those sections
        let has_specific_flag = args.schema || args.stats || args.health || args.partitions;

        if has_specific_flag {
            // Show only requested sections
            if args.schema || args.partitions {
                let inspect_args = InspectArgs {
                    verbose: args.partitions,
                    output: args.output.clone(),
                    snapshot: args.snapshot,
                    as_of: None,
                };
                InspectCommand::execute(inspect_args, ctx).await?;
            }

            if args.stats {
                let stats_args = StatsArgs {
                    output: args.output.clone(),
                    partition: None,
                };
                StatsCommand::execute(stats_args, ctx).await?;
            }

            if args.health {
                let analyze_args = AnalyzeArgs {
                    min_file_size: 16777216,
                    skip_orphans: false,
                    all_snapshots: true,
                    verbose: false,
                    output: args.output.clone(),
                };
                AnalyzeCommand::execute(analyze_args, ctx).await?;
            }
        } else {
            // Default: show schema/metadata (inspect without verbose)
            let inspect_args = InspectArgs {
                verbose: false,
                output: args.output.clone(),
                snapshot: args.snapshot,
                as_of: None,
            };
            InspectCommand::execute(inspect_args, ctx).await?;
        }

        Ok(())
    }
}
