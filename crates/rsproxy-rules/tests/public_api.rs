//! Behavioral smoke tests for the rules facade.
#![allow(clippy::unwrap_used)]

use rsproxy_rules::{
    Action, DeleteBodyPath, HostPool, RequestMeta, RuleModelError, RuleSet, UrlParts, Value,
};

#[test]
fn public_rules_api_parses_and_resolves_a_request() {
    let rules =
        RuleSet::parse("integration", "api.example.test status(201)").expect("rule should parse");
    assert!(!rules.is_empty());
    assert_eq!(rules.rules().len(), 1);
    assert!(rules.version() > 0);
    let request = RequestMeta {
        method: "GET".to_string(),
        url: "http://api.example.test/items".to_string(),
        headers: Vec::new(),
        body: Vec::new(),
        client_ip: None,
        server_ip: None,
        template: Default::default(),
    };

    let result = rules.resolve(&request);
    assert_eq!(result.matched_rules.len(), 1);
    assert!(matches!(result.actions[0].action, Action::Status(201)));
}

#[test]
fn public_model_constructors_use_rule_model_error() {
    let url: Result<UrlParts, RuleModelError> = UrlParts::parse("missing-scheme");
    assert!(matches!(
        url,
        Err(RuleModelError::InvalidSyntax { context: "URL", .. })
    ));

    let host_pool: Result<HostPool, RuleModelError> = HostPool::new(Vec::<Value>::new());
    assert!(matches!(
        host_pool,
        Err(RuleModelError::EmptyInput {
            context: "host addresses",
            ..
        })
    ));

    let body_path: Result<DeleteBodyPath, RuleModelError> = DeleteBodyPath::new(Vec::new());
    assert!(matches!(
        body_path,
        Err(RuleModelError::EmptyInput {
            context: "delete body path",
            ..
        })
    ));
}

#[test]
fn public_lint_report_exposes_bounded_completeness_metadata() {
    let rules = RuleSet::parse(
        "integration",
        "*.example.test status(503)\napi.example.test status(200)",
    )
    .unwrap();
    let report = rules.lint_report();
    assert!(report.complete);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.comparisons, 1);
    assert!(report.comparison_bytes > 0);
    assert_eq!(rsproxy_rules::MAX_RULE_LINT_COMPARISONS, 1_000_000);
    assert_eq!(rsproxy_rules::MAX_RULE_LINT_COMPARISON_BYTES, 268_435_456);
}
