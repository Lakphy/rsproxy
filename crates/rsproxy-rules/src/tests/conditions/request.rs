use super::*;

#[test]
fn parses_and_matches_host_glob() {
    let rules = RuleSet::parse(
        "default",
        r#"
            **.example.com/api/** req.header(x-hit: $1)
            "#,
    )
    .unwrap();
    let result = rules.resolve(&req("http://a.b.example.com/api/v1/users"));
    assert_eq!(result.actions.len(), 1);
    assert!(matches!(result.actions[0].action, Action::ReqHeader(_)));
}

#[test]
fn exact_without_query_allows_any_query() {
    let rules = RuleSet::parse("default", "=http://example.com/a status(204)").unwrap();
    let result = rules.resolve(&req("http://example.com/a?x=1"));
    assert!(matches!(result.actions[0].action, Action::Status(204)));
}

#[test]
fn method_condition_falls_through() {
    let rules = RuleSet::parse(
        "default",
        "example.com status(500) when method(POST)\nexample.com status(200)",
    )
    .unwrap();
    let result = rules.resolve(&req("http://example.com/"));
    assert!(matches!(result.actions[0].action, Action::Status(200)));
}

#[test]
fn client_ip_condition_matches_exact_glob_and_alias() {
    let mut request = req("http://example.com/");
    request.client_ip = Some("203.0.113.9:61234".to_string());

    let exact = RuleSet::parse(
        "default",
        "example.com status(210) when clientIp(203.0.113.9)\nexample.com status(500)",
    )
    .unwrap();
    assert!(matches!(
        exact.resolve(&request).actions[0].action,
        Action::Status(210)
    ));

    let glob = RuleSet::parse(
        "default",
        "example.com status(211) when ip(203.0.*)\nexample.com status(500)",
    )
    .unwrap();
    assert!(matches!(
        glob.resolve(&request).actions[0].action,
        Action::Status(211)
    ));

    request.client_ip = Some("198.51.100.9".to_string());
    assert!(matches!(
        glob.resolve(&request).actions[0].action,
        Action::Status(500)
    ));
}

#[test]
fn server_ip_condition_matches_exact_glob_and_template() {
    let mut request = req("http://127.0.0.1/");
    request.server_ip = Some("127.0.0.1:8080".to_string());

    let exact = RuleSet::parse(
        "default",
        "127.0.0.1 status(210) when serverIp(127.0.0.1)\n127.0.0.1 status(500)",
    )
    .unwrap();
    assert!(matches!(
        exact.resolve(&request).actions[0].action,
        Action::Status(210)
    ));

    let glob = RuleSet::parse(
        "default",
        "127.0.0.1 status(211) when serverIp(127.0.*)\n127.0.0.1 status(500)",
    )
    .unwrap();
    let result = glob.resolve(&request);
    assert!(matches!(result.actions[0].action, Action::Status(211)));

    request.server_ip = Some("198.51.100.9".to_string());
    assert!(matches!(
        glob.resolve(&request).actions[0].action,
        Action::Status(500)
    ));

    request.server_ip = Some("127.0.0.1:8080".to_string());
    let templated = RuleSet::parse(
        "default",
        "127.0.0.1 req.header(x-server-ip: ${serverIp}) when serverIp(127.0.*)",
    )
    .unwrap();
    assert_eq!(
        templated.explain(&request),
        "default:1 req.header(x-server-ip: 127.0.0.1:8080)\n"
    );
}

#[test]
fn url_condition_matches_glob_and_regex() {
    let glob = RuleSet::parse(
        "default",
        "example.com status(210) when url(*mode=match*)\nexample.com status(500)",
    )
    .unwrap();
    assert!(matches!(
        glob.resolve(&req("http://example.com/path?mode=match&x=1"))
            .actions[0]
            .action,
        Action::Status(210)
    ));
    assert!(matches!(
        glob.resolve(&req("http://example.com/path?mode=miss"))
            .actions[0]
            .action,
        Action::Status(500)
    ));

    let regex = RuleSet::parse(
        "default",
        r#"example.com status(211) when url(/\/items\/\d+\?ok=1/)
example.com status(500)"#,
    )
    .unwrap();
    assert!(matches!(
        regex
            .resolve(&req("http://example.com/items/42?ok=1"))
            .actions[0]
            .action,
        Action::Status(211)
    ));
    assert!(matches!(
        regex
            .resolve(&req("http://example.com/items/nope?ok=1"))
            .actions[0]
            .action,
        Action::Status(500)
    ));
}

#[test]
fn any_condition_matches_nested_condition_or_falls_through() {
    let rules = RuleSet::parse(
            "default",
            "example.com status(210) when any(method(POST, PUT), header(x-mode ~ beta))\nexample.com status(500)",
        )
        .unwrap();

    let mut request = req("http://example.com/");
    request.method = "GET".to_string();
    request.headers = vec![("X-Mode".to_string(), "alpha".to_string())];
    let result = rules.resolve(&request);
    assert!(matches!(result.actions[0].action, Action::Status(500)));
    assert_eq!(result.matched_rules[0].line, 2);

    request.headers = vec![("X-Mode".to_string(), "beta-preview".to_string())];
    let result = rules.resolve(&request);
    assert!(matches!(result.actions[0].action, Action::Status(210)));
    assert_eq!(result.matched_rules[0].line, 1);

    request.method = "PUT".to_string();
    request.headers.clear();
    let result = rules.resolve(&request);
    assert!(matches!(result.actions[0].action, Action::Status(210)));
    assert_eq!(result.matched_rules[0].line, 1);
}

#[test]
fn env_condition_matches_presence_and_exact_value() {
    let path = std::env::var("PATH").unwrap_or_default();
    let escaped_path = path.replace('\\', "\\\\").replace('"', "\\\"");

    let exact = RuleSet::parse(
        "default",
        &format!(
            "example.com status(210) when env(PATH=\"{escaped_path}\")\nexample.com status(500)"
        ),
    )
    .unwrap();
    let result = exact.resolve(&req("http://example.com/"));
    assert!(matches!(result.actions[0].action, Action::Status(210)));
    assert_eq!(result.matched_rules[0].line, 1);

    let present = RuleSet::parse(
        "default",
        "example.com status(211) when env(PATH)\nexample.com status(500)",
    )
    .unwrap();
    let result = present.resolve(&req("http://example.com/"));
    assert!(matches!(result.actions[0].action, Action::Status(211)));
    assert_eq!(result.matched_rules[0].line, 1);

    let missing = RuleSet::parse(
            "default",
            "example.com status(212) when env(RSPROXY_TEST_ENV_CONDITION_SHOULD_NOT_EXIST=enabled)\nexample.com status(500)",
        )
        .unwrap();
    let result = missing.resolve(&req("http://example.com/"));
    assert!(matches!(result.actions[0].action, Action::Status(500)));
    assert_eq!(result.matched_rules[0].line, 2);
}

#[test]
fn body_condition_matches_contains_and_regex() {
    let contains = RuleSet::parse(
        "default",
        "example.com status(210) when body(~ beta-token)\nexample.com status(500)",
    )
    .unwrap();
    let mut request = req("http://example.com/");
    request.body = b"alpha BETA-token gamma".to_vec();
    let result = contains.resolve(&request);
    assert!(matches!(result.actions[0].action, Action::Status(210)));
    assert_eq!(result.matched_rules[0].line, 1);

    request.body = b"alpha gamma".to_vec();
    let result = contains.resolve(&request);
    assert!(matches!(result.actions[0].action, Action::Status(500)));
    assert_eq!(result.matched_rules[0].line, 2);

    let regex = RuleSet::parse(
        "default",
        "example.com status(211) when body(/token=\\d+/)\nexample.com status(500)",
    )
    .unwrap();
    request.body = b"token=42".to_vec();
    let result = regex.resolve(&request);
    assert!(matches!(result.actions[0].action, Action::Status(211)));
    assert_eq!(result.matched_rules[0].line, 1);
}

#[test]
fn all_condition_requires_every_nested_condition() {
    let rules = RuleSet::parse(
        "default",
        "example.com status(212) when all(method(POST), header(x-mode ~ beta))\nexample.com status(500)",
    )
    .unwrap();

    let mut request = req("http://example.com/");
    request.method = "POST".to_string();
    request
        .headers
        .push(("x-mode".to_string(), "beta-1".to_string()));
    let result = rules.resolve(&request);
    assert!(matches!(result.actions[0].action, Action::Status(212)));
    assert_eq!(result.matched_rules[0].line, 1);

    // One failing nested condition falls through to the next rule.
    request.method = "GET".to_string();
    let result = rules.resolve(&request);
    assert!(matches!(result.actions[0].action, Action::Status(500)));
    assert_eq!(result.matched_rules[0].line, 2);
}

#[test]
fn not_condition_call_form_matches_bang_prefix() {
    let call = RuleSet::parse("default", "example.com status(213) when not(method(GET))").unwrap();
    let bang = RuleSet::parse("default", "example.com status(213) when !method(GET)").unwrap();
    assert_eq!(call.rules[0].conditions, bang.rules[0].conditions);

    let mut request = req("http://example.com/");
    request.method = "POST".to_string();
    assert!(matches!(
        call.resolve(&request).actions[0].action,
        Action::Status(213)
    ));
    request.method = "GET".to_string();
    assert!(call.resolve(&request).actions.is_empty());
}

#[test]
fn all_and_not_nest_inside_any() {
    let rules = RuleSet::parse(
        "default",
        "example.com status(214) when any(all(method(POST), header(x-a)), not(header(x-b)))",
    )
    .unwrap();
    // Neither branch: GET with x-b present.
    let mut request = req("http://example.com/");
    request.headers.push(("x-b".to_string(), "1".to_string()));
    assert!(rules.resolve(&request).actions.is_empty());
    // Second branch: x-b absent.
    let request = req("http://example.com/");
    assert!(matches!(
        rules.resolve(&request).actions[0].action,
        Action::Status(214)
    ));
}

#[test]
fn all_condition_requires_at_least_one_argument() {
    let errors =
        RuleSet::parse("default", "example.com status(200) when all()").expect_err("empty all");
    assert_eq!(errors[0].code, RuleErrorCode::Condition);
    let errors =
        RuleSet::parse("default", "example.com status(200) when not()").expect_err("empty not");
    assert_eq!(errors[0].code, RuleErrorCode::Condition);
}
