use super::*;

fn resolved(action: Action) -> ResolvedAction {
    ResolvedAction {
        action,
        rule: MatchedRule {
            group: "explain".to_string(),
            line: 7,
            raw: "fixture".to_string(),
        },
        captures: Captures::default(),
        response: None,
    }
}

fn explain(action: Action) -> String {
    explain_action(&resolved(action), &req("https://example.test/items"))
}

#[test]
fn explain_covers_every_value_source_and_structured_action_shape() {
    let host = HostPool::new(vec![
        Value::inline("127.0.0.1:80"),
        Value::File("hosts.txt".to_string()),
        Value::Reference("edge".to_string()),
    ])
    .unwrap();
    assert_eq!(
        explain(Action::Host(host)),
        "host(127.0.0.1:80, <hosts.txt>, @edge)"
    );
    assert_eq!(
        explain(Action::Upstream(Value::File("route.txt".to_string()))),
        "upstream(<route.txt>)"
    );

    let values = [
        (Value::inline("inline"), "inline"),
        (Value::File("body.txt".to_string()), "<body.txt>"),
        (Value::Reference("body".to_string()), "@body"),
    ];
    for (value, expected) in values.clone() {
        assert_eq!(explain(Action::Mock(value)), format!("mock({expected})"));
    }
    for (value, expected) in values.clone() {
        assert_eq!(
            explain(Action::MockRaw(value)),
            format!("mock.raw({expected})")
        );
    }

    assert_eq!(explain(Action::Status(204)), "status(204)");
    assert_eq!(
        explain(Action::Redirect {
            url: Value::Reference("location".to_string()),
            code: 308,
        }),
        "redirect(@location, 308)"
    );
    assert_eq!(
        explain(Action::ReqHeader(HeaderOp::Set {
            name: "x-id".to_string(),
            value: Value::inline("42"),
        })),
        "req.header(x-id: 42)"
    );
    assert_eq!(
        explain(Action::ResHeader(HeaderOp::Remove {
            name: "server".to_string(),
        })),
        "res.header(-server)"
    );
    assert_eq!(
        explain(Action::ResTrailer(HeaderOp::Replace {
            name: "x-path".to_string(),
            pattern: RegexReplacePattern::new("old/path".to_string(), false).unwrap(),
            replacement: "new/path".to_string(),
        })),
        r"res.trailer(x-path ~ /old\/path/new\/path)"
    );

    for (action, expected) in [
        (
            Action::ReqMethod(Value::inline("PATCH")),
            "req.method(PATCH)",
        ),
        (Action::ReqUa(Value::inline("agent")), "req.ua(agent)"),
        (
            Action::ReqReferer(Value::inline("https://ref.test")),
            "req.referer(https://ref.test)",
        ),
        (
            Action::ReqAuth(Value::inline("user:pass")),
            "req.auth(user:pass)",
        ),
        (
            Action::ReqForwarded(Value::inline("192.0.2.1")),
            "req.forwarded(192.0.2.1)",
        ),
        (
            Action::ReqType(Value::inline("application/json")),
            "req.type(application/json)",
        ),
        (
            Action::ReqCharset(Value::inline("utf-8")),
            "req.charset(utf-8)",
        ),
        (
            Action::ResType(Value::inline("text/plain")),
            "res.type(text/plain)",
        ),
        (
            Action::ResCharset(Value::inline("utf-8")),
            "res.charset(utf-8)",
        ),
        (Action::ResMerge(Value::inline("{}")), "res.merge({})"),
        (
            Action::Attachment(Some(Value::inline("report.txt"))),
            "attachment(report.txt)",
        ),
        (Action::Tag(Value::inline("audit")), "tag(audit)"),
    ] {
        assert_eq!(explain(action), expected);
    }

    assert_eq!(
        explain(Action::ReqCookie(CookieOp::Set {
            name: "sid".to_string(),
            value: Value::Reference("session".to_string()),
            attrs: vec![
                CookieAttr {
                    name: "Path".to_string(),
                    value: Some(Value::inline("/")),
                },
                CookieAttr {
                    name: "Secure".to_string(),
                    value: None,
                },
            ],
        })),
        "req.cookie(sid=@session; Path=/; Secure)"
    );
    assert_eq!(
        explain(Action::ResCookie(CookieOp::Remove {
            name: "legacy".to_string(),
        })),
        "res.cookie(-legacy)"
    );
    assert_eq!(
        explain(Action::ResCors(CorsOp {
            origin: Value::inline("https://app.test"),
            methods: Some(Value::inline("GET POST")),
            headers: Some(Value::inline("X-Token")),
            credentials: Some(false),
            expose: Some(Value::inline("X-Trace")),
            max_age: Some(Value::inline("60")),
        })),
        "res.cors(https://app.test, methods=GET POST, headers=X-Token, credentials=false, expose=X-Trace, max-age=60)"
    );
    assert_eq!(explain(Action::Cache(CacheOp::Off)), "cache(off)");
    assert_eq!(
        explain(Action::Cache(CacheOp::Directives(vec![
            CacheDirective {
                name: "public".to_string(),
                value: None,
            },
            CacheDirective {
                name: "max-age".to_string(),
                value: Some(Value::inline("60")),
            },
        ]))),
        "cache(public, max-age=60)"
    );
}

