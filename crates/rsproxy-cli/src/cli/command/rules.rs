use clap::{ArgAction, Args, Subcommand};
use std::path::PathBuf;

use super::ClientArgs;

#[derive(Args)]
pub struct RulesArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    #[command(subcommand)]
    pub command: RulesCommand,
}

#[derive(Subcommand)]
pub enum RulesCommand {
    /// Validate rules read from FILE or stdin.
    Check(RulesCheckArgs),
    /// List rule groups.
    #[command(name = "ls")]
    List(RulesListArgs),
    /// Print a rule group (defaults to `default`).
    Cat(OptionalGroupArgs),
    /// Edit a rule group (defaults to `default`).
    Edit(OptionalGroupArgs),
    /// Replace a rule group from FILE or stdin (defaults to `default`).
    Set(RulesSetArgs),
    /// Remove a rule group.
    #[command(name = "rm")]
    Remove(RequiredGroupArgs),
    /// Enable a rule group.
    Enable(RequiredGroupArgs),
    /// Disable a rule group.
    Disable(RequiredGroupArgs),
    /// Print rule index statistics.
    Stats(RulesSourceArgs),
    /// Benchmark rule resolution.
    Bench(RulesBenchArgs),
    /// Explain the rules applied to a request.
    Test(RulesTestArgs),
}

#[derive(Args)]
pub struct RulesListArgs {}

#[derive(Args)]
pub struct RulesCheckArgs {
    pub file: Option<PathBuf>,
}

#[derive(Args)]
pub struct OptionalGroupArgs {
    #[arg(default_value = "default")]
    pub group: String,
}

#[derive(Args)]
pub struct RequiredGroupArgs {
    pub group: String,
}

#[derive(Args)]
pub struct RulesSetArgs {
    #[arg(default_value = "default")]
    pub group: String,
    #[arg(long)]
    pub file: Option<PathBuf>,
}

#[derive(Args)]
pub struct RulesSourceArgs {
    #[arg(long)]
    pub file: Option<PathBuf>,
}

#[derive(Clone, Default, Args)]
pub struct RequestArgs {
    #[arg(short = 'X', long, default_value = "GET")]
    pub method: String,
    #[arg(short = 'H', long, action = ArgAction::Append)]
    pub header: Vec<String>,
    #[arg(short = 'd', long)]
    pub body: Option<String>,
    #[arg(long)]
    pub client_ip: Option<String>,
    #[arg(long)]
    pub server_ip: Option<String>,
}

#[derive(Args)]
pub struct RulesTestArgs {
    pub url: String,
    #[command(flatten)]
    pub request: RequestArgs,
    #[arg(long)]
    pub response_status: Option<String>,
    #[arg(long, action = ArgAction::Append)]
    pub response_header: Vec<String>,
}

#[derive(Args)]
pub struct RulesBenchArgs {
    /// URL to benchmark (may also be supplied with --url).
    pub positional_url: Option<String>,
    #[arg(long)]
    pub url: Option<String>,
    #[command(flatten)]
    pub request: RequestArgs,
    #[command(flatten)]
    pub source: RulesSourceArgs,
    #[arg(short = 'n', long)]
    pub iterations: Option<usize>,
    #[arg(long)]
    pub warmup: Option<usize>,
}
