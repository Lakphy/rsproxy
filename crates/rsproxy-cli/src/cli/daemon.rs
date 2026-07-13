use super::api_auth::{api_token_path, configure_client_api_auth, prepare_server_api_auth};
use super::command::RuntimeArgs;
use super::config::runtime_config;
use crate::app::{AppConfig, api_display, unix_api_path};
use crate::{CliError, CliResult, DaemonConflict};
use rsproxy_control::{self as control, ControlState, api_request, set_api_token};
use rsproxy_platform::process::{detach_daemon, parse_pid, process_alive, terminate_process};
use std::env;
use std::fs::{self, OpenOptions};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

pub(super) fn run_server(args: &RuntimeArgs) -> CliResult<()> {
    let mut config = runtime_config(args)?;
    prepare_server_api_auth(&mut config)?;
    set_api_token(config.api_token.clone());

    let proxy_addr = format!("{}:{}", config.host, config.port);
    let proxy_listener = TcpListener::bind(&proxy_addr)
        .map_err(|source| CliError::io(format!("bind proxy listener {proxy_addr}"), source))?;
    config.port = proxy_listener
        .local_addr()
        .map_err(|source| CliError::io("read proxy listener address", source))?
        .port();
    let control_listener = control::bind(&config.api)?;
    config.api = control_listener.endpoint()?;

    let state = rsproxy_engine::SharedState::new(config.proxy_config_with_ca_material()?)?;
    let control_state = ControlState::new(config.control_options(), state.handle());

    let (listener_exit_tx, listener_exit_rx) = std::sync::mpsc::channel::<CliError>();
    let proxy_state = state;
    let proxy_exit_tx = listener_exit_tx.clone();
    thread::spawn(move || {
        let error = match rsproxy_engine::serve(proxy_listener, proxy_state) {
            Ok(()) => CliError::ListenerStopped { listener: "proxy" },
            Err(error) => error.into(),
        };
        let _ = proxy_exit_tx.send(error);
    });

    let control_exit_tx = listener_exit_tx;
    thread::spawn(move || {
        let error = match control::serve(control_listener, control_state) {
            Ok(()) => CliError::ListenerStopped {
                listener: "control",
            },
            Err(error) => error.into(),
        };
        let _ = control_exit_tx.send(error);
    });

    let config_source = config
        .config_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "defaults".to_string());
    let api_auth = if config.api_token.is_some() {
        format!("token:{}", api_token_path(&config.storage).display())
    } else {
        "peer".to_string()
    };
    let mitm_mode = if config.no_mitm {
        "disabled"
    } else if config.strict_mitm {
        "strict"
    } else {
        "auto"
    };
    tracing::info!(
        event = "daemon_started",
        proxy_host = %config.host,
        proxy_port = config.port,
        control = %api_display(&config.api),
        storage = %config.storage.display(),
        config = %config_source,
        api_auth = %api_auth,
        mitm_mode,
        rules_watch = config.rules_watch,
        rules_watch_debounce_ms = config.rules_watch_debounce.as_millis() as u64,
        "rsproxy running"
    );
    let error = listener_exit_rx
        .recv()
        .map_err(|source| CliError::ListenerSupervision { source })?;
    tracing::error!(event = "daemon_listener_stopped", error = %error, "daemon listener stopped");
    Err(error)
}

