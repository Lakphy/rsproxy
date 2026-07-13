use super::*;

#[test]
fn request_actions_render_method_id_port_and_request_cookie() {
    let mut request_meta = meta("http://example.test:18080/items");
    request_meta.headers = vec![
        ("X-Method".to_string(), "patch".to_string()),
        ("Cookie".to_string(), "sid=request-cookie".to_string()),
    ];
    request_meta.template = rsproxy_rules::TemplateMetadata::fixed(
        "fixed-request-id",
        1_700_000_000_000,
        7,
        "123e4567-e89b-42d3-a456-426614174000",
    );
    let rules = RuleSet::parse(
        "default",
        "example.test req.method(${reqH.x-method}) req.header(x-template: ${id}|${port}|${reqCookies.sid})",
    )
    .unwrap();
    let actions = rules.resolve(&request_meta).actions;
    let mut request = RawRequest {
        method: "GET".to_string(),
        target: request_meta.url.clone(),
        version: "HTTP/1.1".to_string(),
        headers: request_meta.headers.clone(),
        body: Vec::new(),
        trailers: Vec::new(),
    };

    apply_request_actions(&mut request, &request_meta, &actions, &test_state()).unwrap();

    assert_eq!(request.method, "PATCH");
    assert_eq!(
        http::header(&request.headers, "x-template"),
        Some("fixed-request-id|18080|request-cookie")
    );
}
