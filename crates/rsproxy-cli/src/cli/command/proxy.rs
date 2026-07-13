use clap::{Args, Subcommand, ValueEnum};

use super::ClientArgs;

#[derive(Args)]
pub(crate) struct ProxyArgs {
    #[command(flatten)]
    pub(crate) client: ClientArgs,
    #[arg(long, global = true, value_enum)]
    pub(crate) platform: Option<ProxyPlatformArg>,
    #[arg(long, global = true)]
    pub(crate) service: Option<String>,
    #[arg(long, global = true)]
    pub(crate) dry_run: bool,
    #[command(subcommand)]
    pub(crate) command: Option<ProxyCommand>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ProxyPlatformArg {
    #[value(alias = "darwin")]
    Macos,
    #[value(alias = "win")]
    Windows,
    Linux,
}

#[derive(Subcommand)]
pub(crate) enum ProxyCommand {
    Status(ProxyStatusArgs),
    On(ProxyMutationArgs),
    Off(ProxyMutationArgs),
}

#[derive(Args)]
pub(crate) struct ProxyStatusArgs {}

#[derive(Args)]
pub(crate) struct ProxyMutationArgs {
    #[arg(long)]
    pub(crate) host: Option<String>,
    #[arg(long)]
    pub(crate) port: Option<u16>,
    #[arg(long)]
    pub(crate) bypass: Option<String>,
    #[arg(long)]
    pub(crate) all: bool,
}
