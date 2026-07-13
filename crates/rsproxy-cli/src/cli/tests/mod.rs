mod api;
mod ca;
mod config;
mod rules;
mod runtime;
mod system_proxy;

use crate::CliResult;
use crate::app::AppConfig;
use crate::cli::command::{Cli, RuntimeArgs, TopLevelCommand, TraceCommand};
use crate::cli::config as config_impl;
use crate::cli::util::parse_size;
use clap::Parser;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

fn parse_runtime_args(args: &[String]) -> CliResult<RuntimeArgs> {
    let argv = ["rsproxy", "run"]
        .into_iter()
        .map(str::to_string)
        .chain(args.iter().cloned());
    let cli = Cli::try_parse_from(argv)?;
    match cli.command {
        Some(TopLevelCommand::Run(args)) => Ok(args),
        _ => unreachable!("test runtime parser selected the wrong command"),
    }
}

fn runtime_config(args: &[String]) -> CliResult<AppConfig> {
    config_impl::runtime_config(&parse_runtime_args(args)?)
}

fn runtime_config_without_default(args: &[String]) -> CliResult<AppConfig> {
    config_impl::runtime_config_without_default(&parse_runtime_args(args)?)
}

fn runtime_config_with_default_path(
    args: &[String],
    default_path: Option<PathBuf>,
) -> CliResult<AppConfig> {
    config_impl::runtime_config_with_default_path(&parse_runtime_args(args)?, default_path)
}

fn parse_trace_list_limit(args: &[String]) -> CliResult<usize> {
    let argv = ["rsproxy", "trace", "ls"]
        .into_iter()
        .map(str::to_string)
        .chain(args.iter().cloned());
    let cli = Cli::try_parse_from(argv)?;
    match cli.command {
        Some(TopLevelCommand::Trace(args)) => match args.command {
            TraceCommand::List(args) => Ok(args.limit),
            _ => unreachable!("test trace parser selected the wrong subcommand"),
        },
        _ => unreachable!("test trace parser selected the wrong command"),
    }
}
