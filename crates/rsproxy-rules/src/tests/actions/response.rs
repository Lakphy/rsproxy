use super::super::*;

#[test]
fn header_action_accepts_spaces() {
    let rules = RuleSet::parse("default", "example.com res.header(x-name: hello world)").unwrap();
    assert!(matches!(
        rules.rules[0].actions[0],
        Action::ResHeader(HeaderOp::Set { .. })
    ));
}
#[test]
fn parses_header_cookie_cache_throttle_and_upstream_actions() {
    let rules = RuleSet::parse(
            "default",
            "example.com upstream(proxy://127.0.0.1:18888) url.rewrite(/old,/new) req.cookie(sid=1) req.ua(rsproxy) req.auth(user:pass) req.forwarded(${clientIp}) res.cookie(token=2) res.cors(*) res.type(text/plain) res.charset(utf-8) cache(off) attachment(file.txt) throttle(res, 1KB/s)",
        )
        .unwrap();
    assert!(matches!(rules.rules[0].actions[0], Action::Upstream(_)));
    assert!(matches!(
        rules.rules[0].actions[1],
        Action::UrlRewrite {
            from: UrlRewritePattern::Plain(_),
            ..
        }
    ));
    assert!(matches!(
        rules.rules[0].actions[2],
        Action::ReqCookie(CookieOp::Set { .. })
    ));
    assert!(matches!(rules.rules[0].actions[4], Action::ReqAuth(_)));
    assert!(matches!(rules.rules[0].actions[5], Action::ReqForwarded(_)));
    assert!(matches!(
        &rules.rules[0].actions[7],
        Action::ResCors(CorsOp { origin, .. }) if origin.as_inline() == Some("*")
    ));
    assert!(matches!(
        rules.rules[0].actions[10],
        Action::Cache(CacheOp::Off)
    ));
    assert!(matches!(rules.rules[0].actions[11], Action::Attachment(_)));
    assert!(matches!(
        rules.rules[0].actions[12],
        Action::Throttle {
            phase: Phase::Res,
            bytes_per_sec: 1024
        }
    ));
}
#[test]
fn parses_res_cookie_with_set_cookie_attributes() {
    let rules = RuleSet::parse(
        "default",
        "example.com res.cookie(token=$1; Path=/api; Max-Age=60; HttpOnly; Secure; SameSite=Lax)",
    )
    .unwrap();
    assert!(matches!(
        &rules.rules[0].actions[0],
        Action::ResCookie(CookieOp::Set { name, value, attrs })
            if name == "token"
                && value.as_inline() == Some("$1")
                && attrs == &vec![
                    CookieAttr { name: "Path".to_string(), value: Some(Value::inline("/api")) },
                    CookieAttr { name: "Max-Age".to_string(), value: Some(Value::inline("60")) },
                    CookieAttr { name: "HttpOnly".to_string(), value: None },
                    CookieAttr { name: "Secure".to_string(), value: None },
                    CookieAttr { name: "SameSite".to_string(), value: Some(Value::inline("Lax")) },
                ]
    ));
}

#[test]
fn parses_advanced_cache_directives() {
    let shorthand = RuleSet::parse("default", "example.com cache(60)").unwrap();
    assert!(matches!(
        &shorthand.rules[0].actions[0],
        Action::Cache(CacheOp::Directives(directives))
            if directives == &vec![CacheDirective {
                name: "max-age".to_string(),
                value: Some(Value::inline("60")),
            }]
    ));

    let rules = RuleSet::parse(
            "default",
            "example.com cache(public, max-age=${path}, s-maxage=120, stale-while-revalidate=30, immutable)",
        )
        .unwrap();
    assert!(matches!(
        &rules.rules[0].actions[0],
        Action::Cache(CacheOp::Directives(directives))
            if directives.len() == 5
                && directives[0] == CacheDirective { name: "public".to_string(), value: None }
                && directives[1] == CacheDirective { name: "max-age".to_string(), value: Some(Value::inline("${path}")) }
                && directives[2] == CacheDirective { name: "s-maxage".to_string(), value: Some(Value::inline("120")) }
                && directives[3] == CacheDirective { name: "stale-while-revalidate".to_string(), value: Some(Value::inline("30")) }
                && directives[4] == CacheDirective { name: "immutable".to_string(), value: None }
    ));
    assert_eq!(
        rules.explain(&req("http://example.com/600")),
        "default:1 cache(public, max-age=/600, s-maxage=120, stale-while-revalidate=30, immutable)\n"
    );
}

#[test]
fn parses_detailed_res_cors_options() {
    let rules = RuleSet::parse(
            "default",
            r#"example.com/api res.cors(${reqH.origin}, methods=GET POST, headers=X-Token Content-Type, credentials=true, expose=X-Trace, max-age=600)"#,
        )
        .unwrap();
    assert!(matches!(
        &rules.rules[0].actions[0],
        Action::ResCors(CorsOp {
            origin,
            methods: Some(methods),
            headers: Some(headers),
            credentials: Some(true),
            expose: Some(expose),
            max_age: Some(max_age),
        }) if origin.as_inline() == Some("${reqH.origin}")
            && methods.as_inline() == Some("GET POST")
            && headers.as_inline() == Some("X-Token Content-Type")
            && expose.as_inline() == Some("X-Trace")
            && max_age.as_inline() == Some("600")
    ));

    let mut request = req("http://example.com/api");
    request
        .headers
        .push(("Origin".to_string(), "https://app.test".to_string()));
    assert_eq!(
        rules.explain(&request),
        "default:1 res.cors(https://app.test, methods=GET POST, headers=X-Token Content-Type, credentials=true, expose=X-Trace, max-age=600)\n"
    );
}
