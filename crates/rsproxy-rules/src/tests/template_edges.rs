use super::*;

#[test]
fn resolved_action_constructor_and_response_snapshot_are_observable() {
    let action = ResolvedAction::new(
        Action::Status(200),
        MatchedRule {
            group: "template".into(),
            line: 1,
            raw: "fixture".into(),
        },
        Captures::default(),
    );
    assert_eq!(action.response_meta(), None);
    assert_eq!(action.render("literal", &req("not-a-url")), "literal");

    let rules = RuleSet::parse(
        "template",
        "example.test res.header(x-status: ${statusCode})",
    )
    .unwrap();
    let response = ResponseMeta {
        status: 207,
        headers: Vec::new(),
    };
    let result = rules.resolve_response(&req("http://example.test"), &response);
    assert_eq!(result.actions[0].response_meta(), Some(&response));
}

#[test]
fn template_render_handles_invalid_urls_dangling_tokens_and_runtime_regex_errors() {
    let captures = Captures::default();
    let request = req("not-a-url");
    assert_eq!(
        captures.render("${host}|${port}|${path}|${query}", &request),
        "|||"
    );
    assert_eq!(captures.render("cost=$", &request), "cost=$");
    assert_eq!(captures.render("$9", &request), "");
    assert_eq!(captures.render("${url.replace(/[/, x)}", &request), "");
}

#[test]
fn template_regex_cache_hit_and_lru_eviction_preserve_results() {
    let captures = Captures::default();
    let request = req("http://example.test/path");
    let expression = "${url.replace(/example/, service)}";
    let expected = "http://service.test/path";
    assert_eq!(captures.render(expression, &request), expected);
    assert_eq!(captures.render(expression, &request), expected);

    for index in 0..129 {
        let expression = format!("${{url.replace(/never-{index}/, value)}}");
        assert_eq!(captures.render(&expression, &request), request.url);
    }
    assert_eq!(captures.render(expression, &request), expected);
}

#[test]
fn bounded_template_render_rejects_capture_and_regex_expansion_before_allocation() {
    let captures = Captures::default();
    let request = req("aaaa");
    let expression = "${url.replace(/a/, xx)}";
    assert_eq!(
        captures.render_bounded(expression, &request, 8).unwrap(),
        "xxxxxxxx"
    );
    let error = captures
        .render_bounded(expression, &request, 7)
        .unwrap_err();
    assert!(matches!(error, RuleModelError::LimitExceeded { .. }));

    let action = ResolvedAction::new(
        Action::Tag(Value::inline("$0$0")),
        MatchedRule {
            group: "template".into(),
            line: 1,
            raw: "fixture".into(),
        },
        Captures {
            whole: Some("abcd".into()),
            ..Captures::default()
        },
    );
    assert_eq!(
        action.render_bounded("$0$0", &request, 8).unwrap(),
        "abcdabcd"
    );
    assert!(action.render_bounded("$0$0", &request, 7).is_err());
}

#[test]
fn human_explanations_use_bounded_template_rendering() {
    let rules = RuleSet::parse("explain", "* tag(${url}${url})").unwrap();
    let request = req(&format!("http://example.test/{}", "x".repeat(3000)));
    let explanation = rules.explain(&request);
    assert!(explanation.contains("<render-limit:4096>"));
    assert!(explanation.len() <= MAX_RULE_EXPLAIN_BYTES);
}

#[test]
fn redaction_and_path_constructors_keep_non_secret_inputs_unchanged() {
    assert_eq!(
        redact_secrets("socks5://proxy.test:1080"),
        "socks5://proxy.test:1080"
    );
    assert_eq!(
        redact_secrets("socks5://user/path@proxy.test:1080"),
        "socks5://user/path@proxy.test:1080"
    );
    assert!(DeleteBodyPath::new(Vec::new()).is_err());
}