pub(super) fn start_server(args: &RuntimeArgs) -> CliResult<()> {
    let mut config = runtime_config(args)?;
    validate_daemon_addresses(&config)?;
    prepare_server_api_auth(&mut config)?;
    set_api_token(config.api_token.clone());
    fs::create_dir_all(config.storage.join("run")).map_err(|source| {
        CliError::io(
            format!(
                "create runtime directory {}",
                config.storage.join("run").display()
            ),
            source,
        )
    })?;
    let pid_path = pid_path(&config);
    match fs::read_to_string(&pid_path) {
        Ok(raw_pid) => {
            if let Ok(pid) = parse_pid(&raw_pid)
                && process_alive(pid)
            {
                if daemon_identity_matches(&config) {
                    return Err(DaemonConflict::AlreadyRunning {
                        pid,
                        pid_path: pid_path.clone(),
                    }
                    .into());
                }
                return Err(DaemonConflict::IdentityMismatch {
                    pid,
                    pid_path: pid_path.clone(),
                    operation: "replace it",
                }
                .into());
            }
            let _ = fs::remove_file(&pid_path);
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(CliError::io(
                format!("read pidfile {}", pid_path.display()),
                source,
            ));
        }
    }

    let log_path = config.storage.join("run/rsproxy.log");
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|source| CliError::io(format!("open log {}", log_path.display()), source))?;
    let log_err = log
        .try_clone()
        .map_err(|source| CliError::io(format!("clone log {}", log_path.display()), source))?;
    let executable =
        env::current_exe().map_err(|source| CliError::io("resolve current executable", source))?;
    let mut command = Command::new(executable);
    command.arg("run");
    append_runtime_arguments(&mut command, args);
    command
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    detach_daemon(&mut command);
    let mut child = command
        .spawn()
        .map_err(|source| CliError::io("spawn rsproxy daemon", source))?;
    if let Err(error) = fs::write(&pid_path, child.id().to_string()) {
        let _ = child.kill();
        let _ = child.wait();
        remove_runtime_files(&config, &pid_path);
        return Err(CliError::io(
            format!("write pidfile {}", pid_path.display()),
            error,
        ));
    }

    for _ in 0..50 {
        if let Some(status) = child
            .try_wait()
            .map_err(|source| CliError::io("poll rsproxy daemon", source))?
        {
            remove_runtime_files(&config, &pid_path);
            return Err(CliError::DaemonExited {
                status,
                log_path: log_path.clone(),
            });
        }
        if daemon_ready(&config) {
            println!(
                "started pid={} proxy=http://{}:{} api={} config={} api_auth={} log={}",
                child.id(),
                config.host,
                config.port,
                api_display(&config.api),
                config
                    .config_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "defaults".to_string()),
                if config.api_token.is_some() {
                    format!("token:{}", api_token_path(&config.storage).display())
                } else {
                    "peer".to_string()
                },
                log_path.display()
            );
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    let error = CliError::DaemonReadinessTimeout {
        pid: child.id(),
        log_path,
    };
    let _ = child.kill();
    let _ = child.wait();
    remove_runtime_files(&config, &pid_path);
    Err(error)
}

