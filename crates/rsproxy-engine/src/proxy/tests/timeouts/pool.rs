use super::super::*;

#[test]
fn ttfb_timeout_classification_covers_hyper_pool_sources() {
    for message in [
        "upstream_h1 pool_hit ttfb: timeout after 40ms",
        "upstream_h1 pool_miss ttfb: timeout after 40ms",
        "upstream_h2 ttfb: timeout after 40ms",
    ] {
        assert!(is_upstream_ttfb_timeout(&io::Error::new(
            io::ErrorKind::TimedOut,
            message,
        )));
    }
    assert!(!is_upstream_ttfb_timeout(&io::Error::other(
        "upstream_h2 response: connection closed",
    )));
}

#[test]
fn upstream_pool_errors_record_protocol_and_pool_source() {
    let error = io::Error::other("upstream_h2 response: header count limit exceeded");
    let mut miss = Session::new(
        SessionKind::Http,
        "GET".to_string(),
        "https://example.test/".to_string(),
        "127.0.0.1:1".to_string(),
    );
    miss.tls.push(TlsRecord {
        phase: "upstream_tls".to_string(),
        host: "example.test".to_string(),
        handshake_ms: 1,
        peer_certificates: 1,
        protocol: Some("TLSv1_3".to_string()),
        cipher_suite: Some("TLS_AES_128_GCM_SHA256".to_string()),
        alpn: Some("h2".to_string()),
        error: None,
    });
    apply_upstream_pool_error_flags(&mut miss, &error);
    assert!(miss.flags.contains(&"h2-upstream".to_string()));
    assert!(miss.flags.contains(&"h2-upstream-pool-miss".to_string()));

    let mut hit = Session::new(
        SessionKind::Http,
        "GET".to_string(),
        "https://example.test/".to_string(),
        "127.0.0.1:1".to_string(),
    );
    apply_upstream_pool_error_flags(&mut hit, &error);
    assert!(hit.flags.contains(&"h2-upstream-pool-hit".to_string()));

    let mut h1_hit = Session::new(
        SessionKind::Http,
        "POST".to_string(),
        "http://example.test/".to_string(),
        "127.0.0.1:1".to_string(),
    );
    let error = io::Error::other("upstream_h1 pool_hit response_body: reset");
    apply_upstream_pool_error_flags(&mut h1_hit, &error);
    assert!(h1_hit.flags.contains(&"h1-upstream".to_string()));
    assert!(h1_hit.flags.contains(&"h1-upstream-pool-hit".to_string()));

    let mut h1_miss = Session::new(
        SessionKind::Http,
        "POST".to_string(),
        "http://example.test/".to_string(),
        "127.0.0.1:1".to_string(),
    );
    let error = io::Error::other("upstream_h1 pool_miss handshake: refused");
    apply_upstream_pool_error_flags(&mut h1_miss, &error);
    assert!(h1_miss.flags.contains(&"h1-upstream-pool-miss".to_string()));

    let mut h1_wait_timeout = Session::new(
        SessionKind::Http,
        "GET".to_string(),
        "http://example.test/".to_string(),
        "127.0.0.1:1".to_string(),
    );
    let error = io::Error::other("upstream_h1 pool_wait: timeout after 15000ms (active limit 256)");
    assert!(is_h1_pool_wait_timeout(&error));
    apply_upstream_pool_error_flags(&mut h1_wait_timeout, &error);
    assert!(h1_wait_timeout.flags.contains(&"h1-upstream".to_string()));
    assert!(
        h1_wait_timeout
            .flags
            .contains(&"h1-upstream-pool-wait-timeout".to_string())
    );
    assert!(
        !h1_wait_timeout
            .flags
            .iter()
            .any(|flag| flag == "h1-upstream-pool-hit" || flag == "h1-upstream-pool-miss")
    );

    let mut h2_wait_timeout = Session::new(
        SessionKind::Http,
        "GET".to_string(),
        "https://example.test/".to_string(),
        "127.0.0.1:1".to_string(),
    );
    let error =
        io::Error::other("upstream_h2 pool_wait: timeout after 15000ms (active stream limit 256)");
    assert!(is_h2_pool_wait_timeout(&error));
    apply_upstream_pool_error_flags(&mut h2_wait_timeout, &error);
    assert!(h2_wait_timeout.flags.contains(&"h2-upstream".to_string()));
    assert!(
        h2_wait_timeout
            .flags
            .contains(&"h2-upstream-pool-wait-timeout".to_string())
    );
    assert!(
        !h2_wait_timeout
            .flags
            .iter()
            .any(|flag| flag == "h2-upstream-pool-hit" || flag == "h2-upstream-pool-miss")
    );
}

#[test]
fn upstream_pool_keys_isolate_routes() {
    let request = meta("http://origin.test:18080/items");
    let url = UrlParts::parse(&request.url).unwrap();
    let state = test_state();
    let direct = UpstreamRoute::Direct {
        host: "origin.test".to_string(),
        port: 18080,
    };
    let proxy = UpstreamRoute::HttpProxy {
        proxy_host: "proxy.test".to_string(),
        proxy_port: 18888,
        target_host: "origin.test".to_string(),
        target_port: 18080,
    };

    assert_ne!(
        upstream_pool_key(&url, &direct, &[], &request, &state),
        upstream_pool_key(&url, &proxy, &[], &request, &state)
    );
}
