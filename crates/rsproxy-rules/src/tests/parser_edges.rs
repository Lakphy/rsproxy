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
    assert!(parse_value("@bad/key").is_err());

    assert_eq!(parse_duration_ms("1.5s").unwrap(), 1500);
    assert_eq!(parse_duration_ms("20ms").unwrap(), 20);
    assert_eq!(parse_duration_ms("30").unwrap(), 30);
    for invalid in ["badms", "bads", "bad"] {
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
    assert_eq!(unquote(r#""line\rcolumn\tend""#), "line\rcolumn\tend");
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
    assert_eq!(rules.rules.len(), 1);
    assert!(rules.rules[0].actions.len() >= 35);
    assert!(matches!(
        rules.rules[0].actions[2],
        Action::MockRaw(Value::Reference(_))
    ));
    assert!(matches!(
        rules.rules[0].actions.last(),
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
        "redirect()",
        "redirect(/next, nope)",
        "res.status(nope)",
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
    ];
    for action in invalid {
        let error = parse_error(action);
        assert_eq!(error.code, RuleErrorCode::Action, "{action}: {error:?}");
        assert!(!error.message.is_empty(), "{action}");
    }
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
        "status(200) when chance(nope)",
        "status(200) when chance(1.1)",
        "status(200) when env(=value)",
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
fn delete_aliases_and_body_path_limits_are_explicit() {
    let rules = RuleSet::parse(
        "edges",
        "example.test delete(urlParams, headers.x-old, body, req.type, res.type, req.charset, res.charset, cookies, cookie.sid, trailers)",
    )
    .unwrap();
    let Action::Delete(operations) = &rules.rules[0].actions[0] else {
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
