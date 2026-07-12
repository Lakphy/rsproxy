use super::*;

#[test]
fn resolved_action_constructor_and_response_snapshot_are_observable() {
    let action = ResolvedAction::new(
        Action::Status(200),
        MatchedRule {
            group: "template".to_string(),
            line: 1,
            raw: "fixture".to_string(),
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
