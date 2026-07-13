use super::*;

fn state_with_storage(name: &str) -> (SharedState, PathBuf) {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-value-actions-{name}-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    std::fs::create_dir_all(storage.join("values")).unwrap();
    let mut state = test_state();
    state.config.storage = storage.clone();
    (state, storage)
}

fn write_storage(storage: &Path, relative: &str, bytes: &[u8]) {
    let path = storage.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn references_and_files_render_across_request_response_url_and_routing_actions() {
    let (state, storage) = state_with_storage("matrix");
    for (key, value) in [
        ("method", "patch"),
        ("shared", "${host}-${kind}-$2"),
        ("upstream", "proxy://127.0.0.1:18042"),
        ("redirect", "https://${host}/moved/$2"),
        ("origin", "https://${kind}.test"),
        ("age", "60"),
        ("type", "application/json"),
        ("filename", "report-$2.json"),
    ] {
        write_storage(&storage, &format!("values/{key}"), value.as_bytes());
    }
    for (path, value) in [
        ("files/header.txt", "${path}|${kind}|$2"),
        ("files/cookie-path.txt", "/api/${kind}"),
        ("files/methods.txt", "GET, PATCH"),
        ("files/from.txt", "/users/items/42"),
        ("files/to.txt", "/v2/${kind}/$2"),
        (
            "files/merge.json",
            r#"{"capture":"${kind}-$2","host":"${host}"}"#,
        ),
    ] {
        write_storage(&storage, path, value.as_bytes());
    }

    let request_meta = meta("http://example.test/users/items/42?old=1");
    let rules = rsproxy_rules::RuleSet::parse(
        "default",
        concat!(
            r#"/\/users\/(?P<kind>\w+)\/(\d+)/ "#,
            "req.method(@method) req.header(x-ref: @shared) req.cookie(sid=@shared) ",
            "res.header(x-ref: <files/header.txt>) ",
            "res.cookie(sid=@shared; Path=<files/cookie-path.txt>) ",
            "res.cors(origin=@origin, methods=<files/methods.txt>, credentials=true, ",
            "expose=@shared, max-age=@age) res.type(@type) ",
            "res.merge(<files/merge.json>) res.trailer(x-ref: @shared) ",
            "attachment(@filename) cache(public, max-age=@age) ",
            "url.rewrite(<files/from.txt>, <files/to.txt>) url.query(value=@shared) ",
            "upstream(@upstream) redirect(@redirect, 307) tag(@shared)"
        ),
    )
    .unwrap();
    let request_actions = rules.resolve(&request_meta).actions;
    let mut request = RawRequest {
        method: "GET".to_string(),
        target: request_meta.url.clone(),
        version: "HTTP/1.1".to_string(),
        headers: Vec::new(),
        body: Vec::new(),
        trailers: Vec::new(),
    };

    apply_request_actions(&mut request, &request_meta, &request_actions, &state).unwrap();
    assert_eq!(request.method, "PATCH");
    assert_eq!(
        http::header(&request.headers, "x-ref"),
        Some("example.test-items-42")
    );
    assert_eq!(
        http::header(&request.headers, "cookie"),
        Some("sid=example.test-items-42")
    );

    let effective =
        apply_url_actions(&request_meta.url, &request_meta, &request_actions, &state).unwrap();
    assert_eq!(
        effective,
        "http://example.test/v2/items/42?old=1&value=example.test-items-42"
    );
    assert_eq!(
        first_redirect(&request_actions, &request_meta, &state).unwrap(),
        Some(("https://example.test/moved/42".to_string(), 307))
    );
    let url = UrlParts::parse(&request_meta.url).unwrap();
    assert_eq!(
        upstream_route(&url, &request_actions, &request_meta, &state).unwrap(),
        UpstreamRoute::HttpProxy {
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 18042,
            target_host: "example.test".to_string(),
            target_port: 80,
        }
    );

    let mut session = Session::new(
        SessionKind::Http,
        "GET".to_string(),
        request_meta.url.clone(),
        "127.0.0.1:1".to_string(),
    );
    apply_trace_tags(&mut session, &request_actions, &request_meta, &state);
    assert_eq!(session.flags, ["tag:example.test-items-42"]);

    let response_meta = ResponseMeta {
        status: 200,
        headers: Vec::new(),
    };
    let response_actions = rules
        .resolve_response(&request_meta, &response_meta)
        .actions;
    let mut head = http::RawResponseHead {
        version: "HTTP/1.1".to_string(),
        status: 200,
        reason: "OK".to_string(),
        headers: Vec::new(),
    };
    let mut headers = Vec::new();
    let mut body = br#"{"base":true}"#.to_vec();
    apply_response_actions(
        &mut head,
        &mut headers,
        &mut body,
        &request_meta,
        &response_actions,
        &state,
    )
    .unwrap();
    assert_eq!(
        http::header(&headers, "x-ref"),
        Some("/users/items/42|items|42")
    );
    assert_eq!(
        http::header(&headers, "set-cookie"),
        Some("sid=example.test-items-42; Path=/api/items")
    );
    assert_eq!(
        http::header(&headers, "access-control-allow-origin"),
        Some("https://items.test")
    );
    assert_eq!(
        http::header(&headers, "access-control-allow-methods"),
        Some("GET, PATCH")
    );
    assert_eq!(
        http::header(&headers, "content-type"),
        Some("application/json")
    );
    assert_eq!(
        http::header(&headers, "content-disposition"),
        Some("attachment; filename=\"report-42.json\"")
    );
    assert_eq!(
        http::header(&headers, "cache-control"),
        Some("public, max-age=60")
    );
    let json: JsonValue = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["base"], true);
    assert_eq!(json["capture"], "items-42");
    assert_eq!(json["host"], "example.test");

    let mut trailers = Vec::new();
    apply_response_trailer_actions(&mut trailers, &request_meta, &response_actions, &state)
        .unwrap();
    assert_eq!(
        http::header(&trailers, "x-ref"),
        Some("example.test-items-42")
    );
    let _ = std::fs::remove_dir_all(storage);
}

