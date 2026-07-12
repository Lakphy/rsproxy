use super::*;

#[test]
fn mock_raw_parses_status_headers_and_body() {
    let raw = b"HTTP/1.1 207 Multi-Status\r\nContent-Type: application/json\r\nX-Raw: yes\r\nConnection: keep-alive\r\n\r\n{\"raw\":true}";
    let response = parse_raw_mock_response(raw).unwrap();
    assert_eq!(response.status, 207);
    assert_eq!(response.reason, "Multi-Status");
    assert_eq!(
        response.headers,
        vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("X-Raw".to_string(), "yes".to_string())
        ]
    );
    assert_eq!(response.body, br#"{"raw":true}"#);
}

#[test]
fn mock_file_candidates_infer_content_type() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-mock-test-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    std::fs::create_dir_all(storage.join("mocks")).unwrap();
    std::fs::write(storage.join("mocks/fallback.json"), br#"{"ok":true}"#).unwrap();
    let mut state = test_state();
    state.config.storage = storage.clone();
    let request = meta("http://example.com/mock");
    let rules = rsproxy_rules::RuleSet::parse(
        "default",
        "example.com mock(<mocks/missing.json|mocks/fallback.json>)",
    )
    .unwrap();
    let resolved = rules.resolve(&request);

    let response = first_mock(&resolved.actions, &request, &state)
        .unwrap()
        .expect("mock should resolve fallback file");

    assert_eq!(response.status, 200);
    assert_eq!(response.body, br#"{"ok":true}"#);
    assert_eq!(
        http::header(&response.headers, "content-type"),
        Some("application/json")
    );
    let _ = std::fs::remove_dir_all(storage);
}

#[test]
fn mock_directory_candidate_joins_request_path() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-mock-dir-test-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    std::fs::create_dir_all(storage.join("mocks/api")).unwrap();
    std::fs::write(storage.join("mocks/api/item.json"), br#"{"dir":true}"#).unwrap();
    let mut state = test_state();
    state.config.storage = storage.clone();
    let request = meta("http://example.com/api/item.json");
    let rules = rsproxy_rules::RuleSet::parse("default", "example.com mock(<mocks>)").unwrap();
    let resolved = rules.resolve(&request);

    let response = first_mock(&resolved.actions, &request, &state)
        .unwrap()
        .expect("directory mock should resolve request path");

    assert_eq!(response.body, br#"{"dir":true}"#);
    assert_eq!(
        http::header(&response.headers, "content-type"),
        Some("application/json")
    );
    let _ = std::fs::remove_dir_all(storage);
}

#[test]
fn req_forwarded_action_sets_x_forwarded_for() {
    let mut request = RawRequest {
        method: "GET".to_string(),
        target: "http://example.com/".to_string(),
        version: "HTTP/1.1".to_string(),
        headers: vec![("X-Forwarded-For".to_string(), "192.0.2.10".to_string())],
        body: Vec::new(),
        trailers: Vec::new(),
    };
    let mut meta = meta("http://example.com/");
    meta.client_ip = Some("203.0.113.9:61234".to_string());
    let actions = vec![resolved(Action::ReqForwarded(Value::inline("${clientIp}")))];

    apply_request_actions(&mut request, &meta, &actions, &test_state()).unwrap();

    assert_eq!(
        http::header(&request.headers, "x-forwarded-for"),
        Some("203.0.113.9")
    );
    assert_eq!(forwarded_for_value("[2001:db8::1]:443"), "2001:db8::1");
    assert_eq!(
        forwarded_for_value("10.0.0.1, 10.0.0.2"),
        "10.0.0.1, 10.0.0.2"
    );
}

#[test]
fn trace_tags_render_to_flags_and_hide_marks_session_invisible() {
    let meta = meta("http://example.com/tagged");
    let mut session = Session::new(
        SessionKind::Http,
        "GET".to_string(),
        meta.url.clone(),
        "127.0.0.1:1".to_string(),
    );
    let actions = vec![
        resolved(Action::Tag(Value::inline("${path}"))),
        resolved(Action::Tag(Value::inline("${path}"))),
        resolved(Action::Tag(Value::inline("manual"))),
    ];

    apply_trace_tags(&mut session, &actions, &meta, &test_state());

    assert_eq!(
        session.flags,
        vec!["tag:/tagged".to_string(), "tag:manual".to_string()]
    );
    assert!(!trace_hidden(&actions));
    assert!(trace_hidden(&[resolved(Action::Hide)]));
}
