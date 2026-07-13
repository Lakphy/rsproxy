use clap::{Args, Subcommand};
use std::path::PathBuf;

use super::ClientArgs;

#[derive(Args)]
pub struct ValuesArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    #[command(subcommand)]
    pub command: ValuesCommand,
}

#[derive(Subcommand)]
pub enum ValuesCommand {
    #[command(name = "ls")]
    List(ValuesListArgs),
    Cat(ValueKeyArgs),
    Set(ValueSetArgs),
    #[command(name = "rm")]
    Remove(ValueKeyArgs),
}

#[derive(Args)]
pub struct ValuesListArgs {}

#[derive(Args)]
pub struct ValueKeyArgs {
    pub key: String,
}

#[derive(Args)]
pub struct ValueSetArgs {
    pub key: String,
    #[arg(long)]
    pub file: Option<PathBuf>,
}

#[derive(Args)]
pub struct TraceArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    #[command(subcommand)]
    pub command: TraceCommand,
}

#[derive(Subcommand)]
pub enum TraceCommand {
    #[command(name = "ls")]
    List(TraceListArgs),
    Get(TraceGetArgs),
    Follow(TraceFollowArgs),
    Stats(TraceStatsArgs),
    Clear(TraceClearArgs),
    Export(TraceExportArgs),
}

#[derive(Args)]
pub struct TraceStatsArgs {}

#[derive(Args)]
pub struct TraceClearArgs {}

#[derive(Args)]
pub struct TraceListArgs {
    #[arg(short = 'n', long, default_value_t = 20)]
    pub limit: usize,
}

#[derive(Args)]
pub struct TraceGetArgs {
    pub id: String,
}

#[derive(Args)]
pub struct TraceFollowArgs {
    #[arg(long)]
    pub count: Option<usize>,
    #[arg(long)]
    pub poll_ms: Option<u64>,
}

#[derive(Args)]
pub struct TraceExportArgs {
    #[arg(long)]
    pub har: bool,
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,
}

#[derive(Args)]
pub struct TuiArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    #[arg(short = 'n', long)]
    pub limit: Option<usize>,
    #[arg(long)]
    pub filter: Option<String>,
    #[arg(long, value_parser = ["overview", "headers", "body", "rules"])]
    pub tab: Option<String>,
    #[arg(long)]
    pub interval_ms: Option<u64>,
    #[arg(long)]
    pub once: bool,
}

#[derive(Args)]
pub struct ReplayArgs {
    pub id: String,
    #[command(flatten)]
    pub client: ClientArgs,
}
