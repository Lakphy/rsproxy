pub(super) fn help_requested(args: &[String]) -> bool {
    args.iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
}

pub(super) fn print_help_command(args: &[String]) -> Result<(), String> {
    let Some(command) = args.first().filter(|arg| !is_help_flag(arg)) else {
        print_help();
        return Ok(());
    };
    print_command_help(command, &args[1..])
}

pub(super) fn print_command_help(command: &str, args: &[String]) -> Result<(), String> {
    match command {
        "run" | "start" | "stop" | "restart" => print_runtime_help(command),
        "status" => print_status_help(),
        "rules" => print_rules_help(subcommand(args))?,
        "values" => print_values_help(subcommand(args))?,
        "trace" => print_trace_help(subcommand(args))?,
        "tui" => print_tui_help(),
        "replay" => print_replay_help(),
        "ca" => print_ca_help(subcommand(args))?,
        "proxy" => print_proxy_help(subcommand(args))?,
        "completions" => print_completions_help(),
        "help" | "--help" | "-h" => print_help(),
        other => return Err(format!("unknown command `{other}`")),
    }
    Ok(())
}

fn is_help_flag(arg: &str) -> bool {
    matches!(arg, "--help" | "-h")
}

fn subcommand(args: &[String]) -> Option<&str> {
    args.first()
        .map(String::as_str)
        .filter(|arg| !is_help_flag(arg))
}

pub(super) fn print_help() {
    println!(
        "rsproxy\n\nUSAGE:\n  rsproxy --version\n  rsproxy run|start|stop|restart [-p PORT] [--host HOST] [--api HOST:PORT|unix:/path.sock|pipe:NAME] [--api-token TOKEN] [--storage DIR] [--config FILE] [--watch] [--watch-debounce-ms MS] [--proxy-auth user:pass] [--no-mitm|--strict-mitm] [--mitm-cert-cache-capacity N] [--mitm-failure-cache-capacity N] [--mitm-failure-ttl-seconds SECONDS] [--connect-probe-timeout-ms MS] [--max-header-size SIZE] [--max-header-count N] [--body-buffer-limit SIZE] [--h1-pool-max-active-per-key N] [--h1-pool-wait-timeout-ms MS] [--h2-pool-max-active-streams-per-key N] [--h2-pool-wait-timeout-ms MS] [--dns-timeout-ms MS] [--dns-cache SECONDS] [--dns-server IP[:PORT]] [--tcp-connect-timeout-ms MS] [--client-tls-handshake-timeout-ms MS] [--upstream-tls-handshake-timeout-ms MS] [--upstream-ttfb-timeout-ms MS] [--request-timeout-ms MS] [--trace-body-limit SIZE] [--trace-filter headers-only|media|full] [--trace-queue-capacity N] [--trace-mem-budget SIZE] [--trace-segment-size SIZE] [--trace-disk-budget SIZE] [--trace-spill-compression none|zstd[:level]]\n  rsproxy status [--api HOST:PORT|unix:/path.sock|pipe:NAME] [--api-token TOKEN] [--storage DIR] [--config FILE]\n  rsproxy rules check [FILE]\n  rsproxy rules ls [--json]\n  rsproxy rules cat|edit|set|rm <group> [--file FILE]\n  rsproxy rules enable|disable <group>\n  rsproxy rules stats|bench\n  rsproxy rules test <url> [-X METHOD] [-H 'Name: value']... [--body TEXT] [--client-ip IP] [--server-ip IP]\n  rsproxy values ls|cat|set|rm <key>\n  rsproxy trace ls|get|follow|stats|clear|export\n  rsproxy tui [--api HOST:PORT|unix:/path.sock|pipe:NAME] [--once] [--limit N] [--filter TEXT] [--tab overview|headers|body|rules] [--interval-ms N]\n  rsproxy replay <id>\n  rsproxy ca init [--force] [--name NAME]|status [--keychain FILE]|export [-o FILE]|issue <host> [--force]|install [--keychain FILE]|uninstall [--keychain FILE]\n  rsproxy proxy status [--platform macos|windows|linux] [--service NAME] [--dry-run]\n  rsproxy proxy on|off (--service NAME|--all) [--platform macos|windows|linux] [--host HOST] [--port PORT] [--bypass LIST] [--dry-run]\n  rsproxy completions <bash|zsh|fish|powershell>\n\nCONFIG:\n  Runtime-aware commands read ~/.rsproxy/config.toml by default. --config FILE selects another file; CLI options override file values."
    );
    println!(
        "\nRULES TEST RESPONSE CONTEXT:\n  --response-status CODE\n  --response-header 'Name: value'  (repeatable)"
    );
}

