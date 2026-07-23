use super::*;

fn parse_error(action_or_suffix: &str) -> RuleError {
    RuleSet::parse("edges", &format!("example.test {action_or_suffix}"))
        .unwrap_err()
        .remove(0)
}

#[test]
fn syntax_helpers_reject_malformed_calls_values_durations_and_speeds() {
    assert!(parse_call("missing").is_err());
    assert!(parse_call("call(").is_err());
    assert!(parse_call("(value)").is_err());
    assert!(require_one(&[], "value").is_err());
    assert!(require_one(&[""], "value").is_err());
    assert!(require_one(&["a", "b"], "value").is_err());
    assert!(require_call_body("value()", "value").is_err());
    assert!(parse_value("<>").is_err());
    assert!(parse_value("<missing-end").is_err());
    assert!(parse_value("missing-start>").is_err());
    assert!(parse_value("@bad/key").is_err());

    assert_eq!(parse_duration_ms("1.5s").unwrap(), 1500);
    assert_eq!(parse_duration_ms("20ms").unwrap(), 20);
    assert_eq!(parse_duration_ms("30").unwrap(), 30);
    for invalid in ["badms", "bads", "bad", "-1s", "NaNs", "infs", "1e999s"] {
        assert!(parse_duration_ms(invalid).is_err());
    }

    for (input, expected) in [
        ("2kb/s", 2048),
        ("2k", 2048),
        ("2mb/s", 2 * 1024 * 1024),
        ("2m", 2 * 1024 * 1024),
        ("2b", 2),
        ("2", 2),
    ] {
        assert_eq!(parse_speed_bps(input).unwrap(), expected);
    }
    assert!(parse_speed_bps("bad").is_err());
    assert!(parse_speed_bps("0").is_err());
    for invalid in ["-1KB/s", "NaN", "inf", "1e999MB/s"] {
        assert!(parse_speed_bps(invalid).is_err(), "{invalid}");
    }
    for invalid in ["call(a,)", "call(,a)", "call(a,,b)"] {
        assert!(parse_call(invalid).is_err(), "{invalid}");
    }
    assert_eq!(unquote(r#""line\rcolumn\tend""#), "line\rcolumn\tend");
}

#[test]
fn parser_rejects_unbalanced_call_delimiters() {
    for source in [
        "example.test mock(<fixture.txt)",
        "example.test mock(fixture.txt>)",
        "example.test res.merge({\"ok\": true)",
        "example.test res.merge([1, 2)",
        "example.test req.header(x: ${host)",
    ] {
        assert!(RuleSet::parse("delimiter", source).is_err(), "{source}");
    }

    let nesting = "{".repeat(MAX_RULE_PARSE_NESTING + 1) + &"}".repeat(MAX_RULE_PARSE_NESTING + 1);
    let error =
        RuleSet::parse("delimiter", &format!("example.test res.merge({nesting})")).unwrap_err();
    assert!(error[0].message.contains("call nesting exceeds"));
}

#[test]
fn action_parser_exercises_aliases_defaults_and_all_body_forms() {
    let rules = RuleSet::parse(
        "edges",
        concat!(
            "example.test ",
            "upstream(proxy://one, proxy://two) ",
            "mock(<mock.txt>) mockRaw(@raw) mock_raw(raw2) mock-raw(raw3) ",
            "redirect(/next) req.header(-x-old) res.header(x-new: value) ",
            "res.status(299) req.method(PATCH) req.cookie(-sid) ",
            "res.cookie(sid=1; Partitioned; Priority=High) req.ua(agent) ",
            "req.referer(https://ref.test) req.auth(user:pass) req.forwarded(192.0.2.1) ",
            "req.type(application/json) req.charset(utf-8) res.type(text/plain) ",
            "res.charset(utf-8) res.merge({}) res.trailer(-x-old) attachment() ",
            "url.query(page=2, -token) req.body.set(<request.bin>) ",
            "req.body.prepend(@prefix) req.body.append(suffix) req.body.replace(old, new) ",
            "res.body.set(<response.bin>) res.body.prepend(@prefix) res.body.append(suffix) ",
            "res.body.replace(/old/i, new) inject(javascript, script, replace) ",
            "delay(req, 1.5s) delay(res, 2ms) throttle(req, 2k) throttle(res, 2mb/s)"
        ),
    )
    .unwrap();
    assert_eq!(rules.rules().len(), 1);
    assert!(rules.rules()[0].actions.len() >= 35);
    assert!(matches!(
        rules.rules()[0].actions[2],
        Action::MockRaw(Value::Reference(_))
    ));
    assert!(matches!(
        rules.rules()[0].actions.last(),
        Some(Action::Throttle {
            phase: Phase::Res,
            bytes_per_sec: 2_097_152
        })
    ));
}

#[test]
fn action_parser_rejects_each_malformed_structured_family() {
    let invalid = [
        "upstream()",
        "status(nope)",
        "status(99)",
        "status(600)",
        "redirect()",
        "redirect(/next, nope)",
        "redirect(/next, 200)",
        "redirect(/next, 400)",
        "redirect(/next, 302, ignored)",
        "res.status(nope)",
        "res.status(99)",
        "res.status(600)",
        "attachment(one, two)",
        "url.rewrite(one)",
        "url.query()",
        "url.query(-)",
        "url.query(=value)",
        "req.body.replace(one)",
        "req.body.replace(/[/, value)",
        "inject(html)",
        "inject(xml, value)",
        "inject(html, value, sideways)",
        "delay(req)",
        "delay(other, 1)",
        "delay(req, nope)",
        "throttle(req)",
        "throttle(other, 1)",
        "throttle(req, 0)",
        "tls()",
        "tls(client-cert=)",
        "tls(client-key=)",
        "tls(ciphers=)",
        "tls(no-equals)",
        "cache()",
        "res.cors()",
        "res.cors(*, credentials=maybe)",
        "res.cors(*, unknown=value)",
        "req.header(-)",
        "req.header(~ /a/b)",
        "req.header(no-colon)",
        "req.header(x ~ nope)",
        "req.header(x ~ /unterminated)",
        "req.cookie(-)",
        "req.cookie(name)",
        "req.cookie(=value)",
        "skip(unknown.family)",
        "skip(\"\")",
    ];
    for action in invalid {
        let error = parse_error(action);
        assert_eq!(error.code, RuleErrorCode::Action, "{action}: {error:?}");
        assert!(!error.message.is_empty(), "{action}");
    }
}

#[test]
fn skip_families_are_validated_and_canonicalized_during_parsing() {
    let rules = RuleSet::parse("skip", "example.test skip(REQ_HEADER, res-body, all, *)").unwrap();
    assert_eq!(
        rules.rules()[0].actions[0],
        Action::Skip(ActionFamilySet::ALL)
    );
}

#[test]
fn condition_parser_rejects_empty_invalid_and_out_of_range_arguments() {
    let invalid = [
        "status(200) when method()",
        "status(200) when method(BAD METHOD)",
        "status(200) when clientIp()",
        "status(200) when clientIp(\"\")",
        "status(200) when header()",
        "status(200) when header(x-name~)",
        "status(200) when status()",
        "status(200) when status(nope)",
        "status(200) when status(99)",
        "status(200) when status(600)",
        "status(200) when chance(nope)",
        "status(200) when chance(1.1)",
        "status(200) when env(=value)",
        "status(200) when env(BAD NAME)",
        "status(200) when env(BAD\0NAME)",
        "status(200) when any()",
        "status(200) when body(~)",
        "status(200) when unknown(value)",
    ];
    for suffix in invalid {
        let error = parse_error(suffix);
        assert_eq!(error.code, RuleErrorCode::Condition, "{suffix}: {error:?}");
        assert!(!error.message.is_empty(), "{suffix}");
    }
}

#[test]
fn glob_syntax_is_validated_before_snapshot_publication() {
    for source in [
        r"example.test\?query status(200)",
        r#"example.test status(200) when host("example.test\\")"#,
        r#"example.test status(200) when url("/path\\")"#,
        r#"example.test status(200) when clientIp("192.0.2.\\")"#,
    ] {
        let error = RuleSet::parse("glob", source).unwrap_err().remove(0);
        assert!(
            matches!(
                error.code,
                RuleErrorCode::Matcher | RuleErrorCode::Condition
            ),
            "{source}: {error:?}"
        );
        assert!(
            error.message.contains("glob pattern"),
            "{source}: {error:?}"
        );
    }
}

#[test]
fn properties_are_registry_driven_and_reject_empty_tags() {
    let rules = RuleSet::parse(
        "properties",
        "example.test status(200) @important @disabled @tag:health",
    )
    .unwrap();
    assert!(rules.rules()[0].important);
    assert!(rules.rules()[0].disabled);
    assert_eq!(rules.rules()[0].tags, ["health"]);

    let error = RuleSet::parse("properties", "example.test status(200) @tag:")
        .unwrap_err()
        .remove(0);
    assert_eq!(error.code, RuleErrorCode::Property);
    assert!(error.message.contains("non-empty"));
}

#[test]
fn parser_rejects_unbounded_lines_and_recursive_grammar_depth() {
    let oversized = "x".repeat(MAX_RULE_LINE_BYTES + 1);
    let error = RuleSet::parse("limits", &oversized).unwrap_err().remove(0);
    assert_eq!(error.code, RuleErrorCode::Syntax);
    assert_eq!(error.group, "limits");
    assert_eq!(error.line, 1);
    assert!(error.message.contains("65536-byte limit"));

    let accepted_matcher = format!("{}example.test status(200)", "!".repeat(MAX_PARSE_NESTING));
    assert!(RuleSet::parse("limits", &accepted_matcher).is_ok());
    let rejected_matcher = format!(
        "{}example.test status(200)",
        "!".repeat(MAX_PARSE_NESTING + 1)
    );
    let error = RuleSet::parse("limits", &rejected_matcher)
        .unwrap_err()
        .remove(0);
    assert_eq!(error.code, RuleErrorCode::Matcher);
    assert!(error.message.contains("matcher nesting exceeds 32 levels"));

    let accepted_condition = format!(
        "example.test status(200) when {}method(GET)",
        "!".repeat(MAX_PARSE_NESTING)
    );
    assert!(RuleSet::parse("limits", &accepted_condition).is_ok());
    let rejected_condition = format!(
        "example.test status(200) when {}method(GET)",
        "!".repeat(MAX_PARSE_NESTING + 1)
    );
    let error = RuleSet::parse("limits", &rejected_condition)
        .unwrap_err()
        .remove(0);
    assert_eq!(error.code, RuleErrorCode::Condition);
    assert!(
        error
            .message
            .contains("condition nesting exceeds 32 levels")
    );

    let nested = format!(
        "example.test status(200) when {}method(GET){}",
        "any(".repeat(MAX_PARSE_NESTING + 1),
        ")".repeat(MAX_PARSE_NESTING + 1),
    );
    let error = RuleSet::parse("limits", &nested).unwrap_err().remove(0);
    assert_eq!(error.code, RuleErrorCode::Syntax);
    assert!(error.message.contains("rule nesting exceeds 32 levels"));
}

#[test]
fn delete_aliases_and_body_path_limits_are_explicit() {
    let rules = RuleSet::parse(
        "edges",
        "example.test delete(urlParams, headers.x-old, body, req.type, res.type, req.charset, res.charset, cookies, cookie.sid, trailers)",
    )
    .unwrap();
    let Action::Delete(operations) = &rules.rules()[0].actions[0] else {
        panic!("delete action expected");
    };
    assert_eq!(operations.len(), 14);

    for property in [
        "",
        "\"\"",
        "headers.",
        "cookies.",
        "reqBody.a[999999999999999999999999999]",
    ] {
        let error = parse_error(&format!("delete({property})"));
        assert_eq!(error.code, RuleErrorCode::Action);
    }

    let oversized = "a".repeat(16 * 1024 + 1);
    let error = parse_error(&format!("delete(reqBody.{oversized})"));
    assert!(error.message.contains("exceeds 16384 bytes"));

    let too_many = std::iter::repeat_n("a", 129).collect::<Vec<_>>().join(".");
    let error = parse_error(&format!("delete(reqBody.{too_many})"));
    assert!(error.message.contains("exceeds 128 segments"));
    assert!(DeleteBodyPath::new(Vec::new()).is_err());
}

#[test]
fn cookie_and_cache_aliases_canonicalize_every_documented_spelling() {
    let rules = RuleSet::parse(
        "edges",
        concat!(
            "example.test ",
            "res.cookie(id=value; max_age=60; http-only; same_site=Lax; x-custom-flag=on) ",
            "cache(max_age=60, s_maxage=120, stale_while_revalidate=30, ",
            "stale_if_error=10, must_revalidate, proxy_revalidate, no_cache, ",
            "no_store, no_transform)"
        ),
    )
    .unwrap();

    let Action::ResCookie(CookieOp::Set { attrs, .. }) = &rules.rules()[0].actions[0] else {
        panic!("response cookie action expected");
    };
    assert_eq!(
        attrs
            .iter()
            .map(|attr| attr.name.as_str())
            .collect::<Vec<_>>(),
        ["Max-Age", "HttpOnly", "SameSite", "X-Custom-Flag"]
    );

    let Action::Cache(CacheOp::Directives(directives)) = &rules.rules()[0].actions[1] else {
        panic!("cache directives expected");
    };
    assert_eq!(
        directives
            .iter()
            .map(|directive| directive.name.as_str())
            .collect::<Vec<_>>(),
        [
            "max-age",
            "s-maxage",
            "stale-while-revalidate",
            "stale-if-error",
            "must-revalidate",
            "proxy-revalidate",
            "no-cache",
            "no-store",
            "no-transform",
        ]
    );

    let error = parse_error("req.body.replace([, replacement)");
    assert_eq!(error.code, RuleErrorCode::Action);
    assert!(!error.message.is_empty());
}

#[test]
fn matcher_and_delete_parsers_report_every_authority_and_path_shape() {
    for matcher in [
        "=",
        "=http://[",
        "=1http://example.test",
        "1http://example.test",
        "http:///path",
        "[::1",
        "[::1]suffix",
        "host[",
        "::1",
        "example.test:",
        "example.test:*x",
        "example.test:0",
        "example.test:70000",
        "/(/",
    ] {
        assert!(
            RuleSet::parse("edges", &format!("{matcher} status(200)")).is_err(),
            "{matcher}"
        );
    }

    let rules = RuleSet::parse("edges", "https://[::1]:4*/path?query status(200)").unwrap();
    let Matcher::Glob(glob) = &rules.rules()[0].matcher else {
        panic!("glob matcher expected");
    };
    assert_eq!(glob.host, "::1");
    assert_eq!(glob.port.as_deref(), Some("4*"));
    assert_eq!(glob.path.as_deref(), Some("/path"));
    assert_eq!(glob.query.as_deref(), Some("query"));

    let rules = RuleSet::parse(
        "edges",
        concat!(
            "example.test delete(",
            "pathname.first, pathname.last, pathname.-2, ",
            "reqBody.a\\], reqBody.a[], reqBody.a[01], reqBody.a[x], reqBody.[2], ",
            "reqBody.\\r.\\t.\\f.\\v)"
        ),
    )
    .unwrap();
    let Action::Delete(operations) = &rules.rules()[0].actions[0] else {
        panic!("delete action expected");
    };
    assert_eq!(operations.len(), 9);

    let error = parse_error("delete(pathname.not-an-index)");
    assert_eq!(error.code, RuleErrorCode::Action);
}
