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
    assert_eq!(stats.compiled_globs, 0);
    assert_eq!(stats.compiled_body_literals, 0);
}

#[test]
fn snapshot_compiles_deduplicated_case_insensitive_body_literals() {
    let rules = RuleSet::parse(
        "compiled",
        concat!(
            "example.test req.header(x-a: yes) when body(~ Alpha)\n",
            "example.test req.header(x-b: yes) when any(body(~ alpha), body(~ beta))\n",
            "example.test req.header(x-c: yes) when not(body(~ missing))"
        ),
    )
    .unwrap();
    assert_eq!(rules.stats().compiled_body_literals, 3);

    let mut request = req("http://example.test/");
    request.body = b"ALPHA and BETA".to_vec();
    let result = rules.resolve(&request);
    assert_eq!(result.actions.len(), 3);
}

#[test]
fn snapshot_compiles_and_deduplicates_every_runtime_glob_program() {
    let rules = RuleSet::parse(
        "compiled",
        "api*.example.test/path/**?mode=* status(204) when host(API*.EXAMPLE.TEST)",
    )
    .unwrap();
    assert_eq!(rules.stats().compiled_globs, 3);

    let result = rules.resolve(&req("http://api1.example.test/path/child?mode=debug"));
    assert_eq!(result.actions.len(), 1);
    assert_eq!(result.actions[0].action, Action::Status(204));
}

#[test]
fn snapshot_uses_validated_escape_semantics_without_a_wildcard() {
    let rules = RuleSet::parse(
        "compiled",
        r"api\.example.test/literal\. status(204) when url(http://api\.example.test/literal\.) when clientIp(192\.0\.2\.1)",
    )
    .unwrap();
    assert_eq!(rules.stats().compiled_globs, 4);

    let mut request = req("http://api.example.test/literal.");
    request.client_ip = Some("192.0.2.1".to_string());
    assert_eq!(rules.resolve(&request).actions.len(), 1);
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
fn regex_prefilter_keeps_rules_with_overlapping_required_literals() {
    let root_patterns = [
        r"/^https?:\/\/h\.example\.com\/?$/",
        r"/^https?:\/\/h\.example\.com$/",
        r"/^https?:\/\/h\.example\.com\/*$/",
    ];
    let asset_rule = "@language 3\n/^https?:\\/\\/h\\.example\\.com\\/(.+)\\.js$/ direct";

    for root_pattern in root_patterns {
        let root_rule = format!("@language 3\n{root_pattern} direct");
        for groups in [
            [("root", root_rule.as_str()), ("asset", asset_rule)],
            [("asset", asset_rule), ("root", root_rule.as_str())],
        ] {
            let rules = RuleSet::parse_versioned_groups(groups).unwrap();
            let request = req("https://h.example.com/a.js");
            let url = UrlParts::parse(&request.url).unwrap();
            let candidates = rules.candidate_rule_indices(Some(&url), &request.url);

            assert_eq!(rules.stats().prefilter_rules, 2);
            assert_eq!(candidates.len(), 2, "root pattern: {root_pattern}");

            let result = rules.resolve(&request);
            assert_eq!(result.actions.len(), 1, "root pattern: {root_pattern}");
            assert_eq!(result.matched_rules[0].group.as_ref(), "asset");

            let root_result = rules.resolve(&req("https://h.example.com"));
            assert_eq!(root_result.actions.len(), 1, "root pattern: {root_pattern}");
            assert_eq!(root_result.matched_rules[0].group.as_ref(), "root");
        }
    }

    let partial_overlap = RuleSet::parse(
        "overlap",
        "/abc/ req.header(x-abc: yes)\n/bcd/ req.header(x-bcd: yes)",
    )
    .unwrap();
    let request = req("https://example.test/abcd");
    let url = UrlParts::parse(&request.url).unwrap();
    assert_eq!(
        partial_overlap
            .candidate_rule_indices(Some(&url), &request.url)
            .len(),
        2
    );
    assert_eq!(partial_overlap.resolve(&request).actions.len(), 2);
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
