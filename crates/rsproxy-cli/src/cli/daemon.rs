use super::*;

mod process;

use process::*;

pub(super) fn run_server(args: Vec<String>) -> Result<(), String> {
    let mut config = runtime_config(&args)?;
    prepare_server_api_auth(&mut config)?;
    set_api_token(config.api_token.clone());

    let rules =
        crate::rule_store::RuleStore::load(&config.storage).map_err(|error| error.to_string())?;
    let dns_resolver = Arc::new(dns::DnsResolver::new(&config).map_err(|err| err.to_string())?);
    let proxy_addr = format!("{}:{}", config.host, config.port);
    let proxy_listener = proxy::bind(&proxy_addr)
        .map_err(|error| format!("bind proxy listener {proxy_addr}: {error}"))?;
    config.port = proxy_listener
        .local_addr()
        .map_err(|error| format!("read proxy listener address: {error}"))?
        .port();
    let control_listener = control::bind(&config.api)
        .map_err(|error| format!("bind control listener {}: {error}", config.api))?;
    config.api = control_listener
        .endpoint()
        .map_err(|error| format!("read control listener address: {error}"))?;

    let trace_spill = if config.trace_disk_budget == 0 {
        None
    } else {
        Some(
            rsproxy_trace::TraceSpillConfig::new(
                config.storage.join("trace"),
                config.trace_spill_segment_size as u64,
                config.trace_disk_budget as u64,
            )
            .with_compression(config.trace_spill_compression),
        )
    };
    let state = SharedState {
        config: config.clone(),
        rules,
        trace: rsproxy_trace::TraceStore::new_with_config(rsproxy_trace::TraceStoreConfig {
            max_sessions: 4096,
            queue_capacity: config.trace_queue_capacity,
            memory_budget_bytes: config.trace_memory_budget,
            queue_memory_budget_bytes: None,
            body_limit: config.trace_body_limit,
            spill: trace_spill,
        }),
        mitm_cert_cache: Arc::new(std::sync::Mutex::new(MitmCertCache::new(
            config.mitm_cert_cache_capacity,
        ))),
        mitm_failures: Arc::new(std::sync::Mutex::new(MitmFailureCache::new(
            config.mitm_failure_cache_capacity,
            config.mitm_failure_ttl,
        ))),
        upstream_roots: Arc::new(std::sync::OnceLock::new()),
        dns_resolver,
        started_ms: rsproxy_trace::now_millis(),
    };
    let _rule_watch = if config.rules_watch {
        Some(
            state
                .rules
                .watch(config.rules_watch_debounce)
                .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    proxy::initialize_upstream_roots(&state);

    let (listener_exit_tx, listener_exit_rx) = std::sync::mpsc::channel::<String>();
    let proxy_state = state.clone();
    let proxy_exit_tx = listener_exit_tx.clone();
    thread::spawn(move || {
        let message = match proxy::serve(proxy_listener, proxy_state) {
            Ok(()) => "proxy listener stopped unexpectedly".to_string(),
            Err(error) => format!("proxy listener stopped: {error}"),
        };
        let _ = proxy_exit_tx.send(message);
    });

    let control_state = state.clone();
    let control_exit_tx = listener_exit_tx;
    thread::spawn(move || {
        let message = match control::serve(control_listener, control_state) {
            Ok(()) => "control listener stopped unexpectedly".to_string(),
            Err(error) => format!("control listener stopped: {error}"),
        };
        let _ = control_exit_tx.send(message);
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
        .unwrap_or_else(|_| "listener supervision channel closed".to_string());
    tracing::error!(event = "daemon_listener_stopped", error = %error, "daemon listener stopped");
    Err(error)
}

pub(super) fn start_server(args: Vec<String>) -> Result<(), String> {
    let mut config = runtime_config(&args)?;
    validate_daemon_addresses(&config)?;
    prepare_server_api_auth(&mut config)?;
    set_api_token(config.api_token.clone());
    fs::create_dir_all(config.storage.join("run")).map_err(|e| e.to_string())?;
    let pid_path = pid_path(&config);
    if let Ok(raw_pid) = fs::read_to_string(&pid_path) {
        if let Ok(pid) = parse_pid(&raw_pid)
            && process_alive(pid)
        {
            if daemon_identity_matches(&config) {
                return Err(format!(
                    "rsproxy already running with pid {pid} ({})",
                    pid_path.display()
                ));
            }
            return Err(format!(
                "pidfile {} references live process {pid}, but daemon identity could not be verified; refusing to replace it",
                pid_path.display()
            ));
        }
        let _ = fs::remove_file(&pid_path);
    }

    let log_path = config.storage.join("run/rsproxy.log");
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| format!("open log {}: {e}", log_path.display()))?;
    let log_err = log.try_clone().map_err(|e| e.to_string())?;
    let mut command = Command::new(env::current_exe().map_err(|e| e.to_string())?);
    command
        .arg("run")
        .args(args)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    detach_daemon(&mut command);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    if let Err(error) = fs::write(&pid_path, child.id().to_string()) {
        let _ = child.kill();
        let _ = child.wait();
        remove_runtime_files(&config, &pid_path);
        return Err(format!("write pidfile {}: {error}", pid_path.display()));
    }

    for _ in 0..50 {
        if let Ok(Some(status)) = child.try_wait() {
            remove_runtime_files(&config, &pid_path);
            return Err(format!(
                "rsproxy exited during start with status {status}; see {}",
                log_path.display()
            ));
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
    let error = format!(
        "rsproxy did not become ready; pid={} log={}",
        child.id(),
        log_path.display()
    );
    let _ = child.kill();
    let _ = child.wait();
    remove_runtime_files(&config, &pid_path);
    Err(error)
}

pub(super) fn stop_server(args: Vec<String>) -> Result<(), String> {
    let config = runtime_config(&args)?;
    let pid_path = pid_path(&config);
    let raw_pid = fs::read_to_string(&pid_path)
        .map_err(|_| format!("pidfile not found: {}", pid_path.display()))?;
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
    configure_client_api_auth(&args)?;
    if !daemon_identity_matches(&config) {
        return Err(format!(
            "pidfile {} references live process {pid}, but daemon identity could not be verified; refusing to terminate it",
            pid_path.display()
        ));
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
    Err(format!("pid {pid} did not stop in time"))
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

fn validate_daemon_addresses(config: &AppConfig) -> Result<(), String> {
    if config.port == 0 {
        return Err(
            "daemon mode requires a non-zero proxy port; use `run` for an ephemeral port"
                .to_string(),
        );
    }
    if unix_api_path(&config.api).is_none()
        && config
            .api
            .rsplit_once(':')
            .is_some_and(|(_, port)| port == "0")
    {
        return Err(
            "daemon mode requires a non-zero control port; use `run` for an ephemeral port"
                .to_string(),
        );
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
