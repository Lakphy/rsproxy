use super::*;

#[test]
fn regex_matcher_supports_indexed_and_named_captures() {
    let rules = RuleSet::parse(
            "default",
            r#"/\/users\/(?P<uid>\d+)\/orders\/(\w+)/ req.header(x-uid: ${uid}) req.header(x-order: $2)"#,
        )
        .unwrap();
    let result = rules.resolve(&req("http://example.com/users/42/orders/abc"));
    assert_eq!(result.actions.len(), 2);
    assert!(matches!(
        &result.actions[0].action,
        Action::ReqHeader(HeaderOp::Set { value, .. }) if result.actions[0].captures.render(value.as_inline().unwrap(), &req("http://example.com/users/42/orders/abc")) == "42"
    ));
    assert!(matches!(
        &result.actions[1].action,
        Action::ReqHeader(HeaderOp::Set { value, .. }) if result.actions[1].captures.render(value.as_inline().unwrap(), &req("http://example.com/users/42/orders/abc")) == "abc"
    ));
}

#[test]
fn regex_matcher_falls_back_to_fancy_for_lookahead() {
    let rules = RuleSet::parse(
        "default",
        r#"/\/pay\/(\d+)(?=\?ok=1)/ req.header(x-pay: $1)"#,
    )
    .unwrap();
    assert!(matches!(
        &rules.rules[0].matcher,
        Matcher::Regex(RegexMatcher {
            engine: RegexEngine::Fancy,
            ..
        })
    ));

    let request = req("http://example.com/pay/42?ok=1");
    let result = rules.resolve(&request);
    assert_eq!(result.actions.len(), 1);
    assert!(matches!(
        &result.actions[0].action,
        Action::ReqHeader(HeaderOp::Set { value, .. })
            if result.actions[0].captures.render(value.as_inline().unwrap(), &request) == "42"
    ));
}

#[test]
fn regex_matcher_falls_back_to_fancy_for_backreference() {
    let rules = RuleSet::parse("default", r#"/\/dup\/(\w+)\/\1/ req.header(x-dup: $1)"#).unwrap();
    assert!(matches!(
        &rules.rules[0].matcher,
        Matcher::Regex(RegexMatcher {
            engine: RegexEngine::Fancy,
            ..
        })
    ));

    let request = req("http://example.com/dup/abc/abc");
    let result = rules.resolve(&request);
    assert_eq!(result.actions.len(), 1);
    assert!(matches!(
        &result.actions[0].action,
        Action::ReqHeader(HeaderOp::Set { value, .. })
            if result.actions[0].captures.render(value.as_inline().unwrap(), &request) == "abc"
    ));
}

#[test]
fn fancy_regex_backtrack_limit_is_treated_as_no_match() {
    let rules = RuleSet::parse(
        "default",
        r#"/(a|b|ab)*(?=c)/i req.header(x-redos: matched)"#,
    )
    .unwrap();
    assert!(matches!(
        &rules.rules[0].matcher,
        Matcher::Regex(RegexMatcher {
            engine: RegexEngine::Fancy,
            ..
        })
    ));

    let request = req(&format!("http://a.test/{}", "ab".repeat(30)));
    let result = rules.resolve(&request);
    assert!(result.actions.is_empty());
}
