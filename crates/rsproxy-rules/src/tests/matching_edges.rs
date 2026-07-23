use super::*;
use crate::model::CompiledBodyContainsSet;

#[test]
fn url_parts_cover_authority_query_origin_and_port_boundaries() {
    let bare = UrlParts::parse("http://example.test").unwrap();
    assert_eq!(bare.path, "/");
    assert_eq!(bare.effective_port(), Some(80));
    assert_eq!(bare.origin_form(), "/");

    let query = UrlParts::parse("https://example.test?x=1").unwrap();
    assert_eq!(query.path, "/");
    assert_eq!(query.origin_form(), "/?x=1");
    assert_eq!(query.effective_port(), Some(443));
    assert_eq!(
        UrlParts::parse("wss://example.test")
            .unwrap()
            .effective_port(),
        Some(443)
    );
    assert_eq!(
        UrlParts::parse("ws://example.test")
            .unwrap()
            .effective_port(),
        Some(80)
    );
    assert_eq!(
        UrlParts::parse("tunnel://example.test:8443")
            .unwrap()
            .effective_port(),
        Some(8443)
    );
    assert_eq!(
        UrlParts::parse("custom://example.test")
            .unwrap()
            .effective_port(),
        None
    );
    assert!(UrlParts::parse("missing-scheme").is_err());
    assert!(UrlParts::parse("http://").is_err());
    assert!(UrlParts::parse("http://:80").is_err());
    assert_eq!(UrlParts::parse("http://[::1]/").unwrap().host, "::1");
    for invalid in [
        "http://bad host/",
        "http://example.test:",
        "http://example.test:0",
        "http://example.test:65536",
        "http://example.test:not-a-port",
        "http://2001:db8::1/",
        "http://[not-ipv6]/",
        "http://[::1]suffix/",
        "http://example].test/",
        "http://user@example.test/",
        "http://example.test/#fragment",
    ] {
        assert!(UrlParts::parse(invalid).is_err(), "{invalid}");
    }

    for valid in [
        "#fragment",
        "/next?ok=1#section",
        "//cdn.example.test:8443/asset",
        "https://safe.test:443/path#section",
    ] {
        validate_redirect_location(valid).unwrap_or_else(|error| panic!("{valid}: {error}"));
    }
    for invalid in [
        "",
        "/bad path",
        r"/bad\path",
        "//cdn.example.test:bad/asset",
        "https://safe.test:70000/",
        "javascript:alert(1)",
        "1relative:segment",
        "relative:segment",
        "/bad%2",
    ] {
        assert!(validate_redirect_location(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn low_level_matching_rejects_invalid_and_boundary_patterns() {
    let url = UrlParts::parse("http://api.example.test/path").unwrap();
    assert!(!exact_url_matches("not-a-url", &url));
    let globs = CompiledGlobSet::of(&[
        ("api*.example.test", '.'),
        ("192.0.*.*", '\0'),
        (r"literal\*", '.'),
        ("*", '\0'),
        ("**", '\0'),
        ("*/end", '/'),
    ]);
    assert!(globs.host_matches("*", "anything.test"));
    assert!(!globs.host_matches("*.example.test", "deep.api.example.test"));
    assert!(globs.host_matches("api*.example.test", "api2.example.test"));
    assert!(!globs.host_matches("api*.example.test", "other.example.test"));
    assert!(globs.ip_matches("*", "192.0.2.1"));
    assert!(globs.ip_matches("192.0.*.*", &normalize_ip_value("192.0.2.1:443")));
    assert!(!globs.ip_matches("192.0.2.2", "192.0.2.1"));
    assert!(path_prefix_matches("/", "/anything"));
    assert!(globs.glob_match(r"literal\*", "literal*", '.'));
    assert!(!globs.glob_match(r"literal\*", "literal-x", '.'));
    assert!(!globs.glob_match("*", "left\0right", '\0'));
    assert!(globs.glob_match("**", "left\0right", '\0'));

    let mut captures = Captures::default();
    captures.insert_index("kept".to_string());
    assert!(!globs.glob_match_with_captures("*/end", "a/bad", '/', &mut captures));
    assert_eq!(captures.get_index(1), Some("kept"));

    let request = req("http://example.test/chance");
    assert!(!chance(&request, 1, 0));
    assert!(chance(&request, 1, 1000));
    let deterministic = chance(&request, 9, 500);
    assert_eq!(chance(&request, 9, 500), deterministic);
    assert_eq!(header(&request.headers, "missing"), None);
}

#[test]
fn glob_matching_is_bounded_non_greedy_and_capture_safe() {
    let pattern = std::iter::repeat_n("*:", 12).collect::<String>() + "done";
    let text = (0..12).map(|index| format!("{index}:")).collect::<String>() + "done";
    let long = "a".repeat(MAX_RULE_LINE_BYTES);
    let globs = CompiledGlobSet::of(&[
        ("*/**/end", '/'),
        (&pattern, '\0'),
        ("*:*:missing", '\0'),
        (&long, '/'),
    ]);

    let mut captures = Captures::default();
    assert!(globs.glob_match_with_captures("*/**/end", "first/deep/path/end", '/', &mut captures));
    assert_eq!(captures.get_index(1), Some("first"));
    assert_eq!(captures.get_index(2), Some("deep/path"));

    let mut captures = Captures::default();
    captures.insert_index("existing".to_string());
    assert!(globs.glob_match_with_captures(&pattern, &text, '\0', &mut captures));
    assert_eq!(captures.indexed.len(), 9);
    assert_eq!(captures.get_index(1), Some("existing"));
    for index in 0..8 {
        assert_eq!(
            captures.get_index(index + 2),
            Some(index.to_string().as_str())
        );
    }

    let before = captures.clone();
    assert!(!globs.glob_match_with_captures("*:*:missing", "one:two:done", '\0', &mut captures));
    assert_eq!(captures, before);

    // The former recursive matcher exhausted the stack on a long literal.
    // The compiled linear matcher accepts the same maximum-size input without
    // recursion or backtracking growth.
    assert!(globs.glob_match(&long, &long, '/'));
}

#[test]
fn matcher_and_condition_rejection_paths_do_not_leak_captures() {
    let url = UrlParts::parse("https://example.test:443/api?x=1").unwrap();
    let wrong_scheme = GlobMatcher {
        scheme: Some("http".to_string()),
        host: "example.test".to_string(),
        port: None,
        path: None,
        query: None,
    };
    assert!(
        Matcher::Glob(wrong_scheme)
            .matches(&url, "https://example.test/api")
            .is_none()
    );
    let missing_port = GlobMatcher {
        scheme: None,
        host: "example.test".to_string(),
        port: Some("8443".to_string()),
        path: None,
        query: None,
    };
    assert!(
        Matcher::Glob(missing_port)
            .matches(&url, "https://example.test/api")
            .is_none()
    );
    let wrong_path = GlobMatcher {
        scheme: None,
        host: "example.test".to_string(),
        port: None,
        path: Some("/other".to_string()),
        query: None,
    };
    assert!(
        Matcher::Glob(wrong_path)
            .matches(&url, "https://example.test/api")
            .is_none()
    );
    let wrong_query = GlobMatcher {
        scheme: None,
        host: "example.test".to_string(),
        port: None,
        path: None,
        query: Some("token=*".to_string()),
    };
    assert!(
        Matcher::Glob(wrong_query)
            .matches(&url, "https://example.test/api")
            .is_none()
    );

    let request = req("https://example.test/api");
    assert!(
        !Condition::Url(UrlCondition::Glob("https://other.test".to_string())).matches(
            &request,
            Some(&url),
            None,
            1,
        )
    );
    for condition in [
        Condition::ResHeaderPresent("x-id".to_string()),
        Condition::ResHeaderContains {
            name: "x-id".to_string(),
            value: "42".to_string(),
        },
        Condition::Status(vec![200]),
    ] {
        assert!(!condition.matches(&request, Some(&url), None, 1));
    }
    assert!(Condition::Not(Box::new(Condition::Status(vec![200]))).depends_on_response());
    assert!(
        Condition::Any(vec![Condition::BodyContains("x".to_string())]).depends_on_request_body()
    );
    let condition = Condition::Not(Box::new(Condition::BodyContains("x".to_string())));
    let globs = CompiledGlobSet::for_condition(&condition);
    let body_literals = CompiledBodyContainsSet::for_condition(&condition);
    let resources = bind_condition_resources(&condition, &globs, &body_literals);
    let cache = matcher::ConditionCache::new(&request);
    let context = matcher::ConditionMatchContext::compiled(
        Some(&url),
        None,
        1,
        &globs,
        &body_literals,
        &cache,
    );
    assert!(condition.may_match_before_request_body(&resources, &context));
}

#[test]
fn index_helpers_keep_order_and_classify_non_domain_hosts_as_global() {
    assert_eq!(host_suffixes("a.b."), vec!["a.b.", "b."]);
    let mut output = vec![1];
    let mut seen = HashSet::from([1]);
    extend_unique(&mut output, &mut seen, &[1, 2, 2, 3]);
    assert_eq!(output, vec![1, 2, 3]);

    let rules = RuleSet::parse(
        "edges",
        "api*.example.test status(200)\n*.bad*host status(201)\n* status(202)",
    )
    .unwrap();
    assert_eq!(rules.stats().global_rules, 3);
    assert!(
        RuleSet::empty()
            .resolve(&req("http://example.test"))
            .actions
            .is_empty()
    );
    assert!(
        RuleSet::parse("edges", "\n# comment\n")
            .unwrap()
            .rules
            .is_empty()
    );
}
