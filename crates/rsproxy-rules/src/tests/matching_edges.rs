use super::*;

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
}

#[test]
fn low_level_matching_rejects_invalid_and_boundary_patterns() {
    let url = UrlParts::parse("http://api.example.test/path").unwrap();
    assert!(!exact_url_matches("not-a-url", &url));
    assert!(host_matches("*", "anything.test"));
    assert!(!host_matches("*.example.test", "deep.api.example.test"));
    assert!(host_matches("api*.example.test", "api2.example.test"));
    assert!(!host_matches("api*.example.test", "other.example.test"));
    assert!(ip_matches("*", "192.0.2.1"));
    assert!(ip_matches("192.0.*.*", "192.0.2.1:443"));
    assert!(!ip_matches("192.0.2.2", "192.0.2.1"));
    assert!(path_prefix_matches("/", "/anything"));
    assert!(glob_match(r"literal\*", "literal*", '.'));
    assert!(!glob_match(r"literal\*", "literal-x", '.'));

    let mut captures = Captures::default();
    captures.insert_index("kept".to_string());
    assert!(!glob_match_with_captures(
        "*/end",
        "a/bad",
        '/',
        &mut captures
    ));
    assert_eq!(captures.get_index(1), Some("kept"));

    let request = req("http://example.test/chance");
    assert!(!chance(&request, 1, 0));
    assert!(chance(&request, 1, 1000));
    let deterministic = chance(&request, 9, 500);
    assert_eq!(chance(&request, 9, 500), deterministic);
    assert_eq!(header(&request.headers, "missing"), None);
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
    assert!(
        Condition::Not(Box::new(Condition::BodyContains("x".to_string())))
            .may_match_before_request_body(&request, Some(&url), 1)
    );
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
