use super::super::*;

#[test]
fn host_accepts_an_ordered_address_pool() {
    let rules = RuleSet::parse(
        "default",
        "example.test host(127.0.0.1:18081, 127.0.0.1:18082, backup.test:18083)",
    )
    .unwrap();

    let Action::Host(pool) = &rules.rules[0].actions[0] else {
        panic!("expected host action");
    };
    assert_eq!(
        pool.addresses(),
        [
            Value::inline("127.0.0.1:18081"),
            Value::inline("127.0.0.1:18082"),
            Value::inline("backup.test:18083")
        ]
    );
    assert_eq!(
        pool.clone().selected_address(),
        &Value::inline("127.0.0.1:18081")
    );
    assert_eq!(
        pool.clone().selected_address(),
        &Value::inline("127.0.0.1:18082")
    );
    assert_eq!(
        pool.clone().selected_address(),
        &Value::inline("backup.test:18083")
    );
    assert_eq!(
        pool.clone().selected_address(),
        &Value::inline("127.0.0.1:18081")
    );
}

#[test]
fn separate_host_actions_have_independent_cursors() {
    let first = RuleSet::parse("first", "one.test host(a.test:80, b.test:80)").unwrap();
    let second = RuleSet::parse("second", "two.test host(a.test:80, b.test:80)").unwrap();
    let Action::Host(first) = &first.rules[0].actions[0] else {
        panic!("expected first host action");
    };
    let Action::Host(second) = &second.rules[0].actions[0] else {
        panic!("expected second host action");
    };

    assert_eq!(
        first.clone().selected_address(),
        &Value::inline("a.test:80")
    );
    assert_eq!(
        first.clone().selected_address(),
        &Value::inline("b.test:80")
    );
    assert_eq!(
        second.clone().selected_address(),
        &Value::inline("a.test:80")
    );
}

#[test]
fn host_rejects_missing_or_empty_addresses() {
    for source in [
        "example.test host()",
        "example.test host(one.test,,two.test)",
    ] {
        let errors = RuleSet::parse("default", source).expect_err("host address must be present");
        let error = &errors[0];
        assert_eq!(error.code, RuleErrorCode::Action);
    }
}

#[test]
fn map_remote_parses_aliases_and_resolves_as_single_family() {
    for source in [
        "example.test map.remote(http://127.0.0.1:3000)",
        "example.test mapRemote(http://127.0.0.1:3000)",
        "example.test map_remote(http://127.0.0.1:3000)",
        "example.test map-remote(http://127.0.0.1:3000)",
    ] {
        let rules = RuleSet::parse("default", source).unwrap();
        let Action::MapRemote(value) = &rules.rules[0].actions[0] else {
            panic!("expected map.remote action for `{source}`");
        };
        assert_eq!(value, &Value::inline("http://127.0.0.1:3000"));
        assert_eq!(rules.rules[0].actions[0].family(), "map.remote");
    }
}

#[test]
fn map_remote_keeps_only_the_first_match() {
    let rules = RuleSet::parse(
        "default",
        "example.test map.remote(http://127.0.0.1:3000)\nexample.test map.remote(http://127.0.0.1:4000)",
    )
    .unwrap();
    let result = rules.resolve(&req("http://example.test/app"));
    let map_remote_actions = result
        .actions
        .iter()
        .filter(|item| matches!(item.action, Action::MapRemote(_)))
        .count();
    assert_eq!(map_remote_actions, 1);
}

#[test]
fn map_remote_rejects_literal_targets_without_http_scheme() {
    for source in [
        "example.test map.remote(localhost:3000)",
        "example.test map.remote(socks5://127.0.0.1:1080)",
    ] {
        let errors =
            RuleSet::parse("default", source).expect_err("non-http literal target must fail");
        assert_eq!(errors[0].code, RuleErrorCode::Action);
    }
    // Templated, file, and reference targets defer validation to execution.
    for source in [
        "example.test map.remote(${reqH.x-target})",
        "example.test map.remote(@target)",
        "example.test map.remote(<target.txt>)",
    ] {
        RuleSet::parse("default", source).unwrap();
    }
}

#[test]
fn mock_inline_form_parses_status_headers_and_body() {
    let rules = RuleSet::parse(
        "default",
        r#"example.test mock(status=503, type=application/json, header=X-Mock: yes, body={"ok":false})"#,
    )
    .unwrap();
    let Action::MockInline(op) = &rules.rules[0].actions[0] else {
        panic!("expected inline mock action");
    };
    assert_eq!(op.status, Some(503));
    assert_eq!(
        op.headers,
        vec![
            (
                "Content-Type".to_string(),
                Value::inline("application/json")
            ),
            ("X-Mock".to_string(), Value::inline("yes")),
        ]
    );
    assert_eq!(op.body, Some(Value::inline(r#"{"ok":false}"#)));
    assert_eq!(rules.rules[0].actions[0].family(), "mock");
}

#[test]
fn mock_single_argument_form_is_unchanged() {
    let rules = RuleSet::parse("default", "example.test mock(\"a=b\")").unwrap();
    assert!(matches!(&rules.rules[0].actions[0], Action::Mock(_)));
    let rules = RuleSet::parse("default", "example.test mock(<mocks/a.json>)").unwrap();
    assert!(matches!(&rules.rules[0].actions[0], Action::Mock(_)));
}

#[test]
fn mock_inline_rejects_unknown_keys_and_bad_status() {
    for source in [
        "example.test mock(status=abc)",
        "example.test mock(status=99)",
        "example.test mock(status=200, weird=1)",
        "example.test mock(status=200, header=NoColon)",
    ] {
        let errors = RuleSet::parse("default", source).expect_err("invalid inline mock");
        assert_eq!(errors[0].code, RuleErrorCode::Action);
    }
}

#[test]
fn whistle_operator_tokens_get_migration_hints() {
    for (source, expected) in [
        (
            "example.test socks://127.0.0.1:1080",
            "upstream(socks5://127.0.0.1:1080)",
        ),
        (
            "example.test proxy://127.0.0.1:8888",
            "upstream(proxy://127.0.0.1:8888)",
        ),
        (
            "example.test http://localhost:3000",
            "map.remote(http://localhost:3000)",
        ),
        ("example.test localhost:3000", "host(localhost:3000)"),
        ("example.test $0", "direct skip()"),
    ] {
        let errors = RuleSet::parse("default", source).expect_err("whistle token must fail");
        assert_eq!(errors[0].code, RuleErrorCode::Action, "{source}");
        assert!(
            errors[0].message.contains(expected),
            "`{source}` hint should mention `{expected}`; got: {}",
            errors[0].message
        );
    }
}
