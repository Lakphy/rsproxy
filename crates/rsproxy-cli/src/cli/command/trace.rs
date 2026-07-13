use clap::{Args, Subcommand};
use std::path::PathBuf;

use super::ClientArgs;

#[derive(Args)]
pub(crate) struct ValuesArgs {
    #[command(flatten)]
    pub(crate) client: ClientArgs,
    #[command(subcommand)]
    pub(crate) command: ValuesCommand,
}

#[derive(Subcommand)]
pub(crate) enum ValuesCommand {
    #[command(name = "ls")]
    List(ValuesListArgs),
    Cat(ValueKeyArgs),
    Set(ValueSetArgs),
    #[command(name = "rm")]
    Remove(ValueKeyArgs),
}

#[derive(Args)]
pub(crate) struct ValuesListArgs {}

#[derive(Args)]
pub(crate) struct ValueKeyArgs {
    pub(crate) key: String,
}

#[derive(Args)]
pub(crate) struct ValueSetArgs {
    pub(crate) key: String,
    #[arg(long)]
    pub(crate) file: Option<PathBuf>,
}

#[derive(Args)]
pub(crate) struct TraceArgs {
    #[command(flatten)]
    pub(crate) client: ClientArgs,
    #[command(subcommand)]
    pub(crate) command: TraceCommand,
}

#[derive(Subcommand)]
pub(crate) enum TraceCommand {
    #[command(name = "ls")]
    List(TraceListArgs),
    Get(TraceGetArgs),
    Follow(TraceFollowArgs),
    Stats(TraceStatsArgs),
    Clear(TraceClearArgs),
    Export(TraceExportArgs),
}

#[derive(Args)]
pub(crate) struct TraceStatsArgs {}

#[derive(Args)]
pub(crate) struct TraceClearArgs {}

#[derive(Args)]
pub(crate) struct TraceListArgs {
    #[arg(short = 'n', long, default_value_t = 20)]
    pub(crate) limit: usize,
}

#[derive(Args)]
pub(crate) struct TraceGetArgs {
    pub(crate) id: String,
}

#[derive(Args)]
pub(crate) struct TraceFollowArgs {
    #[arg(long)]
    pub(crate) count: Option<usize>,
    #[arg(long)]
    pub(crate) poll_ms: Option<u64>,
}

#[derive(Args)]
pub(crate) struct TraceExportArgs {
    #[arg(long)]
    pub(crate) har: bool,
    #[arg(short = 'o', long)]
    pub(crate) output: Option<PathBuf>,
}

#[derive(Args)]
pub(crate) struct TuiArgs {
    #[command(flatten)]
    pub(crate) client: ClientArgs,
    #[arg(short = 'n', long)]
    pub(crate) limit: Option<usize>,
    #[arg(long)]
    pub(crate) filter: Option<String>,
    #[arg(long, value_parser = ["overview", "headers", "body", "rules"])]
    pub(crate) tab: Option<String>,
    #[arg(long)]
    pub(crate) interval_ms: Option<u64>,
    #[arg(long)]
    pub(crate) once: bool,
}

#[derive(Args)]
pub(crate) struct ReplayArgs {
    pub(crate) id: String,
    #[command(flatten)]
    pub(crate) client: ClientArgs,
}
