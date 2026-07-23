use super::super::*;

#[test]
fn header_regex_replacement_parses_and_replaces_capture_groups() {
    let rules = RuleSet::parse(
        "default",
        r"example.test req.header(x-release ~ /v(\d+)/release-$1)",
    )
    .unwrap();

    let Action::ReqHeader(HeaderOp::Replace {
        name,
        pattern,
        replacement,
    }) = &rules.rules()[0].actions[0]
    else {
        panic!("expected request header replacement");
    };
    assert_eq!(name, "x-release");
    assert_eq!(pattern.pattern(), r"v(\d+)");
    assert_eq!(replacement, "release-$1");
    assert_eq!(pattern.replace_all("v42", replacement), "release-42");
}

#[test]
fn header_regex_replacement_supports_escaped_slashes() {
    let rules = RuleSet::parse(
        "default",
        r"example.test res.header(location ~ /http:\/\/old/http:\/\/new)",
    )
    .unwrap();

    let Action::ResHeader(HeaderOp::Replace {
        pattern,
        replacement,
        ..
    }) = &rules.rules()[0].actions[0]
    else {
        panic!("expected response header replacement");
    };
    assert_eq!(pattern.pattern(), "http://old");
    assert_eq!(replacement, "http://new");
    assert_eq!(
        pattern.replace_all("http://old/path", replacement),
        "http://new/path"
    );
}

#[test]
fn header_set_value_may_contain_a_tilde() {
    let rules = RuleSet::parse("default", "example.test req.header(x-note: a~b)").unwrap();
    assert!(matches!(
        &rules.rules()[0].actions[0],
        Action::ReqHeader(HeaderOp::Set { name, value })
            if name == "x-note" && value.as_inline() == Some("a~b")
    ));
}

#[test]
fn header_operations_preserve_commas_inside_the_single_operation() {
    let rules = RuleSet::parse(
        "default",
        r"example.test req.header(x-list ~ /item, (\d+)/entry-$1)",
    )
    .unwrap();
    let Action::ReqHeader(HeaderOp::Replace {
        pattern,
        replacement,
        ..
    }) = &rules.rules()[0].actions[0]
    else {
        panic!("expected request header replacement");
    };

    assert_eq!(pattern.replace_all("item, 9", replacement), "entry-9");
}

#[test]
fn invalid_header_replacement_regex_is_an_action_error() {
    let errors = RuleSet::parse("default", "example.test req.header(x-id ~ /[/value)")
        .expect_err("invalid regex should fail while parsing the rule");
    let error = &errors[0];
    assert_eq!(error.code, RuleErrorCode::Action);
    assert!(error.message.contains("unclosed `[`"));
}

#[test]
fn header_operations_reject_names_that_are_not_http_tokens() {
    for action in [
        r#"req.header("x-debug: yes")"#,
        "res.header(bad name: yes)",
        r#"req.header(-"x-debug")"#,
        r#"res.header("x-debug" ~ /old/new)"#,
        "res.trailer(bad name: yes)",
    ] {
        let source = format!("example.test {action}");
        let errors = RuleSet::parse("default", &source)
            .expect_err("invalid header names must fail while parsing the rule");
        let error = &errors[0];
        assert_eq!(error.code, RuleErrorCode::Action, "{action}");
        assert!(
            error.message.contains("invalid header name")
                && error.message.contains("unquoted HTTP header name"),
            "{action}: {}",
            error.message
        );
    }
}
