use super::*;

fn template_request() -> RequestMeta {
    RequestMeta {
        method: "POST".to_string(),
        url: "https://api.example.test:8443/a/b?q=1".to_string(),
        headers: vec![
            ("X-Token".to_string(), "request-token".to_string()),
            (
                "Cookie".to_string(),
                "sid=request-sid; theme=dark".to_string(),
            ),
        ],
        body: Vec::new(),
        client_ip: Some("192.0.2.10".to_string()),
        server_ip: Some("198.51.100.20".to_string()),
        template: TemplateMetadata::fixed(
            "request-id",
            1_700_000_000_123,
            42,
            "123e4567-e89b-42d3-a456-426614174000",
        ),
    }
}

fn template_response() -> ResponseMeta {
    ResponseMeta {
        status: 207,
        headers: vec![
            ("X-Origin".to_string(), "origin-value".to_string()),
            (
                "Set-Cookie".to_string(),
                "token=response-token; Path=/; HttpOnly".to_string(),
            ),
        ],
    }
}

#[test]
fn renders_all_v1_template_variables_from_stable_request_and_response_context() {
    let rules = RuleSet::parse(
        "default",
        concat!(
            "**.example.test res.header(x-template: ",
            "${id}|${now}|${random}|${randomUUID}|${url}|${host}|${hostname}|",
            "${port}|${path}|${pathname}|${query}|${search}|${method}|${clientIp}|",
            "${serverIp}|${statusCode}|${reqH.x-token}|${resH.x-origin}|",
            "${reqCookies.sid}|${resCookies.token})"
        ),
    )
    .unwrap();
    let request = template_request();
    let response = template_response();
    let resolved = rules.resolve_response(&request, &response);
    let Action::ResHeader(HeaderOp::Set { value, .. }) = &resolved.actions[0].action else {
        panic!("expected response header action");
    };

    assert_eq!(
        resolved.actions[0].render(value.as_inline().unwrap(), &request),
        concat!(
            "request-id|1700000000123|42|123e4567-e89b-42d3-a456-426614174000|",
            "https://api.example.test:8443/a/b?q=1|api.example.test|api.example.test|",
            "8443|/a/b|/a/b|q=1|q=1|POST|192.0.2.10|198.51.100.20|207|",
            "request-token|origin-value|request-sid|response-token"
        )
    );
}

#[test]
fn template_values_are_stable_across_request_and_response_resolution() {
    let rules = RuleSet::parse(
        "default",
        "**.example.test req.header(x-request: ${id}-${randomUUID}) res.header(x-response: ${id}-${randomUUID})",
    )
    .unwrap();
    let request = template_request();
    let response = template_response();
    let request_actions = rules.resolve(&request).actions;
    let response_actions = rules.resolve_response(&request, &response).actions;

    let request_value = match &request_actions[0].action {
        Action::ReqHeader(HeaderOp::Set { value, .. }) => {
            request_actions[0].render(value.as_inline().unwrap(), &request)
        }
        _ => panic!("expected request header"),
    };
    let response_value = response_actions
        .iter()
        .find_map(|item| match &item.action {
            Action::ResHeader(HeaderOp::Set { value, .. }) => {
                Some(item.render(value.as_inline().unwrap(), &request))
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(request_value, response_value);
}

#[test]
fn template_replace_supports_regex_captures_flags_and_escaped_slashes() {
    let rules = RuleSet::parse(
        "default",
        r"**.example.test req.header(x-replaced: ${host.replace(/API\.(.*)/i, svc-$1)}:${path.replace(/\/a\//, /v1/)}:${host.replace(/(?P<sub>api)\..*/, ${sub}-named)})",
    )
    .unwrap();
    let request = template_request();

    assert_eq!(
        rules.explain(&request),
        "default:1 req.header(x-replaced: svc-example.test:/v1/b:api-named)\n"
    );
}

#[test]
fn invalid_or_unterminated_template_transforms_are_action_errors() {
    for source in [
        r"example.test req.header(x-value: ${host.replace(/[/, broken)})",
        "example.test req.header(x-value: ${host)",
    ] {
        let errors = RuleSet::parse("default", source).expect_err("template should be rejected");
        assert_eq!(errors[0].code, RuleErrorCode::Action);
    }
}

#[test]
fn regex_capture_zero_is_the_complete_match() {
    let rules =
        RuleSet::parse("default", r"/\/users\/(\d+)/ req.header(x-captures: $0|$1)").unwrap();
    let request = req("http://example.test/users/42/details");

    assert_eq!(
        rules.explain(&request),
        "default:1 req.header(x-captures: /users/42|42)\n"
    );
}

#[test]
fn generated_template_metadata_uses_hex_id_and_v4_uuid_shape() {
    let metadata = TemplateMetadata::generate();
    assert_eq!(metadata.id().len(), 32);
    assert!(
        metadata
            .id()
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    );
    assert_eq!(metadata.random_uuid().len(), 36);
    assert_eq!(metadata.random_uuid().as_bytes()[14], b'4');
    assert!(matches!(
        metadata.random_uuid().as_bytes()[19],
        b'8' | b'9' | b'a' | b'b'
    ));
}

#[test]
fn default_template_metadata_is_stable_across_clones_on_first_use() {
    let metadata = TemplateMetadata::default();
    let cloned = metadata.clone();

    assert_eq!(metadata.id(), cloned.id());
    assert_eq!(metadata.now_ms(), cloned.now_ms());
    assert_eq!(metadata.random(), cloned.random());
    assert_eq!(metadata.random_uuid(), cloned.random_uuid());
}
