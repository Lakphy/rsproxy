use super::*;
use std::collections::BTreeSet;

fn repeated_rule(prefix: &str, token: &str, count: usize) -> String {
    let mut source = String::from(prefix);
    for _ in 0..count {
        source.push(' ');
        source.push_str(token);
    }
    source
}

fn repeated_snapshot(prefix: &str, token: &str, total: usize, per_line: usize) -> String {
    let mut source = String::new();
    let mut remaining = total;
    while remaining > 0 {
        let count = remaining.min(per_line);
        source.push_str(&repeated_rule(prefix, token, count));
        source.push('\n');
        remaining -= count;
    }
    source
}

#[test]
fn snapshot_source_group_and_diagnostic_limits_are_exact() {
    let comment_line = format!("#{}\n", "x".repeat(MAX_RULE_SOURCE_LINE_BYTES - 2));
    assert_eq!(comment_line.len(), MAX_RULE_SOURCE_LINE_BYTES);
    let mut source =
        comment_line.repeat(MAX_RULE_SNAPSHOT_SOURCE_BYTES / MAX_RULE_SOURCE_LINE_BYTES);
    assert_eq!(source.len(), MAX_RULE_SNAPSHOT_SOURCE_BYTES);
    assert!(RuleSet::parse("limits", &source).unwrap().is_empty());
    source.push('x');
    let errors = RuleSet::parse("limits", &source).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("snapshot source exceeds"));

    let valid_name = "g".repeat(MAX_RULE_GROUP_NAME_BYTES);
    assert!(RuleSet::parse(&valid_name, "example.test hide").is_ok());
    let invalid_name = "g".repeat(MAX_RULE_GROUP_NAME_BYTES + 1);
    let errors = RuleSet::parse(&invalid_name, "example.test hide").unwrap_err();
    assert_eq!(errors[0].group, "<invalid-group>");
    assert!(errors[0].message.contains("group name"));

    let groups = (0..MAX_RULE_GROUPS_PER_SNAPSHOT)
        .map(|index| (format!("g{index}"), String::new()))
        .collect::<Vec<_>>();
    assert!(RuleSet::parse_groups(groups.iter().map(|(name, text)| (name, text))).is_ok());
    let mut over_limit = groups;
    over_limit.push(("overflow".to_string(), String::new()));
    let errors =
        RuleSet::parse_groups(over_limit.iter().map(|(name, text)| (name, text))).unwrap_err();
    assert!(errors[0].message.contains("group limit"));

    let invalid = "example.test unknown\n".repeat(MAX_RULE_DIAGNOSTICS + 20);
    let errors = RuleSet::parse("limits", &invalid).unwrap_err();
    assert_eq!(errors.len(), MAX_RULE_DIAGNOSTICS);
    assert!(errors.last().unwrap().message.contains("remaining source"));
}

#[test]
fn snapshot_rule_limit_rejects_the_first_rule_past_the_benchmarked_boundary() {
    let source = "* hide\n".repeat(MAX_RULES_PER_SNAPSHOT + 1);
    let errors = RuleSet::parse("rules", &source).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].line, MAX_RULES_PER_SNAPSHOT + 1);
    assert!(errors[0].message.contains("rule limit"));
}

#[test]
fn snapshot_action_and_condition_node_limits_are_exact() {
    let actions = repeated_snapshot(
        "example.test",
        "hide",
        MAX_RULE_ACTIONS_PER_SNAPSHOT,
        MAX_RULE_ACTIONS_PER_RULE,
    );
    assert!(RuleSet::parse("actions", &actions).is_ok());
    let actions = repeated_snapshot(
        "example.test",
        "hide",
        MAX_RULE_ACTIONS_PER_SNAPSHOT + 1,
        MAX_RULE_ACTIONS_PER_RULE,
    );
    let error = RuleSet::parse("actions", &actions).unwrap_err().remove(0);
    assert_eq!(error.code, RuleErrorCode::Action);
    assert!(error.message.contains("snapshot"));
    assert!(error.message.contains("action limit"));

    let conditions = repeated_snapshot(
        "example.test hide",
        "when method(GET)",
        MAX_RULE_CONDITION_NODES_PER_SNAPSHOT,
        MAX_RULE_CONDITION_NODES_PER_RULE,
    );
    assert!(RuleSet::parse("conditions", &conditions).is_ok());
    let conditions = repeated_snapshot(
        "example.test hide",
        "when method(GET)",
        MAX_RULE_CONDITION_NODES_PER_SNAPSHOT + 1,
        MAX_RULE_CONDITION_NODES_PER_RULE,
    );
    let error = RuleSet::parse("conditions", &conditions)
        .unwrap_err()
        .remove(0);
    assert_eq!(error.code, RuleErrorCode::Condition);
    assert!(error.message.contains("snapshot"));
    assert!(error.message.contains("condition-node limit"));

    let body_conditions = repeated_snapshot(
        "example.test hide",
        "when body(~x)",
        MAX_RULE_BODY_CONDITIONS_PER_SNAPSHOT,
        128,
    );
    assert!(RuleSet::parse("body-conditions", &body_conditions).is_ok());
    let body_conditions = repeated_snapshot(
        "example.test hide",
        "when body(~x)",
        MAX_RULE_BODY_CONDITIONS_PER_SNAPSHOT + 1,
        128,
    );
    let error = RuleSet::parse("body-conditions", &body_conditions)
        .unwrap_err()
        .remove(0);
    assert_eq!(error.code, RuleErrorCode::Condition);
    assert!(error.message.contains("body-condition limit"));
}

