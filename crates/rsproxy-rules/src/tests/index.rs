use super::*;

#[test]
fn ruleset_stats_reports_domain_index_and_prefilter() {
    let rules = RuleSet::parse(
        "default",
        r#"
            api.example.com res.header(x-api: yes)
            **.example.com res.header(x-suffix: yes)
            =http://exact.test/a status(204)
            /health-check/ req.header(x-prefilter: yes)
            * res.header(x-global: yes)
            "#,
    )
    .unwrap();

    let stats = rules.stats();
    assert_eq!(stats.rules, 5);
    assert_eq!(stats.domain_exact_entries, 2);
    assert_eq!(stats.domain_suffix_entries, 1);
    assert_eq!(stats.indexed_rules, 3);
    assert_eq!(stats.global_rules, 1);
    assert_eq!(stats.prefilter_literals, 1);
    assert_eq!(stats.prefilter_rules, 1);
}

#[test]
fn domain_index_reduces_candidates_without_changing_order() {
    let rules = RuleSet::parse(
        "default",
        r#"
            other.example.net res.header(x-other: no)
            * res.header(x-global: first)
            **.example.com res.header(x-suffix: second)
            api.example.com @important res.header(x-api: important)
            "#,
    )
    .unwrap();
    let request = req("http://api.example.com/path");
    let url = UrlParts::parse(&request.url).unwrap();
    let candidates = rules.candidate_rule_indices(Some(&url), &request.url);

    assert_eq!(candidates.len(), 3);
    assert!(!candidates.contains(&0));

    let result = rules.resolve(&request);
    let headers = result
        .actions
        .iter()
        .filter_map(|item| match &item.action {
            Action::ResHeader(HeaderOp::Set { name, value }) => {
                Some((name.as_str(), value.as_inline().unwrap()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        headers,
        vec![
            ("x-api", "important"),
            ("x-global", "first"),
            ("x-suffix", "second")
        ]
    );
}

#[test]
fn regex_prefilter_skips_only_when_required_literal_is_missing() {
    let rules = RuleSet::parse(
        "default",
        r#"
            /health-check/ req.header(x-health: yes)
            /\/users\/(\d+)/ req.header(x-user: $1)
            "#,
    )
    .unwrap();
    assert_eq!(rules.stats().prefilter_literals, 2);

    let miss = rules.resolve(&req("http://example.com/other"));
    assert!(miss.actions.is_empty());

    let health = rules.resolve(&req("http://example.com/health-check"));
    assert_eq!(health.actions.len(), 1);
    assert!(matches!(health.actions[0].action, Action::ReqHeader(_)));

    let user = rules.resolve(&req("http://example.com/users/42"));
    assert_eq!(user.actions.len(), 1);
    assert!(matches!(user.actions[0].action, Action::ReqHeader(_)));
}

#[test]
fn aho_prefilter_maps_one_literal_to_multiple_rules() {
    let rules = RuleSet::parse(
        "default",
        r#"
            /foo(shared-literal)bar/ req.header(x-a: yes)
            /baz(shared-literal)qux/ req.header(x-b: yes)
            /not-present/ req.header(x-miss: no)
            "#,
    )
    .unwrap();
    let stats = rules.stats();
    assert_eq!(stats.global_rules, 0);
    assert_eq!(stats.prefilter_literals, 2);
    assert_eq!(stats.prefilter_rules, 3);

    let result = rules.resolve(&req(
        "http://example.com/fooshared-literalbar/bazshared-literalqux",
    ));
    assert_eq!(result.actions.len(), 2);
    assert_eq!(result.matched_rules[0].line, 2);
    assert_eq!(result.matched_rules[1].line, 3);
}

#[test]
fn regex_prefilter_handles_required_literals_around_classes_and_repetition() {
    let rules = RuleSet::parse(
        "default",
        r#"
            /^http:\/\/bench-42\.example\.test\/api\/[0-9]+$/ status(200)
            /optional(?:-[a-z]+)?/ status(201)
            /CASE-SENSITIVE/i status(202)
            /left|right/ status(203)
            "#,
    )
    .unwrap();

    let stats = rules.stats();
    assert_eq!(stats.prefilter_rules, 2);
    assert_eq!(stats.global_rules, 2);
    assert!(matches!(
        rules
            .resolve(&req("http://bench-42.example.test/api/123"))
            .actions[0]
            .action,
        Action::Status(200)
    ));
    assert!(matches!(
        rules
            .resolve(&req("http://example.test/case-sensitive"))
            .actions[0]
            .action,
        Action::Status(202)
    ));
}
