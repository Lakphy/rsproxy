use super::*;

#[test]
fn lint_reports_specific_rule_shadowed_by_earlier_wildcard() {
    let rules = RuleSet::parse(
        "default",
        "*.foo.test upstream(socks5://127.0.0.1:1111)\napi.foo.test upstream(socks5://127.0.0.1:2222)",
    )
    .unwrap();
    let findings = rules.lint();
    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!((finding.group.as_str(), finding.line), ("default", 2));
    assert_eq!(
        (finding.shadowed_by_group.as_str(), finding.shadowed_by_line),
        ("default", 1)
    );
    assert_eq!(finding.families, vec!["upstream".to_string()]);
}

#[test]
fn lint_reports_global_wildcard_shadowing_later_rules() {
    let rules = RuleSet::parse(
        "default",
        "* upstream(socks5://127.0.0.1:1111)\n**alibaba-inc** direct",
    )
    .unwrap();
    // `direct` and `upstream` are different families; no shadowing.
    assert!(rules.lint().is_empty());

    let rules = RuleSet::parse(
        "default",
        "* upstream(socks5://127.0.0.1:1111)\ninternal.test upstream(direct)",
    )
    .unwrap();
    let findings = rules.lint();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].line, 2);
}

#[test]
fn lint_accepts_specific_before_broad_order() {
    let rules = RuleSet::parse(
        "default",
        "api.foo.test upstream(socks5://127.0.0.1:2222)\n*.foo.test upstream(socks5://127.0.0.1:1111)",
    )
    .unwrap();
    assert!(rules.lint().is_empty());
}

#[test]
fn lint_skips_conditional_and_disabled_and_stackable_rules() {
    // A conditional broad rule does not always win, so no finding.
    let rules = RuleSet::parse(
        "default",
        "*.foo.test upstream(socks5://127.0.0.1:1111) when method(POST)\napi.foo.test upstream(socks5://127.0.0.1:2222)",
    )
    .unwrap();
    assert!(rules.lint().is_empty());

    // A disabled broad rule never participates in resolution.
    let rules = RuleSet::parse(
        "default",
        "*.foo.test upstream(socks5://127.0.0.1:1111) @disabled\napi.foo.test upstream(socks5://127.0.0.1:2222)",
    )
    .unwrap();
    assert!(rules.lint().is_empty());

    // Stackable families accumulate; both header rules apply.
    let rules = RuleSet::parse(
        "default",
        "*.foo.test req.header(x-a: 1)\napi.foo.test req.header(x-b: 2)",
    )
    .unwrap();
    assert!(rules.lint().is_empty());
}

#[test]
fn lint_honors_important_reordering() {
    // The later rule is @important, so it resolves first and is not shadowed.
    let rules = RuleSet::parse(
        "default",
        "*.foo.test upstream(socks5://127.0.0.1:1111)\napi.foo.test upstream(socks5://127.0.0.1:2222) @important",
    )
    .unwrap();
    assert!(rules.lint().is_empty());
}

#[test]
fn lint_respects_path_and_scheme_constraints() {
    // The broad rule is limited to a path the later rule does not share.
    let rules =
        RuleSet::parse("default", "foo.test/api status(503)\nfoo.test status(200)").unwrap();
    assert!(rules.lint().is_empty());

    // Same path prefix on both sides is a provable shadow.
    let rules = RuleSet::parse(
        "default",
        "*.foo.test/api status(503)\napi.foo.test/api status(200)",
    )
    .unwrap();
    assert_eq!(rules.lint().len(), 1);
}

#[test]
fn lint_spans_groups_in_group_order() {
    let rules = RuleSet::parse_groups([
        ("first", "* upstream(socks5://127.0.0.1:1111)"),
        ("second", "api.foo.test upstream(socks5://127.0.0.1:2222)"),
    ])
    .unwrap();
    let findings = rules.lint();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].group, "second");
    assert_eq!(findings[0].shadowed_by_group, "first");
}

#[test]
fn lint_reports_exact_url_covered_by_glob() {
    let rules = RuleSet::parse(
        "default",
        "foo.test status(503)\n=http://foo.test/health status(200)",
    )
    .unwrap();
    assert_eq!(rules.lint().len(), 1);
}
