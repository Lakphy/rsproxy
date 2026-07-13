use clap::{Args, Subcommand};
use std::path::PathBuf;

use super::ClientArgs;

#[derive(Args)]
pub(crate) struct CaArgs {
    #[command(flatten)]
    pub(crate) client: ClientArgs,
    #[command(subcommand)]
    pub(crate) command: Option<CaCommand>,
}

#[derive(Subcommand)]
pub(crate) enum CaCommand {
    Init(CaInitArgs),
    Status(CaStatusArgs),
    Export(CaExportArgs),
    Issue(CaIssueArgs),
    Install(CaTrustArgs),
    Uninstall(CaTrustArgs),
}

#[derive(Args)]
pub(crate) struct CaInitArgs {
    #[arg(long)]
    pub(crate) force: bool,
    #[arg(long)]
    pub(crate) name: Option<String>,
}

#[derive(Args)]
pub(crate) struct CaStatusArgs {
    #[arg(long)]
    pub(crate) keychain: Option<PathBuf>,
}

#[derive(Args)]
pub(crate) struct CaExportArgs {
    #[arg(short = 'o', long = "out")]
    pub(crate) output: Option<PathBuf>,
}

#[derive(Args)]
pub(crate) struct CaIssueArgs {
    pub(crate) host: String,
    #[arg(long)]
    pub(crate) force: bool,
}

#[derive(Args)]
pub(crate) struct CaTrustArgs {
    #[arg(long)]
    pub(crate) keychain: Option<PathBuf>,
    #[arg(long)]
    pub(crate) dry_run: bool,
}
