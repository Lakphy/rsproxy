use super::*;

#[test]
fn group_order_precedes_line_order_across_compiled_indices() {
    let rules = RuleSet::parse_groups([
        ("first", "example.test status(201)"),
        ("second", "example.test status(202)"),
    ])
    .unwrap();

    let result = rules.resolve(&req("http://example.test/"));
    assert!(matches!(result.actions[0].action, Action::Status(201)));
    assert_eq!(result.actions[0].rule.group, "first");
}

#[test]
fn important_rule_precedes_earlier_groups() {
    let rules = RuleSet::parse_groups([
        ("first", "example.test status(201)"),
        ("second", "example.test status(202) @important"),
    ])
    .unwrap();

    let result = rules.resolve(&req("http://example.test/"));
    assert!(matches!(result.actions[0].action, Action::Status(202)));
    assert_eq!(result.actions[0].rule.group, "second");
}

#[test]
fn parse_errors_identify_the_source_group_and_line() {
    let errors = RuleSet::parse_groups([
        ("valid", "example.test status(200)"),
        ("broken", "# comment\nexample.test unknown()"),
    ])
    .unwrap_err();

    assert_eq!(errors[0].group, "broken");
    assert_eq!(errors[0].line, 2);
    assert_eq!(errors[0].code, RuleErrorCode::Action);
    assert_eq!(errors[0].to_string(), "broken:2: unknown action `unknown`");
}
