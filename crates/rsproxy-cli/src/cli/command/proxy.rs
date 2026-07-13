use clap::{Args, Subcommand, ValueEnum};

use super::ClientArgs;

#[derive(Args)]
pub struct ProxyArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    #[arg(long, global = true, value_enum)]
    pub platform: Option<ProxyPlatformArg>,
    #[arg(long, global = true)]
    pub service: Option<String>,
    #[arg(long, global = true)]
    pub dry_run: bool,
    #[command(subcommand)]
    pub command: Option<ProxyCommand>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum ProxyPlatformArg {
    #[value(alias = "darwin")]
    Macos,
    #[value(alias = "win")]
    Windows,
    Linux,
}

#[derive(Subcommand)]
pub enum ProxyCommand {
    Status(ProxyStatusArgs),
    On(ProxyMutationArgs),
    Off(ProxyMutationArgs),
}

#[derive(Args)]
pub struct ProxyStatusArgs {}

#[derive(Args)]
pub struct ProxyMutationArgs {
    #[arg(long)]
    pub host: Option<String>,
    #[arg(long)]
    pub port: Option<u16>,
    #[arg(long)]
    pub bypass: Option<String>,
    #[arg(long)]
    pub all: bool,
}
