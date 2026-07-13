use clap::{Args, Subcommand};
use std::path::PathBuf;

use super::ClientArgs;

#[derive(Args)]
pub struct CaArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    #[command(subcommand)]
    pub command: Option<CaCommand>,
}

#[derive(Subcommand)]
pub enum CaCommand {
    Init(CaInitArgs),
    Status(CaStatusArgs),
    Export(CaExportArgs),
    Issue(CaIssueArgs),
    Install(CaTrustArgs),
    Uninstall(CaTrustArgs),
}

#[derive(Args)]
pub struct CaInitArgs {
    #[arg(long)]
    pub force: bool,
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Args)]
pub struct CaStatusArgs {
    #[arg(long)]
    pub keychain: Option<PathBuf>,
}

#[derive(Args)]
pub struct CaExportArgs {
    #[arg(short = 'o', long = "out")]
    pub output: Option<PathBuf>,
}

#[derive(Args)]
pub struct CaIssueArgs {
    pub host: String,
    #[arg(long)]
    pub force: bool,
}

#[derive(Args)]
pub struct CaTrustArgs {
    #[arg(long)]
    pub keychain: Option<PathBuf>,
    #[arg(long)]
    pub dry_run: bool,
}
