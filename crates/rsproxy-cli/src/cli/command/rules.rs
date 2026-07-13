use clap::{ArgAction, Args, Subcommand};
use std::path::PathBuf;

use super::ClientArgs;

#[derive(Args)]
pub(crate) struct RulesArgs {
    #[command(flatten)]
    pub(crate) client: ClientArgs,
    #[command(subcommand)]
    pub(crate) command: RulesCommand,
}

#[derive(Subcommand)]
pub(crate) enum RulesCommand {
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
pub(crate) struct RulesListArgs {}

#[derive(Args)]
pub(crate) struct RulesCheckArgs {
    pub(crate) file: Option<PathBuf>,
}

#[derive(Args)]
pub(crate) struct OptionalGroupArgs {
    #[arg(default_value = "default")]
    pub(crate) group: String,
}

#[derive(Args)]
pub(crate) struct RequiredGroupArgs {
    pub(crate) group: String,
}

#[derive(Args)]
pub(crate) struct RulesSetArgs {
    #[arg(default_value = "default")]
    pub(crate) group: String,
    #[arg(long)]
    pub(crate) file: Option<PathBuf>,
}

#[derive(Args)]
pub(crate) struct RulesSourceArgs {
    #[arg(long)]
    pub(crate) file: Option<PathBuf>,
}

#[derive(Clone, Default, Args)]
pub(crate) struct RequestArgs {
    #[arg(short = 'X', long, default_value = "GET")]
    pub(crate) method: String,
    #[arg(short = 'H', long, action = ArgAction::Append)]
    pub(crate) header: Vec<String>,
    #[arg(short = 'd', long)]
    pub(crate) body: Option<String>,
    #[arg(long)]
    pub(crate) client_ip: Option<String>,
    #[arg(long)]
    pub(crate) server_ip: Option<String>,
}

#[derive(Args)]
pub(crate) struct RulesTestArgs {
    pub(crate) url: String,
    #[command(flatten)]
    pub(crate) request: RequestArgs,
    #[arg(long)]
    pub(crate) response_status: Option<String>,
    #[arg(long, action = ArgAction::Append)]
    pub(crate) response_header: Vec<String>,
}

#[derive(Args)]
pub(crate) struct RulesBenchArgs {
    /// URL to benchmark (may also be supplied with --url).
    pub(crate) positional_url: Option<String>,
    #[arg(long)]
    pub(crate) url: Option<String>,
    #[command(flatten)]
    pub(crate) request: RequestArgs,
    #[command(flatten)]
    pub(crate) source: RulesSourceArgs,
    #[arg(short = 'n', long)]
    pub(crate) iterations: Option<usize>,
    #[arg(long)]
    pub(crate) warmup: Option<usize>,
}
