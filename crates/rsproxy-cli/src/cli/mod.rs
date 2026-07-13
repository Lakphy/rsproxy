mod api_auth;
pub(crate) mod ca;
pub(crate) mod command;
pub(crate) mod config;
mod daemon;
mod rules;
mod system_proxy;
mod trace;
mod util;

use crate::tui;
use crate::{CliError, CliResult, DaemonConflict};
use clap::{CommandFactory, Parser};
use command::{Cli, CompletionShell, TopLevelCommand};
use rsproxy_control::api_request;
use std::io;

pub use command::Cli as ParsedCli;

pub fn parse_cli() -> Result<ParsedCli, clap::Error> {
    Cli::try_parse()
}

pub fn run_cli() -> CliResult<()> {
    let cli = parse_cli()?;
    run_parsed(cli)
}

pub fn run_parsed(cli: ParsedCli) -> CliResult<()> {
    let Some(command) = cli.command else {
        let mut command = Cli::command();
        command
            .print_help()
            .map_err(|source| CliError::io("print root help", source))?;
        println!();
        return Ok(());
    };

    let command = match command {
        TopLevelCommand::Completions(args) => return generate_completions(args.shell),
        command => command,
    };

    crate::logging::init()?;
    match command {
        TopLevelCommand::Run(args) => daemon::run_server(&args),
        TopLevelCommand::Start(args) => daemon::start_server(&args),
        TopLevelCommand::Stop(args) => daemon::stop_server(&args),
        TopLevelCommand::Restart(args) => match daemon::stop_server(&args) {
            Ok(()) | Err(CliError::DaemonConflict(DaemonConflict::NotRunning { .. })) => {
                daemon::start_server(&args)
            }
            Err(error) => Err(error),
        },
        TopLevelCommand::Status(args) => {
            api_auth::configure_client_api_auth(&args.client)?;
            let config = config::runtime_config(&args)?;
            println!("{}", api_request("GET", &config.api, "/api/status", "")?);
            Ok(())
        }
        TopLevelCommand::Rules(args) => {
            api_auth::configure_client_api_auth(&args.client)?;
            rules::rules_cmd(args, cli.json)
        }
        TopLevelCommand::Values(args) => {
            api_auth::configure_client_api_auth(&args.client)?;
            trace::values_cmd(args, cli.json)
        }
        TopLevelCommand::Trace(args) => {
            api_auth::configure_client_api_auth(&args.client)?;
            trace::trace_cmd(args, cli.json)
        }
        TopLevelCommand::Tui(args) => {
            api_auth::configure_client_api_auth(&args.client)?;
            tui::tui_cmd(args, cli.json)
        }
        TopLevelCommand::Replay(args) => {
            api_auth::configure_client_api_auth(&args.client)?;
            trace::replay_cmd(args)
        }
        TopLevelCommand::Ca(args) => ca::ca_cmd(args, cli.json),
        TopLevelCommand::Proxy(args) => system_proxy::system_proxy_cmd(args, cli.json),
        TopLevelCommand::Completions(_) => unreachable!("completions returned before logging"),
    }
}

fn generate_completions(shell: CompletionShell) -> CliResult<()> {
    let shell = match shell {
        CompletionShell::Bash => clap_complete::Shell::Bash,
        CompletionShell::Zsh => clap_complete::Shell::Zsh,
        CompletionShell::Fish => clap_complete::Shell::Fish,
        CompletionShell::Powershell => clap_complete::Shell::PowerShell,
    };
    let mut command = Cli::command();
    clap_complete::generate(shell, &mut command, "rsproxy", &mut io::stdout());
    Ok(())
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
