use super::*;

#[test]
fn semantic_lint_reports_duplicate_single_families_deterministically() {
    let rules = RuleSet::parse(
        "semantic",
        "example.test status(201) status(202) upstream(proxy://one) upstream(proxy://two)",
    )
    .unwrap();
    let findings = rules.semantic_lint();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, SemanticLintKind::DuplicateSingleFamily);
    assert_eq!(findings[0].families, ["status", "upstream"]);
}

#[test]
fn semantic_lint_report_marks_the_byte_budget_as_incomplete() {
    let padding = "x".repeat(60_000);
    let source = (0..72)
        .map(|index| format!("host-{index}.test status(201) status(202) tag(\"{padding}\")"))
        .collect::<Vec<_>>()
        .join("\n");
    let rules = RuleSet::parse("budget", &source).unwrap();
    let report = rules.semantic_lint_report();
    assert!(!report.complete);
    assert!(report.findings.len() < 72);
    assert!(report.findings.len() <= MAX_RULE_LINT_FINDINGS);
}

#[test]
fn semantic_lint_reports_only_provably_unsatisfiable_conjunctions() {
    for source in [
        "example.test status(200) when method(GET) when all(method(POST), header(x))",
        "example.test res.status(200) when status(200, 201) when status(500)",
        "example.test status(200) when env(MODE=one) when env(MODE=two)",
        "example.test res.status(200) when method(GET, POST) when !method(GET, POST)",
        "example.test res.status(200) when status(200, 404) when !status(200, 404)",
        "example.test res.status(200) when env(MODE=one) when !env(MODE=one)",
        "example.test res.status(200) when env(MODE) when !env(MODE)",
        "example.test res.status(200) when chance(0)",
        "example.test res.status(200) when not(any(chance(1), method(GET)))",
    ] {
        let rules = RuleSet::parse("semantic", source).unwrap();
        let findings = rules.semantic_lint();
        assert_eq!(findings.len(), 1, "{source}");
        assert_eq!(findings[0].kind, SemanticLintKind::UnsatisfiableConditions);
    }

    let valid = RuleSet::parse(
        "semantic",
        "example.test res.status(200) when method(GET, POST) when method(POST) when any(status(200), status(500))",
    )
    .unwrap();
    assert!(valid.semantic_lint().is_empty());
}

#[test]
fn semantic_lint_ignores_disabled_rules() {
    let rules =
        RuleSet::parse("semantic", "example.test status(201) status(202) @disabled").unwrap();
    assert!(rules.semantic_lint().is_empty());
}

#[test]
fn semantic_lint_reports_request_actions_that_require_response_metadata() {
    let rules = RuleSet::parse(
        "semantic",
        concat!(
            "example.test req.header(x: y) direct when all(method(GET), status(404))\n",
            "example.test req.method(POST) when !res.header(x-origin)\n",
            "example.test res.header(x: y) when status(404)\n",
            "example.test req.header(x: y) when any(method(GET), status(404))\n",
            "example.test delete(reqBody, resBody) when status(404)"
        ),
    )
    .unwrap();
    let findings = rules.semantic_lint();
    let phase_findings = findings
        .iter()
        .filter(|finding| finding.kind == SemanticLintKind::RequestActionRequiresResponse)
        .collect::<Vec<_>>();
    assert_eq!(phase_findings.len(), 2);
    assert_eq!(phase_findings[0].line, 1);
    assert_eq!(phase_findings[0].families, ["direct", "req.header"]);
    assert_eq!(phase_findings[1].line, 2);
    assert_eq!(phase_findings[1].families, ["req.method"]);
}

#[test]
fn semantic_lint_reports_provably_ineffective_action_combinations() {
    let rules = RuleSet::parse(
        "semantic",
        concat!(
            "example.test skip(res.body) res.body.append(x) req.header(x: y)\n",
            "example.test skip() tag(dead) req.header(x: y)\n",
            "example.test status(503) redirect(https://new.test) mock(body) res.header(x: y) cache(off) direct upstream(proxy://one)"
        ),
    )
    .unwrap();
    let findings = rules.semantic_lint();

    let skipped = findings
        .iter()
        .filter(|finding| finding.kind == SemanticLintKind::ActionAfterSkip)
        .collect::<Vec<_>>();
    assert_eq!(skipped.len(), 2);
    assert_eq!(skipped[0].families, ["res.body.append"]);
    assert_eq!(skipped[1].families, ["req.header", "tag"]);

    let terminal = findings
        .iter()
        .find(|finding| finding.kind == SemanticLintKind::ConflictingTerminalActions)
        .unwrap();
    assert_eq!(terminal.families, ["status", "redirect", "mock"]);

    let response = findings
        .iter()
        .find(|finding| finding.kind == SemanticLintKind::ResponseActionWithLocalResponse)
        .unwrap();
    assert_eq!(response.families, ["cache", "res.header"]);

    let route = findings
        .iter()
        .find(|finding| finding.kind == SemanticLintKind::UpstreamOverriddenByDirect)
        .unwrap();
    assert_eq!(route.families, ["upstream", "direct"]);

    let skipped_conflicts = RuleSet::parse(
        "semantic",
        "example.test skip(mock, upstream) mock(body) upstream(proxy://one) status(503) direct",
    )
    .unwrap();
    let findings = skipped_conflicts.semantic_lint();
    assert!(
        findings
            .iter()
            .all(|finding| finding.kind != SemanticLintKind::ConflictingTerminalActions)
    );
    assert!(
        findings
            .iter()
            .all(|finding| finding.kind != SemanticLintKind::UpstreamOverriddenByDirect)
    );
}

#[test]
fn semantic_lint_reports_body_actions_with_bodyless_status() {
    let rules = RuleSet::parse(
        "semantic",
        "example.test res.status(205) res.body.append(x) inject(html, y)",
    )
    .unwrap();

    let finding = rules
        .semantic_lint()
        .into_iter()
        .find(|finding| finding.kind == SemanticLintKind::BodyActionWithBodylessStatus)
        .unwrap();

    assert_eq!(
        finding.families,
        ["res.status", "inject", "res.body.append"]
    );
}
