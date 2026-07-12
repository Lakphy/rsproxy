fn main() {
    if let Err(error) = rsproxy::run_cli() {
        if std::env::args().any(|arg| arg == "--json") {
            eprintln!(
                "{}",
                serde_json::json!({
                    "schema": "rsproxy.cli.error/v1",
                    "ok": false,
                    "error": {
                        "code": "command_failed",
                        "message": error,
                    }
                })
            );
        } else {
            tracing::error!(event = "cli_failed", error = %error, "command failed");
            eprintln!("error: {error}");
        }
        std::process::exit(1);
    }
}
