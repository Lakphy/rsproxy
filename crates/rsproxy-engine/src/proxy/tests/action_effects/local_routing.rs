use super::*;

#[test]
fn local_response_families_short_circuit_without_an_origin() {
    let cases = [
        (
            "mock",
            "effect.test mock(\"mock-body\")",
            200,
            "mock-body",
            None,
        ),
        (
            "mock-raw",
            "effect.test mock.raw(\"HTTP/1.1 207 Multi-Status\\r\\nX-Raw: yes\\r\\n\\r\\nraw-body\")",
            207,
            "raw-body",
            Some(("x-raw", "yes")),
        ),
        ("status", "effect.test status(410)", 410, "", None),
        (
            "redirect",
            "effect.test redirect(https://new.test/path, 307)",
            307,
            "",
            Some(("location", "https://new.test/path")),
        ),
    ];

    for (name, rules, status, body, expected_header) in cases {
        let state = state_with_rules(name, rules);
        let exchange = run_exchange(&state, "GET", "http://effect.test/local", &[], &[]);
        assert_eq!(exchange.head.status, status, "{name} status");
        if !body.is_empty() {
            assert_eq!(exchange.body.body, body.as_bytes(), "{name} body");
        }
        if let Some((header_name, value)) = expected_header {
            assert_eq!(
                header(&exchange.head.headers, header_name),
                Some(value),
                "{name} header"
            );
        }
        cleanup_state(&state);
    }
}

#[test]
fn host_family_routes_a_named_origin_without_rewriting_authority() {
    let origin = TestOrigin::spawn(OriginReply::ok("host-ok"));
    let rules = format!("host-effect.test host({})", origin.address);
    let state = state_with_rules("host", &rules);

    let exchange = run_exchange(&state, "GET", "http://host-effect.test/routed", &[], &[]);
    let request = origin.finish();

    assert_eq!(exchange.head.status, 200);
    assert_eq!(exchange.body.body, b"host-ok");
    assert_eq!(request.target, "/routed");
    assert_eq!(header(&request.headers, "host"), Some("host-effect.test"));
    cleanup_state(&state);
}

#[test]
fn upstream_family_uses_absolute_form_at_the_selected_proxy() {
    let upstream = TestOrigin::spawn(OriginReply::ok("upstream-ok"));
    let rules = format!(
        "upstream-target.test upstream(proxy://{})",
        upstream.address
    );
    let state = state_with_rules("upstream", &rules);

    let exchange = run_exchange(
        &state,
        "GET",
        "http://upstream-target.test/via-proxy?x=1",
        &[],
        &[],
    );
    let request = upstream.finish();

    assert_eq!(exchange.head.status, 200);
    assert_eq!(exchange.body.body, b"upstream-ok");
    assert_eq!(request.target, "http://upstream-target.test/via-proxy?x=1");
    assert_eq!(
        header(&request.headers, "host"),
        Some("upstream-target.test")
    );
    cleanup_state(&state);
}

#[test]
fn direct_family_overrides_a_selected_upstream_proxy() {
    let unavailable = TcpListener::bind("127.0.0.1:0").unwrap();
    let unavailable_address = unavailable.local_addr().unwrap();
    drop(unavailable);
    let origin = TestOrigin::spawn(OriginReply::ok("direct-ok"));
    let rules = format!("127.0.0.1 upstream(proxy://{unavailable_address}) direct");
    let state = state_with_rules("direct", &rules);
    let url = format!("http://{}/direct", origin.address);

    let exchange = run_exchange(&state, "GET", &url, &[], &[]);
    let request = origin.finish();

    assert_eq!(exchange.head.status, 200);
    assert_eq!(exchange.body.body, b"direct-ok");
    assert_eq!(request.target, "/direct");
    cleanup_state(&state);
}

#[test]
fn map_remote_family_serves_the_target_backend_transparently() {
    // No path on the target: original path and query are preserved.
    let origin = TestOrigin::spawn(OriginReply::ok("map-ok"));
    let rules = format!("map-effect.test map.remote(http://{})", origin.address);
    let state = state_with_rules("map.remote", &rules);

    let origin_address = origin.address.to_string();
    let exchange = run_exchange(&state, "GET", "http://map-effect.test/app?q=1", &[], &[]);
    let request = origin.finish();

    assert_eq!(exchange.head.status, 200);
    assert_eq!(exchange.body.body, b"map-ok");
    assert_eq!(request.target, "/app?q=1");
    // The upstream Host header follows the mapped target, unlike host().
    assert_eq!(
        header(&request.headers, "host"),
        Some(origin_address.as_str())
    );
    cleanup_state(&state);
}

#[test]
fn map_remote_target_path_replaces_the_original_with_captures() {
    let origin = TestOrigin::spawn(OriginReply::ok("map-path-ok"));
    let rules = format!(
        "/^http:\\/\\/map-path.test\\/(.*)$/ map.remote(http://{}/base/$1)",
        origin.address
    );
    let state = state_with_rules("map.remote-path", &rules);

    let exchange = run_exchange(&state, "GET", "http://map-path.test/js/index.js", &[], &[]);
    let request = origin.finish();

    assert_eq!(exchange.head.status, 200);
    assert_eq!(exchange.body.body, b"map-path-ok");
    assert_eq!(request.target, "/base/js/index.js");
    cleanup_state(&state);
}

#[test]
fn mock_inline_combination_short_circuits_with_status_headers_and_body() {
    let state = state_with_rules(
        "mock-inline",
        "inline.test mock(status=503, type=application/json, header=X-Mock: yes, body={\"ok\":false})",
    );
    let exchange = run_exchange(&state, "GET", "http://inline.test/api", &[], &[]);
    assert_eq!(exchange.head.status, 503);
    assert_eq!(exchange.body.body, br#"{"ok":false}"#);
    assert_eq!(header(&exchange.head.headers, "x-mock"), Some("yes"));
    assert_eq!(
        header(&exchange.head.headers, "content-type"),
        Some("application/json")
    );
    cleanup_state(&state);
}
