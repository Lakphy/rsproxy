use rsproxy_rules::{Action, BodyOp, HeaderOp, RuleErrorCode, RuleSet, UrlRewritePattern, Value};

#[test]
fn parser_preserves_structured_value_sources_across_action_categories() {
    let rules = RuleSet::parse(
        "default",
        concat!(
            "example.test host(@host, <host.txt>) upstream(@upstream) ",
            "redirect(<redirect.txt>) req.header(x-ref: @header) req.method(<method.txt>) ",
            "res.header(x-ref: <header.txt>) res.type(@type) ",
            "url.rewrite(@from, <to.txt>) req.body.set(@body) tag(\"@literal\")"
        ),
    )
    .unwrap();
    let actions = &rules.rules[0].actions;

    assert!(matches!(
        &actions[0],
        Action::Host(pool)
            if pool.addresses()
                == [Value::Reference("host".to_string()), Value::File("host.txt".to_string())]
    ));
    assert!(matches!(
        &actions[1],
        Action::Upstream(Value::Reference(key)) if key == "upstream"
    ));
    assert!(matches!(
        &actions[2],
        Action::Redirect { url: Value::File(path), code: 302 } if path == "redirect.txt"
    ));
    assert!(matches!(
        &actions[3],
        Action::ReqHeader(HeaderOp::Set { value: Value::Reference(key), .. }) if key == "header"
    ));
    assert!(matches!(
        &actions[4],
        Action::ReqMethod(Value::File(path)) if path == "method.txt"
    ));
    assert!(matches!(
        &actions[5],
        Action::ResHeader(HeaderOp::Set { value: Value::File(path), .. }) if path == "header.txt"
    ));
    assert!(matches!(
        &actions[6],
        Action::ResType(Value::Reference(key)) if key == "type"
    ));
    assert!(matches!(
        &actions[7],
        Action::UrlRewrite {
            from: UrlRewritePattern::Plain(Value::Reference(from)),
            to: Value::File(to),
        } if from == "from" && to == "to.txt"
    ));
    assert!(matches!(
        &actions[8],
        Action::ReqBody(BodyOp::Set(Value::Reference(key))) if key == "body"
    ));
    assert!(matches!(
        &actions[9],
        Action::Tag(Value::Inline(value)) if value == "@literal"
    ));
}

#[test]
fn parser_rejects_invalid_reference_keys_and_empty_file_paths() {
    let too_long = format!("@{}", "a".repeat(129));
    for value in [
        "@".to_string(),
        "@../escape".to_string(),
        "@bad/key".to_string(),
        too_long,
    ] {
        let source = format!("example.test req.header(x-value: {value})");
        let errors = RuleSet::parse("default", &source).expect_err("invalid key must fail");
        assert_eq!(errors[0].code, RuleErrorCode::Action);
        assert!(errors[0].message.contains("invalid value key"));
    }

    let errors =
        RuleSet::parse("default", "example.test tag(<>)").expect_err("empty file path must fail");
    assert_eq!(errors[0].code, RuleErrorCode::Action);
    assert!(
        errors[0]
            .message
            .contains("file value path must not be empty")
    );
}

#[test]
fn public_value_key_contract_has_explicit_boundaries() {
    for key in ["a", "A-Z_09.value", "feature.flag-v2"] {
        assert!(rsproxy_rules::valid_value_key(key), "{key}");
    }
    for key in ["", "../escape", "bad/key", "with space", "unicode-值"] {
        assert!(!rsproxy_rules::valid_value_key(key), "{key}");
    }
    assert!(rsproxy_rules::valid_value_key(&"a".repeat(128)));
    assert!(!rsproxy_rules::valid_value_key(&"a".repeat(129)));
}
