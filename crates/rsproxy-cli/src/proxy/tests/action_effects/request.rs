use super::*;

#[test]
fn request_and_url_families_change_the_observed_origin_request() {
    let origin = TestOrigin::spawn(OriginReply::ok("request-ok"));
    let rules = concat!(
        "127.0.0.1 req.header(x-added: yes) req.header(-x-remove) ",
        "req.method(PUT) req.cookie(sid=new) req.cookie(-legacy) ",
        "req.ua(effect-agent) req.referer(https://ref.test/source) ",
        "req.auth(user:pass) req.forwarded(${clientIp}) ",
        "req.type(application/json) req.charset(utf-8) ",
        "url.rewrite(/old, /new) url.query(mode=effect, -drop) ",
        "req.body.set(\"item-42\") req.body.prepend(\"pre-\") ",
        "req.body.append(\"-tail\") req.body.replace(/item-(\\d+)/, id-$1)"
    );
    let state = state_with_rules("request", rules);
    let url = format!("http://{}/old?drop=secret&keep=yes", origin.address);

    let exchange = run_exchange(
        &state,
        "POST",
        &url,
        &[
            ("X-Remove", "stale"),
            ("Cookie", "legacy=old; keep=yes"),
            ("Content-Type", "text/plain; charset=latin1"),
        ],
        b"original",
    );
    let request = origin.finish();

    assert_eq!(exchange.head.status, 200);
    assert_eq!(exchange.body.body, b"request-ok");
    assert_eq!(request.method, "PUT");
    assert_eq!(request.target, "/new?keep=yes&mode=effect");
    assert_eq!(request.body, b"pre-id-42-tail");
    assert_eq!(header(&request.headers, "x-added"), Some("yes"));
    assert_eq!(header(&request.headers, "x-remove"), None);
    assert_eq!(
        header(&request.headers, "cookie"),
        Some("keep=yes; sid=new")
    );
    assert_eq!(header(&request.headers, "user-agent"), Some("effect-agent"));
    assert_eq!(
        header(&request.headers, "referer"),
        Some("https://ref.test/source")
    );
    assert_eq!(
        header(&request.headers, "authorization"),
        Some("Basic dXNlcjpwYXNz")
    );
    assert_eq!(
        header(&request.headers, "x-forwarded-for"),
        Some("127.0.0.1")
    );
    assert_eq!(
        header(&request.headers, "content-type"),
        Some("application/json; charset=utf-8")
    );
    assert_eq!(
        header(&request.headers, "content-length").and_then(|value| value.parse().ok()),
        Some(request.body.len())
    );
    cleanup_state(&state);
}

#[test]
fn delete_family_changes_request_url_response_and_trailers_over_the_network() {
    let origin = TestOrigin::spawn(OriginReply {
        status: 200,
        headers: vec![
            ("X-Remove".to_string(), "stale".to_string()),
            ("X-Keep".to_string(), "yes".to_string()),
            (
                "Content-Type".to_string(),
                "application/json; charset=utf-8".to_string(),
            ),
            ("Set-Cookie".to_string(), "legacy=old; Path=/".to_string()),
            ("Set-Cookie".to_string(), "keep=yes; Path=/".to_string()),
        ],
        body: b"response-body".to_vec(),
        trailers: vec![
            ("X-Remove".to_string(), "stale".to_string()),
            ("X-Keep-Trailer".to_string(), "yes".to_string()),
        ],
    });
    let rules = concat!(
        "127.0.0.1 delete(pathname.0, pathname.-1, urlParams.drop, ",
        "reqHeaders.x-remove, reqCookies.legacy, reqBody, reqType, ",
        "resHeaders.x-remove, resCookies.legacy, resBody, resCharset, trailer.x-remove)"
    );
    let state = state_with_rules("delete", rules);
    let url = format!(
        "http://{}/api/keep/drop?drop=secret&keep=yes",
        origin.address
    );

    let exchange = run_exchange(
        &state,
        "POST",
        &url,
        &[
            ("X-Remove", "stale"),
            ("Cookie", "legacy=old; keep=yes"),
            ("Content-Type", "application/json; charset=latin1"),
        ],
        b"request-body",
    );
    let request = origin.finish();

    assert_eq!(request.target, "/keep?keep=yes");
    assert!(request.body.is_empty());
    assert_eq!(header(&request.headers, "x-remove"), None);
    assert_eq!(header(&request.headers, "cookie"), Some("keep=yes"));
    assert_eq!(
        header(&request.headers, "content-type"),
        Some("; charset=latin1")
    );
    assert_eq!(header(&request.headers, "content-length"), Some("0"));

    assert_eq!(exchange.head.status, 200);
    assert!(exchange.body.body.is_empty());
    assert_eq!(header(&exchange.head.headers, "x-remove"), None);
    assert_eq!(header(&exchange.head.headers, "x-keep"), Some("yes"));
    assert_eq!(
        header(&exchange.head.headers, "content-type"),
        Some("application/json")
    );
    assert!(
        exchange
            .head
            .headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
            .all(|(_, value)| !value.starts_with("legacy="))
    );
    assert!(
        exchange
            .head
            .headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("set-cookie")
                && value.starts_with("keep="))
    );
    assert_eq!(header(&exchange.body.trailers, "x-remove"), None);
    assert_eq!(
        header(&exchange.body.trailers, "x-keep-trailer"),
        Some("yes")
    );
    cleanup_state(&state);
}

