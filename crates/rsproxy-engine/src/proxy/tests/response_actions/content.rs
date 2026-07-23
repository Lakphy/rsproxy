use super::*;

#[test]
fn res_merge_deep_merges_json_objects() {
    let mut body = br#"{"keep":1,"nested":{"old":true,"replace":"before"}}"#.to_vec();
    let changed = apply_res_merge(
        &mut body,
        r#"{"added":2,"nested":{"replace":"after","new":3}}"#,
        usize::MAX,
    )
    .unwrap();
    assert!(changed);
    let merged: JsonValue = serde_json::from_slice(&body).unwrap();
    assert_eq!(merged["keep"], 1);
    assert_eq!(merged["added"], 2);
    assert_eq!(merged["nested"]["old"], true);
    assert_eq!(merged["nested"]["replace"], "after");
    assert_eq!(merged["nested"]["new"], 3);
}

#[test]
fn res_merge_leaves_non_json_response_unchanged() {
    let mut body = b"not json".to_vec();
    let changed = apply_res_merge(&mut body, r#"{"added":true}"#, usize::MAX).unwrap();
    assert!(!changed);
    assert_eq!(body, b"not json");
}

#[test]
fn res_merge_rejects_non_object_patch() {
    let mut body = br#"{"ok":true}"#.to_vec();
    let err = apply_res_merge(&mut body, r#"[1,2,3]"#, usize::MAX).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn body_mutations_reject_output_amplification_atomically() {
    let mut state = test_state();
    state.config.body_buffer_limit = 4;
    let request = meta("http://example.test/");

    let append = resolved(Action::ResBody(BodyOp::Append(Value::inline("xx"))));
    let Action::ResBody(op) = &append.action else {
        panic!("expected response body action");
    };
    let mut body = b"abc".to_vec();
    let error = apply_body_op(&mut body, op, &append, &request, &state).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(body, b"abc");

    let replace = RuleSet::parse("limit", "example.test res.body.replace(/a/, xx)")
        .unwrap()
        .resolve(&request)
        .actions
        .remove(0);
    let Action::ResBody(op) = &replace.action else {
        panic!("expected response body action");
    };
    let mut body = b"aaa".to_vec();
    let error = apply_body_op(&mut body, op, &replace, &request, &state).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(body, b"aaa");

    let mut body = br#"{"a":1}"#.to_vec();
    let error = apply_res_merge(&mut body, r#"{"b":2}"#, 8).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(body, br#"{"a":1}"#);
}

#[test]
fn inject_action_respects_content_type_and_mode() {
    let state = test_state();
    let meta = meta("http://example.com/page");
    let html_headers = vec![(
        "Content-Type".to_string(),
        "text/html; charset=utf-8".to_string(),
    )];
    let text_headers = vec![("Content-Type".to_string(), "text/plain".to_string())];

    let prepend = resolved(Action::Inject(InjectOp {
        target: InjectTarget::Html,
        value: Value::Inline("<head>${path}</head>".to_string()),
        mode: InjectMode::Prepend,
    }));
    let mut body = b"<body>ok</body>".to_vec();
    let Action::Inject(op) = &prepend.action else {
        panic!("expected inject action");
    };
    apply_inject_op(&html_headers, &mut body, op, &prepend, &meta, &state).unwrap();
    assert_eq!(
        String::from_utf8(body).unwrap(),
        "<head>/page</head><body>ok</body>"
    );

    let append = resolved(Action::Inject(InjectOp {
        target: InjectTarget::Html,
        value: Value::Inline("<!--tail-->".to_string()),
        mode: InjectMode::Append,
    }));
    let mut body = b"<body>ok</body>".to_vec();
    let Action::Inject(op) = &append.action else {
        panic!("expected inject action");
    };
    apply_inject_op(&text_headers, &mut body, op, &append, &meta, &state).unwrap();
    assert_eq!(String::from_utf8(body.clone()).unwrap(), "<body>ok</body>");

    let replace = resolved(Action::Inject(InjectOp {
        target: InjectTarget::Html,
        value: Value::Inline("<html>replaced</html>".to_string()),
        mode: InjectMode::Replace,
    }));
    let Action::Inject(op) = &replace.action else {
        panic!("expected inject action");
    };
    apply_inject_op(&html_headers, &mut body, op, &replace, &meta, &state).unwrap();
    assert_eq!(String::from_utf8(body).unwrap(), "<html>replaced</html>");
}

#[test]
fn trace_body_limit_excludes_media_content_types() {
    let mut config = ProxyConfig {
        trace_body_limit: 128,
        trace_exclude_media_body: true,
        ..ProxyConfig::default()
    };
    let image_headers = vec![("Content-Type".to_string(), "image/png".to_string())];
    let font_headers = vec![(
        "Content-Type".to_string(),
        "application/font-woff2".to_string(),
    )];
    let text_headers = vec![("Content-Type".to_string(), "text/plain".to_string())];

    assert_eq!(trace_body_limit_for_headers(&config, &image_headers), 0);
    assert_eq!(trace_body_limit_for_headers(&config, &font_headers), 0);
    assert_eq!(trace_body_limit_for_headers(&config, &text_headers), 128);

    config.trace_exclude_media_body = false;
    assert_eq!(trace_body_limit_for_headers(&config, &image_headers), 128);
}