#[test]
fn binary_values_are_preserved_for_body_actions_and_rejected_for_text_actions() {
    let (state, storage) = state_with_storage("binary");
    let binary = [0xff, 0x00, 0x80, b'X'];
    write_storage(&storage, "values/binary", &binary);
    let request_meta = meta("http://example.test/");
    let rules = rsproxy_rules::RuleSet::parse(
        "default",
        "example.test req.header(x-binary: @binary) req.body.set(@binary)",
    )
    .unwrap();
    let actions = rules.resolve(&request_meta).actions;
    let header = actions
        .iter()
        .find(|item| matches!(item.action, Action::ReqHeader(_)))
        .unwrap();
    let Action::ReqHeader(HeaderOp::Set { value, .. }) = &header.action else {
        panic!("expected header set action");
    };
    let error = resolve_value_text(value, header, &request_meta, &state).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);

    let body_action = actions
        .iter()
        .find(|item| matches!(item.action, Action::ReqBody(_)))
        .unwrap();
    let Action::ReqBody(op) = &body_action.action else {
        panic!("expected request body action");
    };
    let mut body = Vec::new();
    apply_body_op(&mut body, op, body_action, &request_meta, &state).unwrap();
    assert_eq!(body, binary);
    let _ = std::fs::remove_dir_all(storage);
}

#[test]
fn runtime_rejects_programmatic_reference_path_traversal() {
    let state = test_state();
    let item = resolved(Action::Tag(Value::Reference("../escape".to_string())));
    let Action::Tag(value) = &item.action else {
        unreachable!();
    };

    let error = resolve_value_text(value, &item, &meta("http://example.test/"), &state)
        .expect_err("invalid key must fail before filesystem access");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("invalid value key"));
}

#[test]
fn referenced_regex_url_replacement_preserves_regex_captures() {
    let (state, storage) = state_with_storage("regex-replacement");
    write_storage(&storage, "values/replacement", b"/new/$1");
    let request_meta = meta("http://example.test/old/42");
    let rules = rsproxy_rules::RuleSet::parse(
        "default",
        r"example.test url.rewrite(/\/old\/(\d+)/, @replacement)",
    )
    .unwrap();
    let actions = rules.resolve(&request_meta).actions;

    assert_eq!(
        apply_url_actions(&request_meta.url, &request_meta, &actions, &state).unwrap(),
        "http://example.test/new/42"
    );
    let _ = std::fs::remove_dir_all(storage);
}

#[test]
fn mock_sources_render_text_and_preserve_binary_files() {
    let (state, storage) = state_with_storage("mock");
    write_storage(&storage, "values/mock-text", b"hello ${kind} $2 at ${path}");
    let request_meta = meta("http://example.test/users/items/42");
    let rules = rsproxy_rules::RuleSet::parse(
        "default",
        r"/\/users\/(?P<kind>\w+)\/(\d+)/ mock(@mock-text)",
    )
    .unwrap();
    let actions = rules.resolve(&request_meta).actions;
    let response = first_mock(&actions, &request_meta, &state)
        .unwrap()
        .unwrap();
    assert_eq!(response.body, b"hello items 42 at /users/items/42");

    let binary = [0xff, 0x00, 0x80];
    write_storage(&storage, "files/image.bin", &binary);
    let rules =
        rsproxy_rules::RuleSet::parse("default", "example.test mock(<files/image.bin>)").unwrap();
    let actions = rules.resolve(&request_meta).actions;
    let response = first_mock(&actions, &request_meta, &state)
        .unwrap()
        .unwrap();
    assert_eq!(response.body, binary);
    assert_eq!(
        http::header(&response.headers, "content-type"),
        Some("application/octet-stream")
    );
    let _ = std::fs::remove_dir_all(storage);
}