#[test]
fn per_rule_action_condition_property_and_argument_limits_are_exact() {
    let actions = repeated_rule("example.test", "hide", MAX_RULE_ACTIONS_PER_RULE);
    assert_eq!(
        RuleSet::parse("actions", &actions).unwrap().rules()[0]
            .actions
            .len(),
        MAX_RULE_ACTIONS_PER_RULE
    );
    let actions = repeated_rule("example.test", "hide", MAX_RULE_ACTIONS_PER_RULE + 1);
    let error = RuleSet::parse("actions", &actions).unwrap_err().remove(0);
    assert_eq!(error.code, RuleErrorCode::Action);
    assert!(error.message.contains("action limit"));

    let conditions = repeated_rule(
        "example.test hide",
        "when method(GET)",
        MAX_RULE_CONDITION_NODES_PER_RULE,
    );
    assert_eq!(
        RuleSet::parse("conditions", &conditions).unwrap().rules()[0]
            .conditions
            .len(),
        MAX_RULE_CONDITION_NODES_PER_RULE
    );
    let conditions = repeated_rule(
        "example.test hide",
        "when method(GET)",
        MAX_RULE_CONDITION_NODES_PER_RULE + 1,
    );
    let error = RuleSet::parse("conditions", &conditions)
        .unwrap_err()
        .remove(0);
    assert_eq!(error.code, RuleErrorCode::Condition);
    assert!(error.message.contains("condition-node limit"));

    let properties = repeated_rule(
        "example.test hide",
        "@important",
        MAX_RULE_PROPERTIES_PER_RULE,
    );
    assert!(RuleSet::parse("properties", &properties).is_ok());
    let properties = repeated_rule(
        "example.test hide",
        "@important",
        MAX_RULE_PROPERTIES_PER_RULE + 1,
    );
    let error = RuleSet::parse("properties", &properties)
        .unwrap_err()
        .remove(0);
    assert_eq!(error.code, RuleErrorCode::Property);
    assert!(error.message.contains("property limit"));

    let arguments = std::iter::repeat_n("tag", MAX_RULE_CALL_ARGUMENTS).collect::<Vec<_>>();
    let source = format!("example.test skip({})", arguments.join(","));
    assert_eq!(
        RuleSet::parse("arguments", &source).unwrap().rules()[0].actions[0],
        Action::Skip([ActionFamily::Tag].into_iter().collect())
    );
    let source = format!("example.test skip({},tag)", arguments.join(","));
    let error = RuleSet::parse("arguments", &source).unwrap_err().remove(0);
    assert_eq!(error.code, RuleErrorCode::Action);
    assert!(error.message.contains("argument limit"));
}

#[test]
fn snapshot_versions_are_clone_stable_process_monotonic_and_concurrent_unique() {
    let first = RuleSet::empty();
    assert_eq!(first.version(), first.clone().version());
    let second = RuleSet::parse("version", "example.test hide").unwrap();
    assert!(second.version() > first.version());

    let handles = (0..32)
        .map(|_| {
            std::thread::spawn(|| {
                (0..64)
                    .map(|_| RuleSet::empty().version())
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    let versions = handles
        .into_iter()
        .flat_map(|handle| handle.join().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(versions.len(), 32 * 64);
}
