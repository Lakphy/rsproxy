use super::*;
use crate::cli::command::ClientArgs;

#[test]
fn trace_resource_options_parse_sizes() {
    assert_eq!(parse_size("512b").unwrap(), 512);
    assert_eq!(parse_size("2KB").unwrap(), 2048);
    assert_eq!(parse_size("3mb").unwrap(), 3 * 1024 * 1024);
    assert_eq!(parse_size("1gb").unwrap(), 1024 * 1024 * 1024);

    let config = runtime_config_without_default(&[
        "--trace-body-limit".to_string(),
        "8kb".to_string(),
        "--trace-queue-capacity".to_string(),
        "17".to_string(),
        "--trace-mem-budget".to_string(),
        "24mb".to_string(),
        "--body-buffer-limit".to_string(),
        "4mb".to_string(),
        "--trace-segment-size".to_string(),
        "16kb".to_string(),
        "--trace-disk-budget".to_string(),
        "32kb".to_string(),
        "--trace-spill-compression".to_string(),
        "zstd:3".to_string(),
        "--mitm-cert-cache-capacity".to_string(),
        "7".to_string(),
        "--strict-mitm".to_string(),
        "--mitm-failure-cache-capacity".to_string(),
        "9".to_string(),
        "--mitm-failure-ttl-seconds".to_string(),
        "45".to_string(),
        "--connect-probe-timeout-ms".to_string(),
        "225".to_string(),
        "--max-header-count".to_string(),
        "19".to_string(),
        "--h1-pool-max-active-per-key".to_string(),
        "3".to_string(),
        "--h1-pool-wait-timeout-ms".to_string(),
        "250".to_string(),
        "--h2-pool-max-active-streams-per-key".to_string(),
        "5".to_string(),
        "--h2-pool-wait-timeout-ms".to_string(),
        "300".to_string(),
        "--tcp-connect-timeout-ms".to_string(),
        "325".to_string(),
        "--dns-timeout-ms".to_string(),
        "125".to_string(),
        "--dns-cache".to_string(),
        "17".to_string(),
        "--dns-server".to_string(),
        "127.0.0.1:5353,1.1.1.1".to_string(),
        "--client-tls-handshake-timeout-ms".to_string(),
        "340".to_string(),
        "--upstream-tls-handshake-timeout-ms".to_string(),
        "350".to_string(),
        "--upstream-ttfb-timeout-ms".to_string(),
        "375".to_string(),
        "--request-timeout-ms".to_string(),
        "425".to_string(),
        "--api-token".to_string(),
        "0123456789abcdef0123456789abcdef".to_string(),
    ])
    .unwrap();
    assert_eq!(config.trace_body_limit, 8 * 1024);
    assert_eq!(config.trace_queue_capacity, 17);
    assert_eq!(config.trace_memory_budget, 24 * 1024 * 1024);
    assert_eq!(config.body_buffer_limit, 4 * 1024 * 1024);
    assert_eq!(config.trace_spill_segment_size, 16 * 1024);
    assert_eq!(config.trace_disk_budget, 32 * 1024);
    assert_eq!(
        config.trace_spill_compression,
        rsproxy_trace::TraceSpillCompression::Zstd { level: 3 }
    );
    assert_eq!(config.mitm_cert_cache_capacity, 7);
    assert!(!config.no_mitm);
    assert!(config.strict_mitm);
    assert_eq!(config.mitm_failure_cache_capacity, 9);
    assert_eq!(config.mitm_failure_ttl, Duration::from_secs(45));
    assert_eq!(config.connect_probe_timeout, Duration::from_millis(225));
    assert_eq!(config.max_header_count, 19);
    assert_eq!(config.h1_pool_max_active_per_key, 3);
    assert_eq!(config.h1_pool_wait_timeout, Duration::from_millis(250));
    assert_eq!(config.h2_pool_max_active_streams_per_key, 5);
    assert_eq!(config.h2_pool_wait_timeout, Duration::from_millis(300));
    assert_eq!(config.dns_timeout, Duration::from_millis(125));
    assert_eq!(config.dns_cache_ttl, Duration::from_secs(17));
    assert_eq!(
        config.dns_servers,
        vec![
            "127.0.0.1:5353".parse().unwrap(),
            "1.1.1.1:53".parse().unwrap()
        ]
    );
    assert_eq!(config.tcp_connect_timeout, Duration::from_millis(325));
    assert_eq!(
        config.client_tls_handshake_timeout,
        Duration::from_millis(340)
    );
    assert_eq!(
        config.upstream_tls_handshake_timeout,
        Duration::from_millis(350)
    );
    assert_eq!(config.upstream_ttfb_timeout, Duration::from_millis(375));
    assert_eq!(config.request_total_timeout, Duration::from_millis(425));
    assert_eq!(
        config.api_token.as_deref(),
        Some("0123456789abcdef0123456789abcdef")
    );
    assert!(
        runtime_config_without_default(&["--max-header-count".to_string(), "0".to_string()])
            .is_err()
    );
    assert!(
        runtime_config_without_default(&[
            "--h1-pool-max-active-per-key".to_string(),
            "0".to_string()
        ])
        .is_err()
    );
    assert!(
        runtime_config_without_default(&["--h1-pool-wait-timeout-ms".to_string(), "0".to_string()])
            .is_err()
    );
    assert!(
        runtime_config_without_default(&[
            "--h2-pool-max-active-streams-per-key".to_string(),
            "0".to_string()
        ])
        .is_err()
    );
    assert!(
        runtime_config_without_default(&["--h2-pool-wait-timeout-ms".to_string(), "0".to_string()])
            .is_err()
    );
    assert!(
        runtime_config_without_default(&["--tcp-connect-timeout-ms".to_string(), "0".to_string()])
            .is_err()
    );
    assert!(
        runtime_config_without_default(&["--dns-timeout-ms".to_string(), "0".to_string()]).is_err()
    );
    assert!(
        runtime_config_without_default(&[
            "--dns-server".to_string(),
            "resolver.example:53".to_string()
        ])
        .is_err()
    );
    assert!(
        runtime_config_without_default(&[
            "--client-tls-handshake-timeout-ms".to_string(),
            "0".to_string()
        ])
        .is_err()
    );
    assert!(
        runtime_config_without_default(&[
            "--upstream-tls-handshake-timeout-ms".to_string(),
            "0".to_string()
        ])
        .is_err()
    );
    assert!(
        runtime_config_without_default(&[
            "--upstream-ttfb-timeout-ms".to_string(),
            "0".to_string()
        ])
        .is_err()
    );
    assert!(
        runtime_config_without_default(&["--request-timeout-ms".to_string(), "0".to_string()])
            .is_err()
    );
    assert!(
        runtime_config_without_default(&["--no-mitm".to_string(), "--strict-mitm".to_string()])
            .is_err()
    );
    assert!(
        runtime_config_without_default(&[
            "--mitm-failure-ttl-seconds".to_string(),
            "0".to_string()
        ])
        .is_err()
    );
    assert!(
        runtime_config_without_default(&[
            "--connect-probe-timeout-ms".to_string(),
            "0".to_string()
        ])
        .is_err()
    );
    assert!(
        runtime_config_without_default(&["--api-token".to_string(), "short".to_string()]).is_err()
    );
}
#[test]
fn trace_list_limit_accepts_short_and_long_options() {
    assert_eq!(parse_trace_list_limit(&[]).unwrap(), 20);
    assert_eq!(
        parse_trace_list_limit(&["-n".to_string(), "3".to_string()]).unwrap(),
        3
    );
    assert_eq!(
        parse_trace_list_limit(&["--limit".to_string(), "5".to_string()]).unwrap(),
        5
    );
}

