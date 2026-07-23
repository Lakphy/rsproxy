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
fn mock_raw_framing_is_normalized_to_the_actual_body() {
    let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\nConnection: keep-alive\r\n\r\nunexpected";
    let parsed = parse_raw_mock_response(raw).unwrap();
    let response = finalize_mock_response(parsed, &test_state()).unwrap();

    assert_eq!(response.body, b"unexpected");
    assert!(http::header(&response.headers, "content-length").is_none());
    assert!(http::header(&response.headers, "transfer-encoding").is_none());
    assert!(http::header(&response.headers, "connection").is_none());
}

#[test]
fn mock_raw_rejects_bodyless_status_content_and_non_http1_versions() {
    let body_error = parse_raw_mock_response(b"HTTP/1.1 204 No Content\r\n\r\nbody")
        .and_then(|response| finalize_mock_response(response, &test_state()))
        .unwrap_err();
    assert_eq!(body_error.kind(), io::ErrorKind::InvalidData);

    let version_error = parse_raw_mock_response(b"HTTP/2 200 OK\r\n\r\n").unwrap_err();
    assert_eq!(version_error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn mock_raw_rejects_invalid_header_names_before_serialization() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-mock-invalid-header-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    fs::create_dir_all(&storage).unwrap();
    fs::write(
        storage.join("invalid.http"),
        b"HTTP/1.1 200 OK\r\nBad Name: value\r\n\r\nbody",
    )
    .unwrap();
    let mut state = test_state();
    state.config.storage = storage.clone();
    let request = meta("http://example.test/");
    let actions = RuleSet::parse("security", "example.test mock.raw(<invalid.http>)")
        .unwrap()
        .resolve(&request)
        .actions;

    let error = first_mock(&actions, &request, &state)
        .expect_err("invalid raw mock headers must never reach serialization");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("invalid HTTP header name"));
    let _ = fs::remove_dir_all(storage);
}

#[test]
fn mock_file_candidates_infer_content_type() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-mock-test-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    fs::create_dir_all(storage.join("mocks")).unwrap();
    fs::write(storage.join("mocks/fallback.json"), br#"{"ok":true}"#).unwrap();
    let mut state = test_state();
    state.config.storage = storage.clone();
    let request = meta("http://example.com/mock");
    let rules = RuleSet::parse(
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
    let _ = fs::remove_dir_all(storage);
}

#[test]
fn mock_content_type_preserves_windows_extended_paths() {
    assert_eq!(
        content_type_for_path(Path::new(r"\\?\C:\mocks\item.json")),
        "application/json"
    );
}

#[test]
fn mock_file_candidate_lists_are_bounded_before_filesystem_walks() {
    let candidates = (0..=rsproxy_rules::MAX_RULE_MOCK_FILE_CANDIDATES)
        .map(|index| format!("missing-{index}.txt"))
        .collect::<Vec<_>>()
        .join("|");
    let rules = RuleSet::parse("limit", &format!("example.test mock(<{candidates}>)")).unwrap();
    let request = meta("http://example.test/");
    let actions = rules.resolve(&request).actions;
    let error = first_mock(&actions, &request, &test_state()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("32-candidate limit"));
}

#[test]
fn mock_directory_candidate_joins_request_path() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-mock-dir-test-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    fs::create_dir_all(storage.join("mocks/api")).unwrap();
    fs::write(storage.join("mocks/api/item.json"), br#"{"dir":true}"#).unwrap();
    let mut state = test_state();
    state.config.storage = storage.clone();
    let request = meta("http://example.com/api/item.json");
    let rules = RuleSet::parse("default", "example.com mock(<mocks>)").unwrap();
    let resolved = rules.resolve(&request);

    let response = first_mock(&resolved.actions, &request, &state)
        .unwrap()
        .expect("directory mock should resolve request path");

    assert_eq!(response.body, br#"{"dir":true}"#);
    assert_eq!(
        http::header(&response.headers, "content-type"),
        Some("application/json")
    );
    let _ = fs::remove_dir_all(storage);
}

#[test]
fn mock_directory_path_rejects_cross_platform_traversal_segments() {
    for url in [
        "http://example.test/../secret",
        "http://example.test/..\\secret",
        "http://example.test/C:/secret",
        "http://example.test/server\\share",
    ] {
        let error = mock_directory_relative_path(&meta(url)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "url={url}");
    }
    assert_eq!(
        mock_directory_relative_path(&meta("http://example.test/api/")).unwrap(),
        PathBuf::from("api/index.html")
    );
}

#[cfg(unix)]
#[test]
fn mock_directory_rejects_symlink_targets_outside_the_root() {
    use std::os::unix::fs::symlink;

    let storage = std::env::temp_dir().join(format!(
        "rsproxy-mock-symlink-test-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    let root = storage.join("root");
    let outside = storage.join("outside");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), b"secret").unwrap();
    symlink(&outside, root.join("escape")).unwrap();

    let error = read_rule_file_path(&root, &meta("http://example.test/escape/secret.txt"), 1024)
        .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    let _ = fs::remove_dir_all(storage);
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
