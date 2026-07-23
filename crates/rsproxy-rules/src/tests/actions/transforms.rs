use super::super::*;

#[test]
fn parses_mock_raw_and_keeps_mock_family_first_match() {
    let rules = RuleSet::parse(
            "default",
            "example.com mock.raw(\"HTTP/1.1 207 Multi-Status\\r\\nX-Raw: ${path}\\r\\n\\r\\nraw\")\nexample.com mock(\"fallback\")",
        )
        .unwrap();
    assert!(matches!(
        rules.rules()[0].actions[0],
        Action::MockRaw(Value::Inline(_))
    ));

    let request = req("http://example.com/raw");
    let result = rules.resolve(&request);
    assert_eq!(result.actions.len(), 1);
    assert!(matches!(result.actions[0].action, Action::MockRaw(_)));
    assert_eq!(
        rules.explain(&request),
        "default:1 mock.raw(HTTP/1.1 207 Multi-Status\r\nX-Raw: /raw\r\n\r\nraw)\n"
    );
}

#[test]
fn parses_url_query_and_body_actions() {
    let rules = RuleSet::parse(
            "default",
            "example.com url.query(a=1, -b) req.body.prepend(\"pre-\") req.body.replace(/foo-(\\d+)/, bar-$1) res.body.append(@tail) res.body.replace(/hello/i, bye) inject(html, \"<!--${path}-->\", prepend) res.status(299)",
        )
        .unwrap();
    assert!(matches!(rules.rules()[0].actions[0], Action::UrlQuery(_)));
    assert!(matches!(
        rules.rules()[0].actions[1],
        Action::ReqBody(BodyOp::Prepend(_))
    ));
    assert!(matches!(
        rules.rules()[0].actions[2],
        Action::ReqBody(BodyOp::Replace { .. })
    ));
    assert!(matches!(
        rules.rules()[0].actions[3],
        Action::ResBody(BodyOp::Append(Value::Reference(_)))
    ));
    assert!(matches!(
        rules.rules()[0].actions[4],
        Action::ResBody(BodyOp::Replace { .. })
    ));
    assert!(matches!(
        rules.rules()[0].actions[5],
        Action::Inject(InjectOp {
            target: InjectTarget::Html,
            mode: InjectMode::Prepend,
            ..
        })
    ));
    assert!(matches!(
        rules.rules()[0].actions[6],
        Action::ResStatus(299)
    ));

    let request = req("http://example.com/items");
    assert_eq!(
        rules
            .explain(&request)
            .lines()
            .find(|line| line.contains("inject(")),
        Some("default:1 inject(html, <!--/items-->, prepend)")
    );
}

#[test]
fn parses_typed_delete_properties_and_explains_the_canonical_form() {
    let rules = RuleSet::parse(
        "default",
        r#"example.com delete(pathname.0, pathname.last, urlParams.drop, reqHeaders.x-old, resHeaders.x-old, reqBody, reqBody.user.token, reqBody.items[1].secret, reqBody.meta.a\.b, resBody, resBody.payload[0].secret, reqType, resCharset, reqCookies.sid, resCookies.sid, trailer.x-old)"#,
    )
    .unwrap();
    let Action::Delete(operations) = &rules.rules()[0].actions[0] else {
        panic!("expected delete action");
    };
    assert_eq!(
        operations,
        &[
            DeleteOp::PathSegment(DeletePathSegment::Index(0)),
            DeleteOp::PathSegment(DeletePathSegment::Last),
            DeleteOp::UrlParam("drop".to_string()),
            DeleteOp::ReqHeader("x-old".to_string()),
            DeleteOp::ResHeader("x-old".to_string()),
            DeleteOp::ReqBody,
            DeleteOp::ReqBodyPath(
                DeleteBodyPath::new(vec![
                    DeleteBodyPathSegment::Key("user".to_string()),
                    DeleteBodyPathSegment::Key("token".to_string()),
                ])
                .unwrap(),
            ),
            DeleteOp::ReqBodyPath(
                DeleteBodyPath::new(vec![
                    DeleteBodyPathSegment::Key("items".to_string()),
                    DeleteBodyPathSegment::Index(1),
                    DeleteBodyPathSegment::Key("secret".to_string()),
                ])
                .unwrap(),
            ),
            DeleteOp::ReqBodyPath(
                DeleteBodyPath::new(vec![
                    DeleteBodyPathSegment::Key("meta".to_string()),
                    DeleteBodyPathSegment::Key("a.b".to_string()),
                ])
                .unwrap(),
            ),
            DeleteOp::ResBody,
            DeleteOp::ResBodyPath(
                DeleteBodyPath::new(vec![
                    DeleteBodyPathSegment::Key("payload".to_string()),
                    DeleteBodyPathSegment::Index(0),
                    DeleteBodyPathSegment::Key("secret".to_string()),
                ])
                .unwrap(),
            ),
            DeleteOp::ReqType,
            DeleteOp::ResCharset,
            DeleteOp::ReqCookie("sid".to_string()),
            DeleteOp::ResCookie("sid".to_string()),
            DeleteOp::Trailer("x-old".to_string()),
        ]
    );

    assert_eq!(
        rules.explain(&req("http://example.com/api/item")),
        r#"default:1 delete(pathname.0, pathname.last, urlParams.drop, reqHeaders.x-old, resHeaders.x-old, reqBody, reqBody.user.token, reqBody.items[1].secret, reqBody.meta.a\.b, resBody, resBody.payload[0].secret, reqType, resCharset, reqCookies.sid, resCookies.sid, trailer.x-old)
"#
    );
}

