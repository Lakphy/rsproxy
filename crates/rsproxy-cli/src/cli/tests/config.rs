use super::super::*;

fn temp_config(name: &str, text: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rsproxy-config-{name}-{}-{}.toml",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    fs::write(&path, text).unwrap();
    path
}

#[test]
fn config_file_is_loaded_before_cli_overrides() {
    let path = temp_config(
        "precedence",
        r#"
host = "0.0.0.0"
port = 18080
api = "127.0.0.1:18081"
api_token = "0123456789abcdef0123456789abcdef"
storage = "/tmp/rsproxy-from-config"
watch = false
watch_debounce_ms = 500
proxy_auth = "alice:secret"
max_header_size = "512kb"
max_header_count = 300
body_buffer_limit = "4mb"
trace_body_limit = "8kb"
	trace_filter = "full"
	trace_queue_capacity = 33
	trace_mem_budget = "48mb"
	trace_segment_size = 1048576
trace_disk_budget = "2mb"
	trace_spill_compression = "zstd:2"
	no_mitm = false
	strict_mitm = false
	mitm_cert_cache_capacity = 17
	mitm_failure_cache_capacity = 19
	mitm_failure_ttl_seconds = 31
	connect_probe_timeout_ms = 275
h1_pool_max_active_per_key = 11
h1_pool_wait_timeout_ms = 1200
h2_pool_max_active_streams_per_key = 13
h2_pool_wait_timeout_ms = 1300
dns_timeout_ms = 1400
dns_cache = 23
dns_server = ["127.0.0.1:5353", "1.1.1.1"]
tcp_connect_timeout_ms = 1500
client_tls_handshake_timeout_ms = 1600
upstream_tls_handshake_timeout_ms = 1700
upstream_ttfb_timeout_ms = 1800
request_timeout_ms = 1900
"#,
    );
    let args = vec![
        "--config".to_string(),
        path.display().to_string(),
        "--port".to_string(),
        "28080".to_string(),
        "--watch".to_string(),
        "--watch-debounce-ms".to_string(),
        "75".to_string(),
        "--trace-filter".to_string(),
        "media".to_string(),
        "--body-buffer-limit".to_string(),
        "6mb".to_string(),
        "--strict-mitm".to_string(),
        "--mitm-failure-cache-capacity".to_string(),
        "23".to_string(),
        "--mitm-failure-ttl-seconds".to_string(),
        "41".to_string(),
        "--connect-probe-timeout-ms".to_string(),
        "325".to_string(),
        "--dns-server".to_string(),
        "9.9.9.9".to_string(),
    ];
    let config = runtime_config(&args).unwrap();

    assert_eq!(config.config_path.as_deref(), Some(path.as_path()));
    assert_eq!(config.host, "0.0.0.0");
    assert_eq!(config.port, 28080);
    assert_eq!(config.api, "127.0.0.1:18081");
    assert_eq!(
        config.api_token.as_deref(),
        Some("0123456789abcdef0123456789abcdef")
    );
    assert_eq!(config.storage, PathBuf::from("/tmp/rsproxy-from-config"));
    assert!(config.rules_watch);
    assert_eq!(config.rules_watch_debounce, Duration::from_millis(75));
    assert_eq!(config.proxy_auth.as_deref(), Some("alice:secret"));
    assert_eq!(config.max_header_size, 512 * 1024);
    assert_eq!(config.max_header_count, 300);
    assert_eq!(config.body_buffer_limit, 6 * 1024 * 1024);
    assert_eq!(config.trace_body_limit, 8 * 1024);
    assert!(config.trace_exclude_media_body);
    assert_eq!(config.trace_queue_capacity, 33);
    assert_eq!(config.trace_memory_budget, 48 * 1024 * 1024);
    assert_eq!(config.trace_spill_segment_size, 1024 * 1024);
    assert_eq!(config.trace_disk_budget, 2 * 1024 * 1024);
    assert_eq!(
        config.trace_spill_compression,
        rsproxy_trace::TraceSpillCompression::Zstd { level: 2 }
    );
    assert!(!config.no_mitm);
    assert!(config.strict_mitm);
    assert_eq!(config.mitm_cert_cache_capacity, 17);
    assert_eq!(config.mitm_failure_cache_capacity, 23);
    assert_eq!(config.mitm_failure_ttl, Duration::from_secs(41));
    assert_eq!(config.connect_probe_timeout, Duration::from_millis(325));
    assert_eq!(config.h1_pool_max_active_per_key, 11);
    assert_eq!(config.h1_pool_wait_timeout, Duration::from_millis(1200));
    assert_eq!(config.h2_pool_max_active_streams_per_key, 13);
    assert_eq!(config.h2_pool_wait_timeout, Duration::from_millis(1300));
    assert_eq!(config.dns_timeout, Duration::from_millis(1400));
    assert_eq!(config.dns_cache_ttl, Duration::from_secs(23));
    assert_eq!(config.dns_servers, vec!["9.9.9.9:53".parse().unwrap()]);
    assert_eq!(config.tcp_connect_timeout, Duration::from_millis(1500));
    assert_eq!(
        config.client_tls_handshake_timeout,
        Duration::from_millis(1600)
    );
    assert_eq!(
        config.upstream_tls_handshake_timeout,
        Duration::from_millis(1700)
    );
    assert_eq!(config.upstream_ttfb_timeout, Duration::from_millis(1800));
    assert_eq!(config.request_total_timeout, Duration::from_millis(1900));

    let _ = fs::remove_file(path);
}

