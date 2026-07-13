use super::*;

#[test]
fn request_header_replacements_stack_and_update_duplicate_values() {
    let request_meta = meta("http://example.test/users");
    let rules = RuleSet::parse(
        "default",
        concat!(
            r"example.test req.header(x-user ~ /user-(\d+)/id-$1)",
            "\n",
            "example.test req.header(x-user ~ /id-/account-)"
        ),
    )
    .unwrap();
    let actions = rules.resolve(&request_meta).actions;
    let mut request = RawRequest {
        method: "GET".to_string(),
        target: request_meta.url.clone(),
        version: "HTTP/1.1".to_string(),
        headers: vec![
            ("X-User".to_string(), "user-42".to_string()),
            ("x-user".to_string(), "user-7".to_string()),
            ("X-Keep".to_string(), "unchanged".to_string()),
        ],
        body: Vec::new(),
        trailers: Vec::new(),
    };

    apply_request_actions(&mut request, &request_meta, &actions, &test_state()).unwrap();

    assert_eq!(
        request.headers,
        vec![
            ("X-User".to_string(), "account-42".to_string()),
            ("x-user".to_string(), "account-7".to_string()),
            ("X-Keep".to_string(), "unchanged".to_string()),
        ]
    );
}

#[test]
fn response_header_replacements_apply_without_buffering_the_body() {
    let request_meta = meta("http://example.test/releases");
    let rules = RuleSet::parse(
        "default",
        concat!(
            r"example.test res.header(location ~ /old-(\d+)/new-$1)",
            "\n",
            "example.test res.header(location ~ /new-/stable-)"
        ),
    )
    .unwrap();
    let actions = rules.resolve(&request_meta).actions;
    let mut head = http::RawResponseHead {
        version: "HTTP/1.1".to_string(),
        status: 302,
        reason: "Found".to_string(),
        headers: Vec::new(),
    };
    let mut headers = vec![
        ("Location".to_string(), "/old-12".to_string()),
        ("LOCATION".to_string(), "/old-13".to_string()),
    ];

    apply_streaming_response_actions(
        &mut head,
        &mut headers,
        &request_meta,
        &actions,
        &test_state(),
    )
    .unwrap();

    assert_eq!(headers[0].1, "/stable-12");
    assert_eq!(headers[1].1, "/stable-13");
    assert!(!response_actions_require_body(&actions));
}

#[test]
fn nested_response_delete_requires_the_bounded_body_path() {
    let request_meta = meta("http://example.test/releases");
    let rules = RuleSet::parse("default", "example.test delete(resBody.payload.secret)").unwrap();
    let actions = rules.resolve(&request_meta).actions;

    assert!(response_actions_require_body(&actions));
}

#[test]
fn response_actions_render_response_headers_status_and_both_cookie_directions() {
    let mut request_meta = meta("http://example.test/releases");
    request_meta
        .headers
        .push(("Cookie".to_string(), "client=request-cookie".to_string()));
    let response_meta = ResponseMeta {
        status: 201,
        headers: vec![
            ("X-Origin".to_string(), "upstream".to_string()),
            (
                "Set-Cookie".to_string(),
                "sid=response-cookie; Path=/".to_string(),
            ),
        ],
    };
    let rules = RuleSet::parse(
        "default",
        "example.test res.header(x-derived: ${statusCode}|${resH.x-origin}|${reqCookies.client}|${resCookies.sid})",
    )
    .unwrap();
    let actions = rules
        .resolve_response(&request_meta, &response_meta)
        .actions;
    let mut head = http::RawResponseHead {
        version: "HTTP/1.1".to_string(),
        status: response_meta.status,
        reason: "Created".to_string(),
        headers: Vec::new(),
    };
    let mut headers = response_meta.headers;

    apply_streaming_response_actions(
        &mut head,
        &mut headers,
        &request_meta,
        &actions,
        &test_state(),
    )
    .unwrap();

    assert_eq!(
        http::header(&headers, "x-derived"),
        Some("201|upstream|request-cookie|response-cookie")
    );
}