#[test]
fn delete_rejects_unknown_and_empty_body_properties() {
    for property in ["unknown", "reqBody.", "resBody."] {
        let errors =
            RuleSet::parse("default", &format!("example.com delete({property})")).unwrap_err();
        assert_eq!(errors[0].code, RuleErrorCode::Action);
    }
}

#[test]
fn delete_body_paths_decode_documented_special_character_escapes() {
    let rules = RuleSet::parse(
        "default",
        r#"example.com delete(reqBody.\n\ \.p.test\|\&test, reqBody.a\,b)"#,
    )
    .unwrap();
    let Action::Delete(operations) = &rules.rules()[0].actions[0] else {
        panic!("expected delete action");
    };

    assert_eq!(
        operations,
        &[
            DeleteOp::ReqBodyPath(
                DeleteBodyPath::new(vec![
                    DeleteBodyPathSegment::Key("\n .p".to_string()),
                    DeleteBodyPathSegment::Key("test|&test".to_string()),
                ])
                .unwrap(),
            ),
            DeleteOp::ReqBodyPath(
                DeleteBodyPath::new(vec![DeleteBodyPathSegment::Key("a,b".to_string())]).unwrap(),
            ),
        ]
    );
    assert_eq!(
        rules.explain(&req("http://example.com/")),
        "default:1 delete(reqBody.\\n\\ \\.p.test\\|\\&test, reqBody.a\\,b)\n"
    );
}

#[test]
fn parses_url_rewrite_regex_without_consuming_replacement_captures() {
    let rules = RuleSet::parse(
        "default",
        r#"example.com url.rewrite(/\/api\/v(\d+)/, /v$1)"#,
    )
    .unwrap();
    assert!(matches!(
        &rules.rules()[0].actions[0],
        Action::UrlRewrite {
            from: UrlRewritePattern::Regex(pattern),
            to,
        } if !pattern.is_case_insensitive() && to.as_inline() == Some("/v$1")
    ));

    let request = req("http://example.com/api/v2/items");
    assert_eq!(
        rules.explain(&request),
        "default:1 url.rewrite(/\\/api\\/v(\\d+)/, /v$1)\n"
    );
}

#[test]
fn parses_res_merge_json_with_commas_and_templates() {
    let rules = RuleSet::parse(
        "default",
        r#"/\/users\/(\d+)/ res.merge({"ok":true,"user":"$1","nested":{"source":"${host}"}})"#,
    )
    .unwrap();
    assert!(matches!(
        &rules.rules()[0].actions[0],
        Action::ResMerge(value)
            if value.as_inline() == Some(r#"{"ok":true,"user":"$1","nested":{"source":"${host}"}}"#)
    ));

    let request = req("http://example.com/users/42");
    assert_eq!(
        rules.explain(&request),
        r#"default:1 res.merge({"ok":true,"user":"42","nested":{"source":"example.com"}})"#
            .to_string()
            + "\n"
    );
}
#[test]
fn parses_res_trailer_as_stackable_response_action() {
    let rules = RuleSet::parse(
        "default",
        r#"/\/jobs\/(\d+)/ res.trailer(x-job: $1) res.trailer(x-source: ${host})"#,
    )
    .unwrap();
    assert!(matches!(
        &rules.rules()[0].actions[0],
        Action::ResTrailer(HeaderOp::Set { name, value })
            if name == "x-job" && value.as_inline() == Some("$1")
    ));
    assert!(matches!(
        &rules.rules()[0].actions[1],
        Action::ResTrailer(HeaderOp::Set { name, value })
            if name == "x-source" && value.as_inline() == Some("${host}")
    ));

    let request = req("http://example.com/jobs/42");
    assert_eq!(
        rules.explain(&request),
        "default:1 res.trailer(x-job: 42)\ndefault:1 res.trailer(x-source: example.com)\n"
    );
    assert_eq!(rules.resolve(&request).actions.len(), 2);
}

#[test]
fn response_trailer_actions_reject_fields_that_control_message_semantics() {
    for name in [
        "content-length",
        "transfer-encoding",
        "trailer",
        "host",
        "cache-control",
    ] {
        let errors = RuleSet::parse(
            "trailers",
            &format!("example.test res.trailer({name}: unsafe)"),
        )
        .unwrap_err();
        assert_eq!(errors[0].code, RuleErrorCode::Action, "{name}");
        assert!(
            errors[0].message.contains("forbidden in a trailer section"),
            "{name}"
        );

        // Removal is safe and remains useful for stripping an upstream field.
        RuleSet::parse("trailers", &format!("example.test res.trailer(-{name})"))
            .unwrap_or_else(|errors| panic!("{name}: {errors:?}"));
    }

    RuleSet::parse("trailers", "example.test res.trailer(grpc-status: 0)").unwrap();
}