fn print_runtime_help(command: &str) {
    println!(
        "rsproxy {command}\n\nUSAGE:\n  rsproxy {command} [OPTIONS]\n\nCORE OPTIONS:\n  -p, --port PORT\n  --host HOST\n  --api HOST:PORT|unix:/path.sock|pipe:NAME\n  --api-token TOKEN\n  --storage DIR\n  --config FILE\n  --watch\n  --watch-debounce-ms MS\n  --proxy-auth USER:PASS\n\nMITM/POOL OPTIONS:\n  --no-mitm | --strict-mitm\n  --mitm-cert-cache-capacity N\n  --mitm-failure-cache-capacity N\n  --mitm-failure-ttl-seconds SECONDS\n  --connect-probe-timeout-ms MS\n  --h1-pool-max-active-per-key N\n  --h1-pool-wait-timeout-ms MS\n  --h2-pool-max-active-streams-per-key N\n  --h2-pool-wait-timeout-ms MS\n\nLIMIT/TIMEOUT OPTIONS:\n  --max-header-size SIZE\n  --max-header-count N\n  --body-buffer-limit SIZE\n  --dns-timeout-ms MS\n  --dns-cache SECONDS\n  --dns-server IP[:PORT]\n  --tcp-connect-timeout-ms MS\n  --client-tls-handshake-timeout-ms MS\n  --upstream-tls-handshake-timeout-ms MS\n  --upstream-ttfb-timeout-ms MS\n  --request-timeout-ms MS\n\nTRACE OPTIONS:\n  --trace-body-limit SIZE\n  --trace-filter headers-only|media|full\n  --trace-queue-capacity N\n  --trace-mem-budget SIZE\n  --trace-segment-size SIZE\n  --trace-disk-budget SIZE\n  --trace-spill-compression none|zstd[:level]"
    );
}

fn print_status_help() {
    println!(
        "rsproxy status\n\nUSAGE:\n  rsproxy status [--api ENDPOINT] [--api-token TOKEN] [--storage DIR] [--config FILE]"
    );
}

fn print_rules_help(sub: Option<&str>) -> Result<(), String> {
    let usage = match sub {
        None => {
            "rsproxy rules <check|ls|cat|edit|set|rm|enable|disable|stats|bench|test> [OPTIONS]"
        }
        Some("check") => "rsproxy rules check [FILE]",
        Some("ls") => "rsproxy rules ls [--json] [CLIENT OPTIONS]",
        Some("cat") => "rsproxy rules cat <GROUP> [--json] [CLIENT OPTIONS]",
        Some("edit") => "rsproxy rules edit <GROUP> [CLIENT OPTIONS]",
        Some("set") => "rsproxy rules set <GROUP> [--file FILE] [CLIENT OPTIONS]",
        Some("rm") => "rsproxy rules rm <GROUP> [CLIENT OPTIONS]",
        Some("enable") => "rsproxy rules enable <GROUP> [CLIENT OPTIONS]",
        Some("disable") => "rsproxy rules disable <GROUP> [CLIENT OPTIONS]",
        Some("stats") => "rsproxy rules stats [--json] [CLIENT OPTIONS]",
        Some("bench") => {
            "rsproxy rules bench [--url URL] [--iterations N] [--warmup N] [--json] [REQUEST OPTIONS] [CLIENT OPTIONS]"
        }
        Some("test") => {
            "rsproxy rules test <URL> [-X METHOD] [-H 'NAME: VALUE']... [--body TEXT] [--client-ip IP] [--server-ip IP] [--response-status CODE] [--response-header 'NAME: VALUE']... [--json] [CLIENT OPTIONS]"
        }
        Some(other) => return Err(format!("unknown rules command `{other}`")),
    };
    println!(
        "rsproxy rules\n\nUSAGE:\n  {usage}\n\nCLIENT OPTIONS:\n  --api ENDPOINT\n  --api-token TOKEN\n  --storage DIR\n  --config FILE"
    );
    Ok(())
}

