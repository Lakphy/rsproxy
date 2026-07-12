use super::*;

#[test]
fn response_header_condition_matches_presence_and_contains() {
    let rules = RuleSet::parse(
            "default",
            "example.com res.header(x-res-condition: contains) when res.header(x-origin ~ hit)\nexample.com res.header(x-res-present: yes) when res.header(x-origin)\nexample.com cache(55) when res.header(x-origin ~ hit)\nexample.com cache(5)",
        )
        .unwrap();
    let request = req("http://example.com/");
    let hit = ResponseMeta {
        status: 200,
        headers: vec![("X-Origin".to_string(), "route-hit".to_string())],
    };
    let miss = ResponseMeta {
        status: 200,
        headers: vec![("X-Origin".to_string(), "route-miss".to_string())],
    };

    let result = rules.resolve_response(&request, &hit);
    assert_eq!(result.actions.len(), 3);
    assert!(matches!(
        result.actions[0].action,
        Action::ResHeader(HeaderOp::Set { .. })
    ));
    assert!(matches!(
        result.actions[1].action,
        Action::ResHeader(HeaderOp::Set { .. })
    ));
    assert!(matches!(
        result.actions[2].action,
        Action::Cache(CacheOp::Directives(ref directives))
            if directives.iter().any(|directive| {
                directive.name == "max-age"
                    && directive.value.as_ref().and_then(Value::as_inline) == Some("55")
            })
    ));

    let result = rules.resolve_response(&request, &miss);
    assert_eq!(result.actions.len(), 2);
    assert!(matches!(
        result.actions[0].action,
        Action::ResHeader(HeaderOp::Set { .. })
    ));
    assert!(matches!(
        result.actions[1].action,
        Action::Cache(CacheOp::Directives(ref directives))
            if directives.iter().any(|directive| {
                directive.name == "max-age"
                    && directive.value.as_ref().and_then(Value::as_inline) == Some("5")
            })
    ));
}

#[test]
fn status_condition_matches_only_with_response_meta() {
    let rules = RuleSet::parse(
            "default",
            "example.com res.header(x-status-hit: yes) cache(55) when status(404)\nexample.com res.header(x-status-any: yes) when status(200, 404)\nexample.com cache(5)",
        )
        .unwrap();
    let request = req("http://example.com/");

    let request_only = rules.resolve(&request);
    assert_eq!(request_only.actions.len(), 1);
    assert_eq!(request_only.matched_rules.len(), 1);
    assert_eq!(request_only.matched_rules[0].line, 3);
    assert!(matches!(
        request_only.actions[0].action,
        Action::Cache(CacheOp::Directives(ref directives))
            if directives.iter().any(|directive| {
                directive.name == "max-age"
                    && directive.value.as_ref().and_then(Value::as_inline) == Some("5")
            })
    ));

    let not_found = ResponseMeta {
        status: 404,
        headers: vec![],
    };
    let result = rules.resolve_response(&request, &not_found);
    assert_eq!(result.actions.len(), 3);
    assert_eq!(result.matched_rules.len(), 2);
    assert_eq!(result.matched_rules[0].line, 1);
    assert_eq!(result.matched_rules[1].line, 2);
    assert!(matches!(
        result.actions[0].action,
        Action::ResHeader(HeaderOp::Set { .. })
    ));
    assert!(matches!(
        result.actions[1].action,
        Action::Cache(CacheOp::Directives(ref directives))
            if directives.iter().any(|directive| {
                directive.name == "max-age"
                    && directive.value.as_ref().and_then(Value::as_inline) == Some("55")
            })
    ));
    assert!(matches!(
        result.actions[2].action,
        Action::ResHeader(HeaderOp::Set { .. })
    ));

    let ok = ResponseMeta {
        status: 200,
        headers: vec![],
    };
    let result = rules.resolve_response(&request, &ok);
    assert_eq!(result.actions.len(), 2);
    assert_eq!(result.matched_rules.len(), 2);
    assert_eq!(result.matched_rules[0].line, 2);
    assert_eq!(result.matched_rules[1].line, 3);
    assert!(matches!(
        result.actions[0].action,
        Action::ResHeader(HeaderOp::Set { .. })
    ));
    assert!(matches!(
        result.actions[1].action,
        Action::Cache(CacheOp::Directives(ref directives))
            if directives.iter().any(|directive| {
                directive.name == "max-age"
                    && directive.value.as_ref().and_then(Value::as_inline) == Some("5")
            })
    ));
}
