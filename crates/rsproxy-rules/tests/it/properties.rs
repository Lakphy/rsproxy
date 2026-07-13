use proptest::prelude::*;
use rsproxy_rules::{RequestMeta, ResponseMeta, RuleErrorCode, RuleSet};

fn safe_word() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-z][a-z0-9]{0,11}").unwrap()
}

fn action() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("direct".to_string()),
        Just("bypass".to_string()),
        Just("hide".to_string()),
        (100u16..600).prop_map(|code| format!("status({code})")),
        safe_word().prop_map(|value| format!("tag({value}-${{path}})")),
        safe_word().prop_map(|value| format!("req.header(x-property: {value})")),
        safe_word().prop_map(|value| format!("req.cookie({value}=${{id}})")),
        safe_word().prop_map(|value| format!("url.query({value}=${{method}})")),
        safe_word().prop_map(|value| format!("req.body.append(\"{value}\")")),
        (1u64..5_000).prop_map(|millis| format!("delay(req, {millis}ms)")),
        (1u64..1_024).prop_map(|kb| format!("throttle(res, {kb}KB/s)")),
    ]
}

fn condition() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just(" when method(GET, POST)".to_string()),
        safe_word().prop_map(|value| format!(" when header(x-mode ~ {value})")),
        safe_word().prop_map(|value| format!(" when url(*{value}*)")),
        safe_word().prop_map(|value| format!(" when !header(x-{value})")),
    ]
}

fn rule_line() -> impl Strategy<Value = String> {
    (
        safe_word(),
        proptest::collection::vec(action(), 1..5),
        condition(),
        any::<bool>(),
    )
        .prop_map(|(label, actions, condition, important)| {
            format!(
                "{label}.example.test/api/{label} {}{condition}{}",
                actions.join(" "),
                if important { " @important" } else { "" }
            )
        })
}

fn request(url: String) -> RequestMeta {
    RequestMeta {
        method: "GET".to_string(),
        url,
        headers: vec![("X-Mode".to_string(), "property".to_string())],
        body: b"property-body".to_vec(),
        client_ip: Some("192.0.2.10".to_string()),
        server_ip: Some("198.51.100.20".to_string()),
        template: Default::default(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn generated_valid_rules_reparse_to_the_same_ast_and_resolution(
        lines in proptest::collection::vec(rule_line(), 1..12)
    ) {
        let source = lines.join("\n");
        let parsed = RuleSet::parse("property", &source).unwrap();
        let printed = parsed
            .rules
            .iter()
            .map(|rule| rule.raw.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let reparsed = RuleSet::parse("property", &printed).unwrap();
        prop_assert_eq!(&parsed.rules, &reparsed.rules);
        prop_assert_eq!(parsed.stats(), reparsed.stats());

        let req = request("http://alpha.example.test/api/alpha?mode=property".to_string());
        let res = ResponseMeta {
            status: 200,
            headers: vec![("X-Origin".to_string(), "property".to_string())],
        };
        prop_assert_eq!(parsed.resolve(&req), reparsed.resolve(&req));
        prop_assert_eq!(
            parsed.resolve_response(&req, &res),
            reparsed.resolve_response(&req, &res)
        );
    }

    #[test]
    fn generated_near_valid_rules_return_structured_errors(
        line in rule_line(), mutation in 0u8..3
    ) {
        let broken = match mutation {
            0 => format!("{line} unknown.action("),
            1 => format!("{line} when"),
            _ => format!("{line} @unknown-property"),
        };
        let errors = RuleSet::parse("property", &broken).unwrap_err();
        prop_assert_eq!(errors.len(), 1);
        prop_assert_eq!(errors[0].group.as_str(), "property");
        prop_assert_eq!(errors[0].line, 1);
        prop_assert!(matches!(
            errors[0].code,
            RuleErrorCode::Syntax
                | RuleErrorCode::Action
                | RuleErrorCode::Condition
                | RuleErrorCode::Property
        ));
    }

    #[test]
    fn bounded_utf8_input_never_panics_across_parse_and_resolve(
        input in proptest::collection::vec(any::<char>(), 0..512)
            .prop_map(|chars| chars.into_iter().collect::<String>()),
        url_tail in proptest::collection::vec(any::<char>(), 0..128)
            .prop_map(|chars| chars.into_iter().collect::<String>())
    ) {
        if let Ok(rules) = RuleSet::parse("property", &input) {
            let req = request(format!("http://example.test/{url_tail}"));
            let res = ResponseMeta {
                status: 200,
                headers: Vec::new(),
            };
            let _ = rules.stats();
            let _ = rules.request_body_required(&req);
            let _ = rules.resolve(&req);
            let _ = rules.resolve_without_request_body(&req);
            let _ = rules.resolve_response(&req, &res);
            let _ = rules.resolve_response_without_request_body(&req, &res);
            let _ = rules.explain(&req);
            let _ = rules.explain_response(&req, &res);
        }
    }
}