#[test]
fn explain_covers_tls_url_body_delete_and_control_variants() {
    let all_ciphers = vec![
        TlsCipherSuite::Tls13Aes128GcmSha256,
        TlsCipherSuite::Tls13Aes256GcmSha384,
        TlsCipherSuite::Tls13Chacha20Poly1305Sha256,
        TlsCipherSuite::Tls12EcdheEcdsaAes128GcmSha256,
        TlsCipherSuite::Tls12EcdheEcdsaAes256GcmSha384,
        TlsCipherSuite::Tls12EcdheEcdsaChacha20Poly1305Sha256,
        TlsCipherSuite::Tls12EcdheRsaAes128GcmSha256,
        TlsCipherSuite::Tls12EcdheRsaAes256GcmSha384,
        TlsCipherSuite::Tls12EcdheRsaChacha20Poly1305Sha256,
    ];
    let tls = explain(Action::Tls(TlsOp {
        client_cert: Some("client.pem".to_string()),
        client_key: Some("client.key".to_string()),
        min_version: Some(TlsMinVersion::Tls13),
        ciphers: all_ciphers,
    }));
    assert!(tls.starts_with("tls(client-cert=client.pem, client-key=client.key, min=1.3"));
    assert!(tls.contains("TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256"));

    assert_eq!(
        explain(Action::UrlRewrite {
            from: UrlRewritePattern::Plain(Value::inline("/old")),
            to: Value::File("new-path.txt".to_string()),
        }),
        "url.rewrite(/old, <new-path.txt>)"
    );
    assert_eq!(
        explain(Action::UrlRewrite {
            from: UrlRewritePattern::Regex(
                RegexReplacePattern::new("old".to_string(), true).unwrap(),
            ),
            to: Value::Reference("replacement".to_string()),
        }),
        "url.rewrite(/old/i, @replacement)"
    );
    assert_eq!(
        explain(Action::UrlQuery(vec![
            QueryOp::Set {
                name: "page".to_string(),
                value: Value::inline("2"),
            },
            QueryOp::Remove {
                name: "token".to_string(),
            },
        ])),
        "url.query(page=2, -token)"
    );

    let escaped_path = DeleteBodyPath::new(vec![
        DeleteBodyPathSegment::Key("a\\.,()[]{}<>#|& '\"\n\r\t\u{000c}\u{000b}".to_string()),
        DeleteBodyPathSegment::Index(3),
    ])
    .unwrap();
    let delete = explain(Action::Delete(vec![
        DeleteOp::Pathname,
        DeleteOp::PathSegment(DeletePathSegment::Index(-1)),
        DeleteOp::PathSegment(DeletePathSegment::Last),
        DeleteOp::UrlParams,
        DeleteOp::UrlParam("token".to_string()),
        DeleteOp::ReqHeader("x-old".to_string()),
        DeleteOp::ResHeader("x-old".to_string()),
        DeleteOp::ReqBody,
        DeleteOp::ResBody,
        DeleteOp::ReqBodyPath(escaped_path.clone()),
        DeleteOp::ResBodyPath(escaped_path),
        DeleteOp::ReqType,
        DeleteOp::ResType,
        DeleteOp::ReqCharset,
        DeleteOp::ResCharset,
        DeleteOp::ReqCookie("sid".to_string()),
        DeleteOp::ResCookie("sid".to_string()),
        DeleteOp::ReqCookies,
        DeleteOp::ResCookies,
        DeleteOp::Trailer("x-old".to_string()),
        DeleteOp::Trailers,
    ]));
    for fragment in [
        "pathname",
        "pathname.-1",
        "pathname.last",
        "urlParams.token",
        "reqBody.a",
        "resBody.a",
        "reqCharset",
        "resCookies",
        "trailer.x-old",
        "trailers",
        "\\n",
        "\\r",
        "\\t",
        "\\f",
        "\\v",
    ] {
        assert!(delete.contains(fragment), "missing {fragment} in {delete}");
    }

    let body_values = [
        (Value::inline("x"), "x"),
        (Value::File("body.bin".to_string()), "<body.bin>"),
        (Value::Reference("body".to_string()), "@body"),
    ];
    for (value, expected) in body_values.clone() {
        assert_eq!(
            explain(Action::ReqBody(BodyOp::Set(value))),
            format!("req.body.set({expected})")
        );
    }
    for (value, expected) in body_values.clone() {
        assert_eq!(
            explain(Action::ReqBody(BodyOp::Prepend(value))),
            format!("req.body.prepend({expected})")
        );
    }
    for (value, expected) in body_values {
        assert_eq!(
            explain(Action::ResBody(BodyOp::Append(value))),
            format!("res.body.append({expected})")
        );
    }
    assert_eq!(
        explain(Action::ResBody(BodyOp::Replace {
            pattern: RegexReplacePattern::new("old".to_string(), true).unwrap(),
            replacement: "new".to_string(),
        })),
        "res.body.replace(/old/i, new)"
    );

    for (target, mode, expected) in [
        (
            InjectTarget::Html,
            InjectMode::Append,
            "inject(html, x, append)",
        ),
        (
            InjectTarget::Js,
            InjectMode::Prepend,
            "inject(js, x, prepend)",
        ),
        (
            InjectTarget::Css,
            InjectMode::Replace,
            "inject(css, x, replace)",
        ),
    ] {
        assert_eq!(
            explain(Action::Inject(InjectOp {
                target,
                value: Value::inline("x"),
                mode,
            })),
            expected
        );
    }
    assert_eq!(explain(Action::Direct), "direct");
    assert_eq!(explain(Action::Bypass), "bypass");
    assert_eq!(explain(Action::Hide), "hide");
    assert_eq!(explain(Action::Skip(Vec::new())), "skip()");
    assert_eq!(
        explain(Action::Skip(vec![
            "cache".to_string(),
            "res.header".to_string()
        ])),
        "skip(cache, res.header)"
    );
    assert_eq!(explain(Action::Attachment(None)), "Attachment(None)");
    assert!(
        explain(Action::Delay {
            phase: Phase::Req,
            millis: 5,
        })
        .starts_with("Delay")
    );
    assert!(
        explain(Action::Throttle {
            phase: Phase::Res,
            bytes_per_sec: 1024,
        })
        .starts_with("Throttle")
    );
}
