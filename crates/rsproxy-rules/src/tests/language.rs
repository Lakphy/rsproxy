use super::*;
use std::collections::HashSet;

#[test]
fn language_registry_spellings_and_topics_are_closed() {
    for (kind, spellings) in [
        ("action", ACTION_SYNTAX),
        ("condition", CONDITION_SYNTAX),
        ("matcher", MATCHER_SYNTAX),
        ("property", PROPERTY_SYNTAX),
    ] {
        let mut accepted = HashSet::new();
        for spelling in spellings {
            assert!(
                !spelling.topics.is_empty(),
                "{kind}: {}",
                spelling.canonical
            );
            assert!(!spelling.canonical.is_empty(), "{kind}");
            assert!(
                accepted.insert(spelling.canonical),
                "{kind}: {}",
                spelling.canonical
            );
            for alias in spelling.aliases {
                assert!(accepted.insert(alias), "{kind}: duplicate {alias}");
            }
        }
    }

    let action_topics = ACTION_SYNTAX
        .iter()
        .flat_map(|spelling| spelling.topics)
        .map(|topic| ActionFamily::from_name(topic.strip_prefix("action.").unwrap()).unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(
        action_topics,
        Action::FAMILIES.iter().copied().collect::<HashSet<_>>()
    );
    let stackable = Action::STACKABLE_FAMILIES
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    assert_eq!(stackable.len(), Action::STACKABLE_FAMILIES.len());
    assert!(stackable.is_subset(&action_topics));
    for family in Action::FAMILIES {
        assert_eq!(
            Action::family_is_stackable(*family),
            stackable.contains(family)
        );
    }

    let condition_topics = CONDITION_SYNTAX
        .iter()
        .flat_map(|spelling| spelling.topics)
        .copied()
        .collect::<HashSet<_>>();
    assert_eq!(condition_topics.len(), 14);
    assert!(condition_topics.contains("condition.res.header"));
    assert!(condition_topics.contains("condition.client-ip"));
    assert_eq!(MATCHER_SYNTAX.len(), 5);
    assert_eq!(PROPERTY_SYNTAX.len(), 3);
}

#[test]
fn every_action_family_declares_effect_phases() {
    for family in Action::FAMILIES {
        assert!(!Action::family_phases(*family).is_empty(), "{family}");
    }
    assert_eq!(ActionFamily::Host.phases(), [Phase::Req]);
    assert_eq!(ActionFamily::ResHeader.phases(), [Phase::Res]);
    assert_eq!(ActionFamily::Delete.phases(), [Phase::Req, Phase::Res]);
    assert!(ActionFamily::from_name("unknown").is_none());

    let request_delete = RuleSet::parse("phase", "example.test delete(reqBody)").unwrap();
    let action = &request_delete.rules()[0].actions[0];
    assert!(action.applies_in(Phase::Req));
    assert!(!action.applies_in(Phase::Res));

    let mixed_delete = RuleSet::parse("phase", "example.test delete(reqBody, resBody)").unwrap();
    let action = &mixed_delete.rules()[0].actions[0];
    assert!(action.applies_in(Phase::Req));
    assert!(action.applies_in(Phase::Res));
}

#[test]
fn language_registry_canonicalizes_every_compatibility_alias() {
    for spelling in ACTION_SYNTAX {
        assert_eq!(
            canonical_action_name(spelling.canonical),
            Some(spelling.canonical)
        );
        for alias in spelling.aliases {
            assert_eq!(canonical_action_name(alias), Some(spelling.canonical));
        }
    }
    for spelling in CONDITION_SYNTAX {
        assert_eq!(
            canonical_condition_name(spelling.canonical),
            Some(spelling.canonical)
        );
        for alias in spelling.aliases {
            assert_eq!(canonical_condition_name(alias), Some(spelling.canonical));
        }
    }
    assert_eq!(canonical_action_name("unknown"), None);
    assert_eq!(canonical_condition_name("unknown"), None);
    assert_eq!(canonical_property_name("@important"), Some("@important"));
    assert_eq!(canonical_property_name("@tag:health"), Some("@tag:"));
    assert_eq!(canonical_property_name("@tag:"), None);
    assert_eq!(canonical_property_name("unknown"), None);
}

#[test]
fn every_registered_compatibility_alias_reaches_real_parser_dispatch() {
    for spelling in ACTION_SYNTAX
        .iter()
        .filter(|spelling| !spelling.aliases.is_empty())
    {
        for alias in spelling.aliases {
            let action = match spelling.canonical {
                "mock.raw" => format!("{alias}(raw)"),
                "map.remote" => format!("{alias}(http://origin.test)"),
                canonical => panic!("missing action alias fixture for {canonical}"),
            };
            RuleSet::parse("aliases", &format!("example.test {action}"))
                .unwrap_or_else(|errors| panic!("{alias}: {errors:?}"));
        }
    }

    for spelling in CONDITION_SYNTAX
        .iter()
        .filter(|spelling| !spelling.aliases.is_empty())
    {
        for alias in spelling.aliases {
            let condition = match spelling.canonical {
                "client.ip" | "server.ip" => format!("{alias}(192.0.2.*)"),
                "res.header" => format!("{alias}(x-test)"),
                canonical => panic!("missing condition alias fixture for {canonical}"),
            };
            RuleSet::parse(
                "aliases",
                &format!("example.test status(204) when {condition}"),
            )
            .unwrap_or_else(|errors| panic!("{alias}: {errors:?}"));
        }
    }
}

#[test]
fn versioned_v3_sources_require_the_header_and_canonical_names() {
    let missing = RuleSet::parse_versioned("v3", "example.test direct").unwrap_err();
    assert!(missing[0].message.contains(RULE_LANGUAGE_HEADER));

    let rules = RuleSet::parse_versioned(
        "v3",
        "# comments may precede the directive\n@language 3\nexample.test direct when client.ip(192.0.2.*)\n",
    )
    .unwrap();
    assert_eq!(rules.language_version(), 3);
    assert_eq!(rules.rules()[0].line, 3);

    for alias in ["clientIp", "client_ip", "client-ip", "ip"] {
        let errors = RuleSet::parse_versioned(
            "v3",
            &format!("@language 3\nexample.test direct when {alias}(192.0.2.*)"),
        )
        .unwrap_err();
        assert!(errors[0].message.contains("canonical condition names only"));
        assert!(errors[0].message.contains("client.ip"));
    }
}

#[test]
fn versioned_groups_validate_each_non_empty_source_independently() {
    let errors = RuleSet::parse_versioned_groups([
        ("first", "@language 3\nexample.test direct"),
        ("second", "example.test status(503)"),
    ])
    .unwrap_err();
    assert_eq!(errors[0].group, "second");
    assert!(errors[0].message.contains("missing"));

    assert!(RuleSet::parse_versioned_groups([("empty", ""), ("comments", "# only\n")]).is_ok());
}