#[test]
fn delete_family_clears_cookie_and_trailer_collections_over_the_network() {
    let origin = TestOrigin::spawn(OriginReply {
        status: 200,
        headers: vec![
            ("Set-Cookie".to_string(), "first=1; Path=/".to_string()),
            ("Set-Cookie".to_string(), "second=2; Path=/".to_string()),
        ],
        body: b"kept".to_vec(),
        trailers: vec![
            ("X-First".to_string(), "1".to_string()),
            ("X-Second".to_string(), "2".to_string()),
        ],
    });
    let state = state_with_rules("delete-collections", "127.0.0.1 delete(cookies, trailers)");
    let url = format!("http://{}/collections", origin.address);

    let exchange = run_exchange(&state, "GET", &url, &[("Cookie", "first=1; second=2")], b"");
    let request = origin.finish();

    assert_eq!(header(&request.headers, "cookie"), None);
    assert_eq!(header(&exchange.head.headers, "set-cookie"), None);
    assert_eq!(exchange.body.body, b"kept");
    assert!(exchange.body.trailers.is_empty());
    cleanup_state(&state);
}

#[test]
fn delete_family_removes_nested_json_jsonp_and_form_fields_over_the_network() {
    let json_origin = TestOrigin::spawn(OriginReply {
        status: 200,
        headers: vec![(
            "Content-Type".to_string(),
            "application/javascript".to_string(),
        )],
        body: br#"callback({"payload":{"secret":true,"keep":2},"items":[0,1,2]});"#.to_vec(),
        trailers: Vec::new(),
    });
    let rules = r#"127.0.0.1 delete(reqBody.profile.secret, reqBody.items[0].private, reqBody.meta.a\.b, resBody.payload.secret, resBody.items[1])"#;
    let state = state_with_rules("delete-nested-json", rules);
    let url = format!("http://{}/nested", json_origin.address);
    let request_body = br#"{
        "profile":{"secret":"drop","keep":1},
        "items":[{"private":true,"keep":"yes"}],
        "meta":{"a.b":"drop","keep":"yes"}
    }"#;

    let exchange = run_exchange(
        &state,
        "POST",
        &url,
        &[("Content-Type", "application/json; charset=utf-8")],
        request_body,
    );
    let request = json_origin.finish();
    let request_json: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(request_json["profile"], serde_json::json!({"keep": 1}));
    assert_eq!(request_json["items"], serde_json::json!([{"keep": "yes"}]));
    assert_eq!(request_json["meta"], serde_json::json!({"keep": "yes"}));
    assert_eq!(
        header(&request.headers, "content-length").and_then(|value| value.parse().ok()),
        Some(request.body.len())
    );

    let response = std::str::from_utf8(&exchange.body.body).unwrap();
    assert!(response.starts_with("callback("));
    assert!(response.ends_with(");"));
    let response_json: serde_json::Value =
        serde_json::from_str(&response[9..response.len() - 2]).unwrap();
    assert_eq!(response_json["payload"], serde_json::json!({"keep": 2}));
    assert_eq!(response_json["items"], serde_json::json!([0, 2]));
    assert_eq!(
        header(&exchange.head.headers, "content-length").and_then(|value| value.parse().ok()),
        Some(exchange.body.body.len())
    );
    cleanup_state(&state);

    let form_origin = TestOrigin::spawn(OriginReply::ok("form-ok"));
    let state = state_with_rules(
        "delete-nested-form",
        "127.0.0.1 delete(reqBody.drop, reqBody.profile.secret)",
    );
    let url = format!("http://{}/form", form_origin.address);
    let exchange = run_exchange(
        &state,
        "POST",
        &url,
        &[("Content-Type", "application/x-www-form-urlencoded")],
        b"drop=1&keep=2&profile.secret=3",
    );
    let request = form_origin.finish();

    assert_eq!(request.body, b"keep=2");
    assert_eq!(header(&request.headers, "content-length"), Some("6"));
    assert_eq!(exchange.body.body, b"form-ok");
    cleanup_state(&state);
}