#[test]
fn dns_server_append_preserves_order_and_duplicates() {
    let args = parse_runtime_args(&[
        "--dns-server".to_string(),
        "1.1.1.1".to_string(),
        "--dns-server".to_string(),
        "9.9.9.9:5353".to_string(),
        "--dns-server".to_string(),
        "1.1.1.1".to_string(),
    ])
    .unwrap();
    assert_eq!(args.dns_server, ["1.1.1.1", "9.9.9.9:5353", "1.1.1.1"]);
}

#[test]
fn proxy_auth_requires_nonempty_user_and_password() {
    let config = runtime_config_without_default(&[
        "--proxy-auth".to_string(),
        "alice:secret:with-colon".to_string(),
    ])
    .unwrap();
    assert_eq!(
        config.proxy_auth.as_deref(),
        Some("alice:secret:with-colon")
    );

    for invalid in ["alice", ":secret", "alice:"] {
        let err =
            runtime_config_without_default(&["--proxy-auth".to_string(), invalid.to_string()])
                .expect_err("invalid proxy auth should fail");
        assert!(err.to_string().contains("--proxy-auth"));
    }
}

#[test]
fn trace_filter_headers_only_disables_body_capture() {
    let config = runtime_config_without_default(&[
        "--trace-body-limit".to_string(),
        "8kb".to_string(),
        "--trace-filter".to_string(),
        "headers-only".to_string(),
    ])
    .unwrap();
    assert_eq!(config.trace_body_limit, 0);
    assert!(config.trace_exclude_media_body);

    let config = runtime_config_without_default(&[
        "--trace-body-limit".to_string(),
        "8kb".to_string(),
        "--trace-filter".to_string(),
        "full".to_string(),
    ])
    .unwrap();
    assert_eq!(config.trace_body_limit, 8 * 1024);
    assert!(!config.trace_exclude_media_body);

    let config = runtime_config_without_default(&[
        "--trace-filter".to_string(),
        "media".to_string(),
        "--trace-body-limit".to_string(),
        "8kb".to_string(),
    ])
    .unwrap();
    assert_eq!(config.trace_body_limit, 8 * 1024);
    assert!(config.trace_exclude_media_body);

    let err = runtime_config_without_default(&["--trace-filter".to_string(), "images".to_string()])
        .expect_err("unsupported trace filter should fail");
    assert!(
        err.to_string()
            .contains("--trace-filter supports headers-only, media, or full")
    );
}

#[test]
fn debug_output_redacts_api_tokens() {
    let secret = "0123456789abcdef0123456789abcdef";
    let client = ClientArgs {
        api_token: Some(secret.to_string()),
        ..ClientArgs::default()
    };
    let client_debug = format!("{client:?}");
    assert!(!client_debug.contains(secret));
    assert!(client_debug.contains("[REDACTED]"));

    let mut config = AppConfig::default();
    config.api_token = Some(secret.to_string());
    config.proxy_auth = Some(format!("user:{secret}"));
    let config_debug = format!("{config:?}");
    assert!(!config_debug.contains(secret));
    assert!(config_debug.contains("[REDACTED]"));
}