#[test]
fn config_file_rejects_unknown_invalid_and_missing_inputs() {
    let unknown = temp_config("unknown", "porrt = 8899\n");
    let error =
        runtime_config(&["--config".to_string(), unknown.display().to_string()]).unwrap_err();
    assert!(error.contains("unknown field"));
    assert!(error.contains("porrt"));

    let invalid = temp_config("invalid", "h1_pool_wait_timeout_ms = 0\n");
    let error =
        runtime_config(&["--config".to_string(), invalid.display().to_string()]).unwrap_err();
    assert!(error.contains("h1_pool_wait_timeout_ms must be greater than zero"));

    let invalid_watch = temp_config("invalid-watch", "watch_debounce_ms = 0\n");
    let error =
        runtime_config(&["--config".to_string(), invalid_watch.display().to_string()]).unwrap_err();
    assert!(error.contains("watch_debounce_ms must be greater than zero"));
    let error =
        runtime_config_without_default(&["--watch-debounce-ms".to_string(), "0".to_string()])
            .unwrap_err();
    assert!(error.contains("--watch-debounce-ms must be greater than zero"));

    let conflicting_mitm = temp_config("conflicting-mitm", "no_mitm = true\nstrict_mitm = true\n");
    let error = runtime_config(&[
        "--config".to_string(),
        conflicting_mitm.display().to_string(),
    ])
    .unwrap_err();
    assert!(error.contains("--no-mitm and --strict-mitm"));

    for (name, text, expected) in [
        (
            "zero-trace-queue",
            "trace_queue_capacity = 0\n",
            "trace_queue_capacity must be greater than zero",
        ),
        (
            "zero-trace-memory",
            "trace_mem_budget = 0\n",
            "trace_mem_budget must be greater than zero",
        ),
        (
            "zero-mitm-ttl",
            "mitm_failure_ttl_seconds = 0\n",
            "mitm_failure_ttl_seconds must be greater than zero",
        ),
        (
            "zero-connect-probe",
            "connect_probe_timeout_ms = 0\n",
            "connect_probe_timeout_ms must be greater than zero",
        ),
    ] {
        let path = temp_config(name, text);
        let error =
            runtime_config(&["--config".to_string(), path.display().to_string()]).unwrap_err();
        assert!(error.contains(expected));
        let _ = fs::remove_file(path);
    }

    for (option, expected) in [
        (
            "--trace-queue-capacity",
            "--trace-queue-capacity must be greater than zero",
        ),
        (
            "--trace-mem-budget",
            "--trace-mem-budget must be greater than zero",
        ),
    ] {
        let error = runtime_config_without_default(&[option.to_string(), "0".to_string()])
            .expect_err("zero trace capacity must fail");
        assert!(error.contains(expected));
    }

    let zero_body_limit = temp_config("zero-body-limit", "body_buffer_limit = 0\n");
    let error = runtime_config(&[
        "--config".to_string(),
        zero_body_limit.display().to_string(),
    ])
    .unwrap_err();
    assert!(error.contains("body_buffer_limit must be greater than zero"));

    let malformed = temp_config("malformed", "port = \"not-a-port\"\n");
    let error =
        runtime_config(&["--config".to_string(), malformed.display().to_string()]).unwrap_err();
    assert!(error.contains("parse config"));
    assert!(error.contains("port"));

    assert!(
        runtime_config(&["--config".to_string()])
            .unwrap_err()
            .contains("--config requires a file path")
    );
    let missing = std::env::temp_dir().join(format!(
        "rsproxy-config-missing-{}-{}.toml",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    let error =
        runtime_config(&["--config".to_string(), missing.display().to_string()]).unwrap_err();
    assert!(error.contains("read config"));

    let _ = fs::remove_file(unknown);
    let _ = fs::remove_file(invalid);
    let _ = fs::remove_file(invalid_watch);
    let _ = fs::remove_file(conflicting_mitm);
    let _ = fs::remove_file(zero_body_limit);
    let _ = fs::remove_file(malformed);
}

#[test]
fn default_config_is_optional_but_loaded_when_present() {
    let present = temp_config("default", "port = 18888\n");
    let config = runtime_config_with_default_path(&[], Some(present.clone())).unwrap();
    assert_eq!(config.port, 18888);
    assert_eq!(config.config_path.as_deref(), Some(present.as_path()));

    let missing = present.with_file_name("rsproxy-config-default-missing.toml");
    let config = runtime_config_with_default_path(&[], Some(missing)).unwrap();
    assert_eq!(config.port, AppConfig::default().port);
    assert_eq!(config.config_path, None);

    let no_mitm = temp_config("no-mitm", "no_mitm = true\n");
    let config = runtime_config_with_default_path(&[], Some(no_mitm.clone())).unwrap();
    assert!(config.no_mitm);
    assert!(!config.strict_mitm);

    let _ = fs::remove_file(present);
    let _ = fs::remove_file(no_mitm);
}

#[test]
fn implicit_control_endpoint_tracks_storage_and_explicit_api_wins() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-config-api-storage-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    let config =
        runtime_config_without_default(&["--storage".to_string(), storage.display().to_string()])
            .unwrap();
    assert_eq!(config.api, crate::app::default_api_for_storage(&storage));

    let config = runtime_config_without_default(&[
        "--storage".to_string(),
        storage.display().to_string(),
        "--api".to_string(),
        "127.0.0.1:19999".to_string(),
    ])
    .unwrap();
    assert_eq!(config.api, "127.0.0.1:19999");
}