fn print_values_help(sub: Option<&str>) -> Result<(), String> {
    let usage = match sub {
        None => "rsproxy values <ls|cat|set|rm> [OPTIONS]",
        Some("ls") => "rsproxy values ls [--json] [CLIENT OPTIONS]",
        Some("cat") => "rsproxy values cat <KEY> [--json] [CLIENT OPTIONS]",
        Some("set") => "rsproxy values set <KEY> [--file FILE] [CLIENT OPTIONS]",
        Some("rm") => "rsproxy values rm <KEY> [CLIENT OPTIONS]",
        Some(other) => return Err(format!("unknown values command `{other}`")),
    };
    println!(
        "rsproxy values\n\nUSAGE:\n  {usage}\n\nCLIENT OPTIONS:\n  --api ENDPOINT\n  --api-token TOKEN\n  --storage DIR\n  --config FILE"
    );
    Ok(())
}

fn print_trace_help(sub: Option<&str>) -> Result<(), String> {
    let usage = match sub {
        None => "rsproxy trace <ls|get|follow|stats|clear|export> [OPTIONS]",
        Some("ls") => "rsproxy trace ls [-n N|--limit N] [--json] [CLIENT OPTIONS]",
        Some("get") => "rsproxy trace get <ID> [CLIENT OPTIONS]",
        Some("follow") => "rsproxy trace follow [--count N] [--poll-ms MS] [CLIENT OPTIONS]",
        Some("stats") => "rsproxy trace stats [CLIENT OPTIONS]",
        Some("clear") => "rsproxy trace clear [CLIENT OPTIONS]",
        Some("export") => "rsproxy trace export [--har] [-o FILE|--output FILE] [CLIENT OPTIONS]",
        Some(other) => return Err(format!("unknown trace command `{other}`")),
    };
    println!(
        "rsproxy trace\n\nUSAGE:\n  {usage}\n\nCLIENT OPTIONS:\n  --api ENDPOINT\n  --api-token TOKEN\n  --storage DIR\n  --config FILE"
    );
    Ok(())
}

fn print_tui_help() {
    println!(
        "rsproxy tui\n\nUSAGE:\n  rsproxy tui [--once] [--json] [-n N|--limit N] [--filter TEXT] [--tab overview|headers|body|rules] [--interval-ms MS] [CLIENT OPTIONS]"
    );
}

fn print_replay_help() {
    println!("rsproxy replay\n\nUSAGE:\n  rsproxy replay <SESSION_ID> [CLIENT OPTIONS]");
}

fn print_ca_help(sub: Option<&str>) -> Result<(), String> {
    let usage = match sub {
        None => "rsproxy ca <init|status|export|issue|install|uninstall> [OPTIONS]",
        Some("init") => "rsproxy ca init [--force] [--name NAME] [--storage DIR]",
        Some("status") => "rsproxy ca status [--keychain FILE] [--json] [--storage DIR]",
        Some("export") => "rsproxy ca export [-o FILE|--out FILE] [--storage DIR]",
        Some("issue") => "rsproxy ca issue <HOST> [--force] [--storage DIR]",
        Some("install") => {
            "rsproxy ca install [--keychain FILE] [--dry-run] [--json] [--storage DIR]"
        }
        Some("uninstall") => {
            "rsproxy ca uninstall [--keychain FILE] [--dry-run] [--json] [--storage DIR]"
        }
        Some(other) => return Err(format!("unknown ca command `{other}`")),
    };
    println!("rsproxy ca\n\nUSAGE:\n  {usage}");
    Ok(())
}

fn print_proxy_help(sub: Option<&str>) -> Result<(), String> {
    let usage = match sub {
        None => "rsproxy proxy <status|on|off> [OPTIONS]",
        Some("status") => {
            "rsproxy proxy status [--platform macos|windows|linux] [--service NAME] [--dry-run]"
        }
        Some("on") | Some("off") => {
            "rsproxy proxy on|off (--service NAME|--all) [--platform macos|windows|linux] [--host HOST] [--port PORT] [--bypass LIST] [--dry-run]"
        }
        Some(other) => return Err(format!("unknown proxy command `{other}`")),
    };
    println!("rsproxy proxy\n\nUSAGE:\n  {usage}");
    Ok(())
}

fn print_completions_help() {
    println!("rsproxy completions\n\nUSAGE:\n  rsproxy completions <bash|zsh|fish|powershell>");
}
