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
