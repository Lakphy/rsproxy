use super::*;

#[test]
fn response_header_and_trailer_families_change_the_client_response() {
    let origin = TestOrigin::spawn(OriginReply {
        status: 201,
        headers: vec![
            ("Content-Type".to_string(), "text/plain".to_string()),
            ("X-Remove".to_string(), "stale".to_string()),
            ("Set-Cookie".to_string(), "legacy=old; Path=/".to_string()),
            ("Pragma".to_string(), "no-cache".to_string()),
        ],
        body: b"response-ok".to_vec(),
        trailers: Vec::new(),
    });
    let rules = concat!(
        "127.0.0.1 res.header(x-added: yes) res.header(-x-remove) ",
        "res.status(299) res.cookie(-legacy) ",
        "res.cookie(sid=new; Path=/api; HttpOnly) ",
        "res.cors(https://app.test, methods=GET POST, headers=X-Test Content-Type, ",
        "credentials=true, expose=X-Trace, max-age=60) ",
        "res.type(application/json) res.charset(utf-8) ",
        "res.trailer(x-effect-end: yes) attachment(report.txt) ",
        "cache(public, max-age=60)"
    );
    let state = state_with_rules("response-headers", rules);
    let url = format!("http://{}/headers", origin.address);

    let exchange = run_exchange(&state, "GET", &url, &[], &[]);
    let _ = origin.finish();

    assert_eq!(exchange.head.status, 299);
    assert_eq!(exchange.body.body, b"response-ok");
    assert_eq!(header(&exchange.head.headers, "x-added"), Some("yes"));
    assert_eq!(header(&exchange.head.headers, "x-remove"), None);
    assert_eq!(
        header(&exchange.head.headers, "set-cookie"),
        Some("sid=new; Path=/api; HttpOnly")
    );
    assert_eq!(
        header(&exchange.head.headers, "access-control-allow-origin"),
        Some("https://app.test")
    );
    assert_eq!(
        header(&exchange.head.headers, "access-control-allow-credentials"),
        Some("true")
    );
    assert_eq!(header(&exchange.head.headers, "vary"), Some("Origin"));
    assert_eq!(
        header(&exchange.head.headers, "content-type"),
        Some("application/json; charset=utf-8")
    );
    assert_eq!(
        header(&exchange.head.headers, "content-disposition"),
        Some("attachment; filename=\"report.txt\"")
    );
    assert_eq!(
        header(&exchange.head.headers, "cache-control"),
        Some("public, max-age=60")
    );
    assert_eq!(header(&exchange.head.headers, "pragma"), None);
    assert_eq!(header(&exchange.body.trailers, "x-effect-end"), Some("yes"));
    cleanup_state(&state);
}

#[test]
fn response_body_families_stack_in_rule_order() {
    let origin = TestOrigin::spawn(OriginReply::ok("origin"));
    let rules = concat!(
        "127.0.0.1 res.body.set(\"item-42\") ",
        "res.body.prepend(\"pre-\") res.body.append(\"-tail\") ",
        "res.body.replace(/item-(\\d+)/, id-$1)"
    );
    let state = state_with_rules("response-body", rules);
    let url = format!("http://{}/body", origin.address);

    let exchange = run_exchange(&state, "GET", &url, &[], &[]);
    let _ = origin.finish();

    assert_eq!(exchange.head.status, 200);
    assert_eq!(exchange.body.body, b"pre-id-42-tail");
    cleanup_state(&state);
}

#[test]
fn response_merge_and_inject_have_network_visible_effects() {
    let json_origin = TestOrigin::spawn(OriginReply {
        status: 200,
        headers: vec![("Content-Type".to_string(), "application/json".to_string())],
        body: br#"{"keep":1,"nested":{"old":true}}"#.to_vec(),
        trailers: Vec::new(),
    });
    let state = state_with_rules(
        "response-merge",
        r#"127.0.0.1 res.merge({"added":2,"nested":{"new":3}})"#,
    );
    let url = format!("http://{}/json", json_origin.address);
    let exchange = run_exchange(&state, "GET", &url, &[], &[]);
    let _ = json_origin.finish();
    let body: serde_json::Value = serde_json::from_slice(&exchange.body.body).unwrap();
    assert_eq!(body["keep"], 1);
    assert_eq!(body["added"], 2);
    assert_eq!(body["nested"]["old"], true);
    assert_eq!(body["nested"]["new"], 3);
    cleanup_state(&state);

    let html_origin = TestOrigin::spawn(OriginReply {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/html".to_string())],
        body: b"<main>ok</main>".to_vec(),
        trailers: Vec::new(),
    });
    let state = state_with_rules(
        "response-inject",
        r#"127.0.0.1 inject(html, "<!--effect-->", append)"#,
    );
    let url = format!("http://{}/html", html_origin.address);
    let exchange = run_exchange(&state, "GET", &url, &[], &[]);
    let _ = html_origin.finish();
    assert_eq!(exchange.body.body, b"<main>ok</main><!--effect-->");
    cleanup_state(&state);
}