pub(super) fn stop_server(args: &RuntimeArgs) -> CliResult<()> {
    let config = runtime_config(args)?;
    let pid_path = pid_path(&config);
    let raw_pid = match fs::read_to_string(&pid_path) {
        Ok(raw_pid) => raw_pid,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Err(DaemonConflict::NotRunning {
                pid_path: pid_path.clone(),
            }
            .into());
        }
        Err(source) => {
            return Err(CliError::io(
                format!("read pidfile {}", pid_path.display()),
                source,
            ));
        }
    };
    let pid = match parse_pid(&raw_pid) {
        Ok(pid) => pid,
        Err(_) => {
            remove_runtime_files(&config, &pid_path);
            println!("removed stale pidfile {}", pid_path.display());
            return Ok(());
        }
    };
    if !process_alive(pid) {
        remove_runtime_files(&config, &pid_path);
        println!("removed stale pidfile {}", pid_path.display());
        return Ok(());
    }
    configure_client_api_auth(&args.client)?;
    if !daemon_identity_matches(&config) {
        return Err(DaemonConflict::IdentityMismatch {
            pid,
            pid_path: pid_path.clone(),
            operation: "terminate it",
        }
        .into());
    }
    terminate_process(pid)?;
    for _ in 0..50 {
        if !process_alive(pid) {
            remove_runtime_files(&config, &pid_path);
            println!("stopped pid={pid}");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(CliError::DaemonStopTimeout { pid })
}

pub(super) fn pid_path(config: &AppConfig) -> PathBuf {
    config.storage.join("run/rsproxy.pid")
}

fn remove_runtime_files(config: &AppConfig, pid_path: &Path) {
    let _ = fs::remove_file(pid_path);
    if let Some(socket_path) = unix_api_path(&config.api) {
        let _ = fs::remove_file(socket_path);
    }
}

fn validate_daemon_addresses(config: &AppConfig) -> CliResult<()> {
    if config.port == 0 {
        return Err(CliError::Usage(
            "daemon mode requires a non-zero proxy port; use `run` for an ephemeral port"
                .to_string(),
        ));
    }
    if unix_api_path(&config.api).is_none()
        && config
            .api
            .rsplit_once(':')
            .is_some_and(|(_, port)| port == "0")
    {
        return Err(CliError::Usage(
            "daemon mode requires a non-zero control port; use `run` for an ephemeral port"
                .to_string(),
        ));
    }
    Ok(())
}

fn daemon_ready(config: &AppConfig) -> bool {
    daemon_status(config).is_some_and(|status| {
        status.get("proxy").and_then(|value| value.as_str())
            == Some(format!("{}:{}", config.host, config.port).as_str())
    })
}

fn daemon_identity_matches(config: &AppConfig) -> bool {
    daemon_status(config).is_some()
}

fn daemon_status(config: &AppConfig) -> Option<serde_json::Value> {
    let body = api_request("GET", &config.api, "/api/status", "").ok()?;
    let status = serde_json::from_str::<serde_json::Value>(&body).ok()?;
    (status.get("status").and_then(|value| value.as_str()) == Some("running")
        && status.get("storage").and_then(|value| value.as_str())
            == Some(config.storage.to_string_lossy().as_ref()))
    .then_some(status)
}

fn append_runtime_arguments(command: &mut Command, args: &RuntimeArgs) {
    append_display(command, "--port", args.port);
    append_string(command, "--host", args.host.as_deref());
    append_string(command, "--api", args.client.api.as_deref());
    append_string(command, "--api-token", args.client.api_token.as_deref());
    append_path(command, "--storage", args.client.storage.as_deref());
    append_path(command, "--config", args.client.config.as_deref());
    append_flag(command, "--watch", args.watch);
    append_display(command, "--watch-debounce-ms", args.watch_debounce_ms);
    append_string(command, "--proxy-auth", args.proxy_auth.as_deref());
    append_string(
        command,
        "--max-header-size",
        args.max_header_size.as_deref(),
    );
    append_display(command, "--max-header-count", args.max_header_count);
    append_string(
        command,
        "--body-buffer-limit",
        args.body_buffer_limit.as_deref(),
    );
    append_string(
        command,
        "--trace-body-limit",
        args.trace_body_limit.as_deref(),
    );
    append_string(command, "--trace-filter", args.trace_filter.as_deref());
    append_display(command, "--trace-queue-capacity", args.trace_queue_capacity);
    append_string(
        command,
        "--trace-mem-budget",
        args.trace_mem_budget.as_deref(),
    );
    append_string(
        command,
        "--trace-segment-size",
        args.trace_segment_size.as_deref(),
    );
    append_string(
        command,
        "--trace-disk-budget",
        args.trace_disk_budget.as_deref(),
    );
    append_string(
        command,
        "--trace-spill-compression",
        args.trace_spill_compression.as_deref(),
    );
    append_flag(command, "--no-mitm", args.no_mitm);
    append_flag(command, "--strict-mitm", args.strict_mitm);
    append_display(
        command,
        "--mitm-cert-cache-capacity",
        args.mitm_cert_cache_capacity,
    );
    append_display(
        command,
        "--mitm-failure-cache-capacity",
        args.mitm_failure_cache_capacity,
    );
    append_display(
        command,
        "--mitm-failure-ttl-seconds",
        args.mitm_failure_ttl_seconds,
    );
    append_display(
        command,
        "--connect-probe-timeout-ms",
        args.connect_probe_timeout_ms,
    );
    append_display(
        command,
        "--h1-pool-max-active-per-key",
        args.h1_pool_max_active_per_key,
    );
    append_display(
        command,
        "--h1-pool-wait-timeout-ms",
        args.h1_pool_wait_timeout_ms,
    );
    append_display(
        command,
        "--h2-pool-max-active-streams-per-key",
        args.h2_pool_max_active_streams_per_key,
    );
    append_display(
        command,
        "--h2-pool-wait-timeout-ms",
        args.h2_pool_wait_timeout_ms,
    );
    append_display(
        command,
        "--tcp-connect-timeout-ms",
        args.tcp_connect_timeout_ms,
    );
    append_display(command, "--dns-timeout-ms", args.dns_timeout_ms);
    append_display(command, "--dns-cache", args.dns_cache);
    for server in &args.dns_server {
        command.args(["--dns-server", server]);
    }
    append_display(
        command,
        "--client-tls-handshake-timeout-ms",
        args.client_tls_handshake_timeout_ms,
    );
    append_display(
        command,
        "--upstream-tls-handshake-timeout-ms",
        args.upstream_tls_handshake_timeout_ms,
    );
    append_display(
        command,
        "--upstream-ttfb-timeout-ms",
        args.upstream_ttfb_timeout_ms,
    );
    append_display(command, "--request-timeout-ms", args.request_timeout_ms);
    append_flag(command, "--no-trace-body", args.no_trace_body);
}

fn append_string(command: &mut Command, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        command.args([name, value]);
    }
}

fn append_path(command: &mut Command, name: &str, value: Option<&Path>) {
    if let Some(value) = value {
        command.arg(name).arg(value);
    }
}

fn append_display<T: ToString>(command: &mut Command, name: &str, value: Option<T>) {
    if let Some(value) = value {
        command.arg(name).arg(value.to_string());
    }
}

fn append_flag(command: &mut Command, name: &str, enabled: bool) {
    if enabled {
        command.arg(name);
    }
}
