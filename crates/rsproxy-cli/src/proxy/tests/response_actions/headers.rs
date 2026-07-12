use super::*;

#[test]
fn detailed_cors_sets_optional_headers_and_vary_origin() {
    let mut headers = vec![("Vary".to_string(), "Accept-Encoding".to_string())];
    let meta = RequestMeta {
        method: "OPTIONS".to_string(),
        url: "http://api.test/items".to_string(),
        headers: vec![("Origin".to_string(), "https://app.test".to_string())],
        body: Vec::new(),
        client_ip: None,
        server_ip: None,
        template: Default::default(),
    };
    let item = resolved(Action::ResCors(CorsOp {
        origin: Value::inline("${reqH.origin}"),
        methods: Some(Value::inline("GET POST OPTIONS")),
        headers: Some(Value::inline("X-Token Content-Type")),
        credentials: Some(true),
        expose: Some(Value::inline("X-Trace")),
        max_age: Some(Value::inline("600")),
    }));

    if let Action::ResCors(op) = &item.action {
        apply_res_cors(&mut headers, op, &item, &meta, &test_state()).unwrap();
    }

    assert_eq!(
        http::header(&headers, "access-control-allow-origin"),
        Some("https://app.test")
    );
    assert_eq!(
        http::header(&headers, "access-control-allow-methods"),
        Some("GET POST OPTIONS")
    );
    assert_eq!(
        http::header(&headers, "access-control-allow-headers"),
        Some("X-Token Content-Type")
    );
    assert_eq!(
        http::header(&headers, "access-control-allow-credentials"),
        Some("true")
    );
    assert_eq!(
        http::header(&headers, "access-control-expose-headers"),
        Some("X-Trace")
    );
    assert_eq!(
        http::header(&headers, "access-control-max-age"),
        Some("600")
    );
    assert_eq!(
        http::header(&headers, "vary"),
        Some("Accept-Encoding, Origin")
    );
}

#[test]
fn res_cookie_keeps_default_path_for_legacy_syntax() {
    let mut headers = Vec::new();
    let item = resolved(Action::ResCookie(CookieOp::Set {
        name: "token".to_string(),
        value: Value::inline("abc"),
        attrs: Vec::new(),
    }));
    let meta = RequestMeta {
        method: "GET".to_string(),
        url: "http://example.com/".to_string(),
        headers: Vec::new(),
        body: Vec::new(),
        client_ip: None,
        server_ip: None,
        template: Default::default(),
    };

    if let Action::ResCookie(op) = &item.action {
        apply_res_cookie(&mut headers, op, &item, &meta, &test_state()).unwrap();
    }

    assert_eq!(
        http::header(&headers, "set-cookie"),
        Some("token=abc; Path=/")
    );
}

#[test]
fn res_cookie_renders_advanced_set_cookie_attributes() {
    let mut headers = Vec::new();
    let item = resolved(Action::ResCookie(CookieOp::Set {
        name: "token".to_string(),
        value: Value::inline("${path}"),
        attrs: vec![
            rsproxy_rules::CookieAttr {
                name: "Path".to_string(),
                value: Some(Value::inline("/api")),
            },
            rsproxy_rules::CookieAttr {
                name: "Max-Age".to_string(),
                value: Some(Value::inline("60")),
            },
            rsproxy_rules::CookieAttr {
                name: "HttpOnly".to_string(),
                value: None,
            },
            rsproxy_rules::CookieAttr {
                name: "Secure".to_string(),
                value: None,
            },
            rsproxy_rules::CookieAttr {
                name: "SameSite".to_string(),
                value: Some(Value::inline("Lax")),
            },
        ],
    }));
    let meta = RequestMeta {
        method: "GET".to_string(),
        url: "http://example.com/items".to_string(),
        headers: Vec::new(),
        body: Vec::new(),
        client_ip: None,
        server_ip: None,
        template: Default::default(),
    };

    if let Action::ResCookie(op) = &item.action {
        apply_res_cookie(&mut headers, op, &item, &meta, &test_state()).unwrap();
    }

    assert_eq!(
        http::header(&headers, "set-cookie"),
        Some("token=/items; Path=/api; Max-Age=60; HttpOnly; Secure; SameSite=Lax")
    );
}

#[test]
fn cache_directives_render_with_templates() {
    let item = resolved(Action::Cache(CacheOp::Directives(vec![
        rsproxy_rules::CacheDirective {
            name: "public".to_string(),
            value: None,
        },
        rsproxy_rules::CacheDirective {
            name: "max-age".to_string(),
            value: Some(Value::inline("${path}")),
        },
        rsproxy_rules::CacheDirective {
            name: "immutable".to_string(),
            value: None,
        },
    ])));
    let meta = RequestMeta {
        method: "GET".to_string(),
        url: "http://example.com/120".to_string(),
        headers: Vec::new(),
        body: Vec::new(),
        client_ip: None,
        server_ip: None,
        template: Default::default(),
    };
    let Action::Cache(CacheOp::Directives(directives)) = &item.action else {
        panic!("expected cache action");
    };

    assert_eq!(
        render_cache_directives(directives, &item, &meta, &test_state()).unwrap(),
        "public, max-age=/120, immutable"
    );
}
