use super::api_auth::{api_token_path, configure_client_api_auth, prepare_server_api_auth};
use super::command::RuntimeArgs;
use super::config::runtime_config;
use crate::app::{AppConfig, api_display, unix_api_path};
use crate::{CliError, CliResult, DaemonConflict};
use rsproxy_control::{self as control, ControlState, api_request, set_api_token};
use rsproxy_platform::process::{
    detach_daemon, force_terminate_process, parse_pid, pid_listening_on, process_alive,
    process_executable_path, terminate_process,
};
use std::env;
use std::fs::{self, OpenOptions};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

mod args;

use args::append_runtime_arguments;

/// Environment variable through which the npm shim advertises its pid to the native process.
const SUPERVISOR_PID_ENV: &str = "RSPROXY_SUPERVISOR_PID";

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

    let control_exit_tx = listener_exit_tx.clone();
    thread::spawn(move || {
        let error = match control::serve(control_listener, control_state) {
            Ok(()) => CliError::ListenerStopped {
                listener: "control",
            },
            Err(error) => error.into(),
        };
        let _ = control_exit_tx.send(error);
    });

    // When launched by the npm shim (which cannot forward an uncatchable SIGKILL), the shim
    // passes its pid so we exit with it instead of orphaning this process and its listener port.
    if let Ok(raw_pid) = env::var(SUPERVISOR_PID_ENV)
        && let Ok(supervisor_pid) = parse_pid(&raw_pid)
    {
        let watchdog_exit_tx = listener_exit_tx;
        thread::spawn(move || {
            while process_alive(supervisor_pid) {
                thread::sleep(Duration::from_millis(500));
            }
            let _ = watchdog_exit_tx.send(CliError::SupervisorExited);
        });
    }

    let config_source = config
        .config_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "defaults".to_string());
    let api_auth = if config.api_token.is_some() {
        format!(
            "token:{}",
            api_token_path(&config.engine().storage).display()
        )
    } else {
        "peer".to_string()
    };
    let mitm_mode = if config.engine().no_mitm {
        "disabled"
    } else if config.engine().strict_mitm {
        "strict"
    } else {
        "auto"
    };
    tracing::info!(
        event = "daemon_started",
        proxy_host = %config.host,
        proxy_port = config.port,
        control = %api_display(&config.api),
        storage = %config.engine().storage.display(),
        config = %config_source,
        api_auth = %api_auth,
        mitm_mode,
        rules_watch = config.engine().rules_watch,
        rules_watch_debounce_ms = config.engine().rules_watch_debounce.as_millis() as u64,
        "rsproxy running"
    );
    let outcome = listener_exit_rx
        .recv()
        .map_err(|source| CliError::ListenerSupervision { source })?;
    if matches!(outcome, CliError::SupervisorExited) {
        tracing::info!(
            event = "supervisor_exited",
            "supervising process exited; shutting down"
        );
        return Ok(());
    }
    tracing::error!(event = "daemon_listener_stopped", error = %outcome, "daemon listener stopped");
    Err(outcome)
}

pub(super) fn start_server(args: &RuntimeArgs) -> CliResult<()> {
    let mut config = runtime_config(args)?;
    validate_daemon_addresses(&config)?;
    prepare_server_api_auth(&mut config)?;
    set_api_token(config.api_token.clone());
    fs::create_dir_all(config.engine().storage.join("run")).map_err(|source| {
        CliError::io(
            format!(
                "create runtime directory {}",
                config.engine().storage.join("run").display()
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

    // Probe only once no tracked daemon owns the pidfile, so an already-running instance still
    // reports the clearer "already running" conflict rather than a bind error.
    ensure_proxy_port_available(&config)?;

    let log_path = config.engine().storage.join("run/rsproxy.log");
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
    // The detached daemon reparents to init, so it must not inherit a shim watchdog target it
    // would immediately observe as dead.
    command.env_remove(SUPERVISOR_PID_ENV);
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
                    format!(
                        "token:{}",
                        api_token_path(&config.engine().storage).display()
                    )
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
            return reclaim_orphan_or_not_running(&config, &pid_path);
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
    config.engine().storage.join("run/rsproxy.pid")
}

/// Recovers a proxy port held by an untracked rsproxy orphan (e.g. a `run` child whose npm
/// shim was `SIGKILL`ed), since such a process writes no pidfile for `stop` to follow.
///
/// Refuses to touch a non-rsproxy process, and falls back to `NotRunning` when the port is free.
fn reclaim_orphan_or_not_running(config: &AppConfig, pid_path: &Path) -> CliResult<()> {
    let addr = format!("{}:{}", config.host, config.port);
    let Some(pid) = pid_listening_on(&config.host, config.port) else {
        return Err(DaemonConflict::NotRunning {
            pid_path: pid_path.to_path_buf(),
        }
        .into());
    };
    if !process_is_rsproxy(pid) {
        return Err(CliError::PortHeldByForeignProcess { addr, pid });
    }
    terminate_process(pid)?;
    for _ in 0..50 {
        if !process_alive(pid) {
            println!("reclaimed {addr} (stopped orphan pid={pid})");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    force_terminate_process(pid)?;
    for _ in 0..50 {
        if !process_alive(pid) {
            println!("reclaimed {addr} (killed orphan pid={pid})");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(CliError::DaemonStopTimeout { pid })
}

/// Reports whether `pid`'s executable shares this binary's file name, used to avoid signalling
/// an unrelated process that merely happens to hold the port.
fn process_is_rsproxy(pid: u32) -> bool {
    let Some(candidate) = process_executable_path(pid) else {
        return false;
    };
    let Ok(current) = env::current_exe() else {
        return false;
    };
    match (candidate.file_stem(), current.file_stem()) {
        (Some(candidate), Some(expected)) => candidate == expected,
        _ => false,
    }
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

fn ensure_proxy_port_available(config: &AppConfig) -> CliResult<()> {
    let proxy_addr = format!("{}:{}", config.host, config.port);
    // Probe the proxy port up front so `start` surfaces `Address already in use` directly
    // instead of forcing the user to read run/rsproxy.log after a "daemon exited" message.
    // The daemon rebinds after this probe drops; the DaemonExited path still covers the race.
    match TcpListener::bind(&proxy_addr) {
        Ok(_) => Ok(()),
        Err(source) => Err(CliError::io(
            format!("bind proxy listener {proxy_addr}"),
            source,
        )),
    }
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
            == Some(config.engine().storage.to_string_lossy().as_ref()))
    .then_some(status)
}
