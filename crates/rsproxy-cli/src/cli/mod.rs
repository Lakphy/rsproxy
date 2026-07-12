pub(crate) mod api;
mod api_auth;
pub(crate) mod args;
pub(crate) mod ca;
mod completions;
pub(crate) mod config;
mod daemon;
mod help;
mod rules;
mod system_proxy;
mod trace;

use api::*;
use api_auth::*;
use args::*;
use ca::*;
use completions::*;
use config::*;
use daemon::*;
use help::*;
use rules::*;
use system_proxy::*;
use trace::*;

use crate::app::{
    AppConfig, MitmCertCache, MitmFailureCache, SharedState, api_display, default_storage,
    unix_api_path,
};
use crate::{control, dns, proxy, tui};
use rsproxy_rules::{RequestMeta, ResponseMeta, RuleSet, UrlParts};
use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub fn run_cli() -> Result<(), String> {
    crate::logging::init()?;
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        print_help();
        return Ok(());
    }
    let command = args.remove(0);
    if matches!(command.as_str(), "--version" | "-V") {
        println!("rsproxy {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if matches!(command.as_str(), "help" | "--help" | "-h") {
        return print_help_command(&args);
    }
    if help_requested(&args) {
        return print_command_help(&command, &args);
    }
    if matches!(
        command.as_str(),
        "status" | "rules" | "values" | "trace" | "tui" | "replay"
    ) {
        configure_client_api_auth(&args)?;
    }

    match command.as_str() {
        "run" => run_server(args),
        "start" => start_server(args),
        "stop" => stop_server(args),
        "restart" => {
            let stop_args = args.clone();
            let _ = stop_server(stop_args);
            start_server(args)
        }
        "status" => {
            let config = runtime_config(&args)?;
            println!("{}", api_request("GET", &config.api, "/api/status", "")?);
            Ok(())
        }
        "rules" => rules_cmd(args),
        "values" => values_cmd(args),
        "trace" => trace_cmd(args),
        "tui" => tui::tui_cmd(args),
        "replay" => replay_cmd(args),
        "ca" => ca_cmd(args),
        "proxy" => system_proxy_cmd(args),
        "completions" => completions_cmd(args),
        other => Err(format!("unknown command `{other}`")),
    }
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
