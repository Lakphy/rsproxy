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
