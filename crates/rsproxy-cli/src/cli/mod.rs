mod api_auth;
pub(crate) mod ca;
pub(crate) mod command;
pub(crate) mod config;
mod daemon;
mod output;
mod rules;
mod startup;
mod system_proxy;
mod trace;
mod util;

use crate::tui;
use crate::{CliError, CliResult, DaemonConflict};
use clap::{CommandFactory, Parser};
use command::{
    Cli, CompletionShell, ConfigCommand, RulesCommand, RuntimeArgs, StartupCommand, TopLevelCommand,
};
use rsproxy_control::api_request;
use std::io;

pub use command::Cli as ParsedCli;

/// Parses process arguments without initializing logging or runtime services.
pub fn parse_cli() -> Result<ParsedCli, clap::Error> {
    Cli::try_parse()
}

/// Parses process arguments and executes the selected composition-root action.
pub fn run_cli() -> CliResult<()> {
    let cli = parse_cli()?;
    run_parsed(cli)
}

/// Executes an already parsed command, initializing only the selected services.
///
/// The caller retains responsibility for process-level exit-code and JSON error
/// rendering; use [`crate::CliError::exit_code`] and [`crate::CliError::code`].
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
        TopLevelCommand::Rules(args) if matches!(&args.command, Some(RulesCommand::Help(_))) => {
            return rules::rules_cmd(args, cli.json);
        }
        command => command,
    };

    crate::logging::init()?;
    match command {
        TopLevelCommand::Run(args) => daemon::run_server(&args),
        TopLevelCommand::Start(args) => daemon::start_server(&args, true),
        TopLevelCommand::Stop(args) => daemon::stop_server(&args.into_runtime(), true),
        TopLevelCommand::Restart(args) => match daemon::stop_server(&args, true) {
            Ok(()) | Err(CliError::DaemonConflict(DaemonConflict::NotRunning { .. })) => {
                daemon::start_server(&args, true)
            }
            Err(error) => Err(error),
        },
        TopLevelCommand::Status(args) => {
            api_auth::configure_client_api_auth(&args)?;
            let config = config::runtime_config(&RuntimeArgs::from_client(args))?;
            let body = api_request("GET", &config.api, "/api/status", "")?;
            println!("{}", output::status(&body, cli.json)?);
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
            trace::replay_cmd(args, cli.json)
        }
        TopLevelCommand::Config(args) => {
            let runtime = RuntimeArgs::from_client(args.client);
            // `rsproxy config` with no subcommand defaults to `show`.
            match args.command {
                None | Some(ConfigCommand::Show(_)) => {
                    let config = config::runtime_config(&runtime)?;
                    println!("{}", output::config(&config, cli.json)?);
                }
                Some(ConfigCommand::Path(_)) => match config::effective_config_path(&runtime) {
                    Some(path) => println!("{}", path.display()),
                    None => println!("built-in defaults (no config file loaded)"),
                },
            }
            Ok(())
        }
        TopLevelCommand::Ca(args) => ca::ca_cmd(args, cli.json),
        TopLevelCommand::Proxy(args) => system_proxy::system_proxy_cmd(args, cli.json),
        TopLevelCommand::Startup(args) => match args.command {
            StartupCommand::Install(args) => startup::install(args, cli.json),
            StartupCommand::Status => startup::status(cli.json),
            StartupCommand::Uninstall(args) => startup::uninstall(args, cli.json),
            StartupCommand::Launch => startup::launch(cli.json),
        },
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
