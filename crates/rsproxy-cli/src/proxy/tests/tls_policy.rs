use super::*;

#[test]
fn upstream_mtls_flag_only_applies_to_origin_tls_paths() {
    let state = test_state();
    let direct = meta("https://origin.test/secure");
    let rules = rsproxy_rules::RuleSet::parse(
        "default",
        "origin.test tls(client-cert=client.pem, client-key=client.key)",
    )
    .unwrap();
    let resolved = rules.resolve(&direct);
    assert!(upstream_mtls_enabled(
        &direct.url,
        &resolved.actions,
        &direct,
        &state
    ));

    let proxied = rsproxy_rules::RuleSet::parse(
            "default",
            "origin.test upstream(https-proxy://secure-proxy.test:18443) tls(client-cert=client.pem, client-key=client.key)",
        )
        .unwrap();
    let resolved = proxied.resolve(&direct);
    assert!(upstream_mtls_enabled(
        &direct.url,
        &resolved.actions,
        &direct,
        &state
    ));

    let plain = meta("http://origin.test/plain");
    let resolved = rules.resolve(&plain);
    assert!(!upstream_mtls_enabled(
        &plain.url,
        &resolved.actions,
        &plain,
        &state
    ));
}

#[test]
fn upstream_tls_policy_flags_apply_without_mtls() {
    let state = test_state();
    let request = meta("https://origin.test/secure");
    let rules = rsproxy_rules::RuleSet::parse(
        "default",
        "origin.test tls(min=1.3, ciphers=TLS_AES_128_GCM_SHA256)",
    )
    .unwrap();
    let resolved = rules.resolve(&request);
    assert!(!upstream_mtls_enabled(
        &request.url,
        &resolved.actions,
        &request,
        &state
    ));

    let mut session = Session::new(
        SessionKind::Http,
        "GET".to_string(),
        request.url.clone(),
        "127.0.0.1:1".to_string(),
    );
    apply_upstream_tls_policy_flags(
        &mut session,
        &request.url,
        &resolved.actions,
        &request,
        &state,
    );
    assert_eq!(
        session.flags,
        vec![
            "upstream-tls-policy".to_string(),
            "upstream-tls-min:1.3".to_string(),
            "upstream-tls-ciphers:1".to_string(),
        ]
    );
}

#[test]
fn https_origin_via_http_proxy_uses_connect_tunnel() {
    let state = test_state();
    let request = meta("https://origin.test:18443/secure");
    let rules = rsproxy_rules::RuleSet::parse(
            "default",
            "origin.test upstream(proxy://127.0.0.1:18888) tls(client-cert=client.pem, client-key=client.key)",
        )
        .unwrap();
    let resolved = rules.resolve(&request);
    let url = UrlParts::parse(&request.url).unwrap();
    let route = upstream_route(&url, &resolved.actions, &request, &state).unwrap();

    assert_eq!(
        route,
        UpstreamRoute::HttpProxy {
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 18888,
            target_host: "origin.test".to_string(),
            target_port: 18443
        }
    );
    assert!(route.uses_absolute_form());
    assert!(route.uses_proxy_tunnel_for_https_origin(&url));
    assert!(!route.uses_absolute_form_for_url(&url));
    assert_eq!(url.host, "origin.test");
    assert!(upstream_mtls_enabled(
        &request.url,
        &resolved.actions,
        &request,
        &state
    ));

    let plain = UrlParts::parse("http://origin.test/path").unwrap();
    assert!(!route.uses_proxy_tunnel_for_https_origin(&plain));
    assert!(route.uses_absolute_form_for_url(&plain));
}

#[test]
fn tls_file_path_prefers_storage_relative_files() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-tls-path-test-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    std::fs::create_dir_all(storage.join("certs")).unwrap();
    std::fs::write(storage.join("certs/client.pem"), b"cert").unwrap();
    let mut state = test_state();
    state.config.storage = storage.clone();

    assert_eq!(
        resolve_tls_file_path("certs/client.pem", &state),
        storage.join("certs/client.pem")
    );
    assert_eq!(
        resolve_tls_file_path("certs/missing.pem", &state),
        PathBuf::from("certs/missing.pem")
    );
    let _ = std::fs::remove_dir_all(storage);
}

#[test]
fn upstream_route_parses_https_proxy_tunnel_target() {
    let state = test_state();
    let request = meta("tunnel://secure-origin.test:443");
    let rules = rsproxy_rules::RuleSet::parse(
        "default",
        "secure-origin.test bypass upstream(https-proxy://secure-proxy.test:18443)",
    )
    .unwrap();
    let resolved = rules.resolve(&request);
    let url = UrlParts::parse(&request.url).unwrap();
    let route = upstream_route(&url, &resolved.actions, &request, &state).unwrap();

    assert_eq!(
        route,
        UpstreamRoute::HttpsProxy {
            proxy_host: "secure-proxy.test".to_string(),
            proxy_port: 18443,
            target_host: "secure-origin.test".to_string(),
            target_port: 443
        }
    );
    assert_eq!(
        route.tunnel_session_label(),
        "https-proxy://secure-proxy.test:18443->secure-origin.test:443"
    );
}
