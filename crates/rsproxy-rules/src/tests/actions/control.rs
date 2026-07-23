use super::super::*;

#[test]
fn skipped_single_family_rule_is_not_reported_as_matched() {
    let mut request = req("http://example.com/");
    request.method = "POST".to_string();
    let rules = RuleSet::parse(
        "default",
        "example.com status(410) when method(POST)\nexample.com status(200)",
    )
    .unwrap();
    let result = rules.resolve(&request);
    assert!(matches!(result.actions[0].action, Action::Status(410)));
    assert_eq!(result.matched_rules.len(), 1);
    assert_eq!(result.matched_rules[0].line, 1);
}

#[test]
fn skip_action_suppresses_named_families_or_all_following_actions() {
    let request = req("http://example.com/");
    let rules = RuleSet::parse(
            "default",
            "example.com skip(res.header) res.header(x-same: no) cache(62)\nexample.com res.header(x-later: no) cache(63)",
        )
        .unwrap();
    let result = rules.resolve(&request);
    assert_eq!(result.actions.len(), 2);
    assert!(matches!(result.actions[0].action, Action::Skip(_)));
    assert!(matches!(result.actions[1].action, Action::Cache(_)));
    assert_eq!(
        rules.explain(&request),
        "default:1 skip(res.header)\ndefault:1 cache(max-age=62)\n"
    );

    let all = RuleSet::parse(
        "default",
        "example.com skip()\nexample.com res.header(x-later: no) cache(63)",
    )
    .unwrap();
    let result = all.resolve(&request);
    assert_eq!(result.actions.len(), 1);
    assert!(matches!(result.actions[0].action, Action::Skip(_)));
    assert_eq!(all.explain(&request), "default:1 skip()\n");
}

#[test]
fn hide_and_tag_explain_as_control_actions() {
    let request = req("http://example.com/trace-me");
    let rules = RuleSet::parse(
        "default",
        "example.com tag(${path}) hide res.header(x-visible: no)",
    )
    .unwrap();
    let result = rules.resolve(&request);
    assert_eq!(result.actions.len(), 3);
    assert!(matches!(result.actions[0].action, Action::Tag(_)));
    assert!(matches!(result.actions[1].action, Action::Hide));
    assert_eq!(
        rules.explain(&request),
        "default:1 tag(/trace-me)\ndefault:1 hide\ndefault:1 res.header(x-visible: no)\n"
    );
}
#[test]
fn req_forwarded_explains_template_and_uses_first_match() {
    let rules = RuleSet::parse(
        "default",
        "example.com req.forwarded(${clientIp})\nexample.com req.forwarded(10.0.0.2)",
    )
    .unwrap();
    let mut request = req("http://example.com/");
    request.client_ip = Some("10.0.0.1".to_string());
    let result = rules.resolve(&request);

    assert_eq!(result.actions.len(), 1);
    assert!(matches!(
        &result.actions[0].action,
        Action::ReqForwarded(value) if value.as_inline() == Some("${clientIp}")
    ));
    assert_eq!(
        rules.explain(&request),
        "default:1 req.forwarded(10.0.0.1)\n"
    );
}

#[test]
fn parses_multi_hop_upstream_action() {
    let rules = RuleSet::parse(
        "default",
        "example.com upstream(proxy://127.0.0.1:18001, proxy://127.0.0.1:18002)",
    )
    .unwrap();

    assert_eq!(
        rules.rules()[0].actions[0],
        Action::Upstream(Value::inline(
            "proxy://127.0.0.1:18001, proxy://127.0.0.1:18002"
        ))
    );
}

#[test]
fn redacts_socks_credentials_in_observable_rule_text() {
    assert_eq!(
        redact_secrets("example.com upstream(socks5://alice:secret@127.0.0.1:1080)"),
        "example.com upstream(socks5://auth@127.0.0.1:1080)"
    );
    assert_eq!(
        redact_secrets("example.com upstream(socks://bob:pw@proxy.test:1080)"),
        "example.com upstream(socks://auth@proxy.test:1080)"
    );
}
