//! Process entry point for the `rsproxy` executable.
//!
//! Argument parsing and execution live in `rsproxy_cli`; this boundary owns
//! stable exit codes plus human/JSON error rendering.

use clap::error::ErrorKind;
use rsproxy_cli::CliError;

fn main() {
    let cli = match rsproxy_cli::parse_cli() {
        Ok(cli) => cli,
        Err(error) => exit_parse_error(error),
    };
    let json = cli.json;
    if let Err(error) = rsproxy_cli::run_parsed(cli) {
        render_runtime_error(&error, json);
        std::process::exit(error.exit_code());
    }
}

fn exit_parse_error(error: clap::Error) -> ! {
    let exit_code = error.exit_code();
    if matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) {
        let _ = error.print();
    } else if std::env::args().any(|argument| argument == "--json") {
        render_json_error("usage_error", &error.to_string());
    } else {
        let _ = error.print();
    }
    std::process::exit(exit_code)
}

fn render_runtime_error(error: &CliError, json: bool) {
    if json {
        render_json_error(error.code(), &error.to_string());
    } else {
        tracing::error!(event = "cli_failed", code = error.code(), error = %error, "command failed");
        eprintln!("error: {error}");
        if let Some(hint) = human_hint(error) {
            eprintln!("hint: {hint}");
        }
    }
}

fn human_hint(error: &CliError) -> Option<&'static str> {
    match error {
        CliError::Usage(_) | CliError::Clap(_) => Some(
            "run `rsproxy --help` for the workflow overview or `rsproxy help <COMMAND>` for examples",
        ),
        CliError::Config(_) | CliError::Logging(_) => Some(
            "run `rsproxy run --help` to review accepted formats, defaults, and configuration precedence",
        ),
        CliError::Control(_) => Some(
            "ensure the daemon is running and that --api, --storage, and --api-token select the same instance",
        ),
        CliError::Engine(_)
        | CliError::ListenerStopped { .. }
        | CliError::ListenerSupervision { .. } => Some(
            "retry in the foreground with `RSPROXY_LOG=rsproxy=debug rsproxy run` to see detailed diagnostics",
        ),
        CliError::Platform(_) | CliError::ExternalCommand { .. } => Some(
            "preview trust or system-proxy mutations with --dry-run and verify native tool permissions",
        ),
        CliError::RuleModel(_) | CliError::RuleStore(_) | CliError::RuleDiagnostics(_) => Some(
            "run `rsproxy rules check FILE` for focused diagnostics and `rsproxy help rules` for examples",
        ),
        CliError::Io { .. } => Some(
            "verify that the reported path exists, is writable when required, and belongs to the selected storage",
        ),
        CliError::DaemonConflict(rsproxy_cli::DaemonConflict::AlreadyRunning { .. }) => {
            Some("use `rsproxy status` to inspect it or `rsproxy restart` to apply new settings")
        }
        CliError::DaemonConflict(rsproxy_cli::DaemonConflict::NotRunning { .. }) => Some(
            "start it with `rsproxy start`, or pass the same --storage/--config used by the existing instance",
        ),
        CliError::DaemonConflict(rsproxy_cli::DaemonConflict::IdentityMismatch { .. }) => Some(
            "verify the pid and selected storage; rsproxy will not terminate an unverified process",
        ),
        CliError::DaemonExited { .. } | CliError::DaemonReadinessTimeout { .. } => {
            Some("inspect the daemon log path shown above, then retry with `rsproxy run`")
        }
        CliError::DaemonStopTimeout { .. } => Some(
            "check the process before taking further action; rsproxy avoids force-killing it automatically",
        ),
        CliError::PortHeldByForeignProcess { .. } => Some(
            "identify the process holding the port and stop it, or run rsproxy on a different --port",
        ),
        CliError::Json { .. }
        | CliError::InvalidPlatformOutcome { .. }
        | CliError::InvalidRuleOperation
        | CliError::SupervisorExited => None,
    }
}

fn render_json_error(code: &str, message: &str) {
    eprintln!(
        "{}",
        serde_json::json!({
            "schema": "rsproxy.cli.error/v1",
            "ok": false,
            "error": {
                "code": code,
                "message": message,
            }
        })
    );
}
