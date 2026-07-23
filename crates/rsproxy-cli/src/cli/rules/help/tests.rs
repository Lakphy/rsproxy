use super::*;
use rsproxy_rules::{
    ACTION_SYNTAX, Action, CONDITION_SYNTAX, MATCHER_SYNTAX, PROPERTY_SYNTAX, RuleSet,
};
use std::collections::{HashMap, HashSet};

#[test]
fn catalog_ids_aliases_relations_and_categories_are_closed() {
    let ids = topics().map(|topic| topic.id).collect::<HashSet<_>>();
    assert_eq!(ids.len(), topic_count(), "topic IDs must be unique");
    let categories = CATEGORIES
        .iter()
        .map(|(category, _)| *category)
        .collect::<HashSet<_>>();
    for topic in topics() {
        assert!(
            topic
                .id
                .starts_with(&format!("{}.", singular(topic.category))),
            "{} has mismatched category {}",
            topic.id,
            topic.category
        );
        assert!(categories.contains(topic.category), "{}", topic.id);
        assert!(!topic.summary.is_empty(), "{}", topic.id);
        assert!(!topic.syntax.is_empty(), "{}", topic.id);
        assert!(!topic.examples.is_empty(), "{}", topic.id);
        for related in topic.related {
            assert!(
                ids.contains(related) || category_name(related).is_some(),
                "{} has unknown relation {related}",
                topic.id
            );
        }
        if topic.category != "concepts" {
            assert!(
                topic_dsl_spellings(topic).next().is_some(),
                "{} has no shared DSL spelling",
                topic.id
            );
        }
    }

    for spelling in ACTION_SYNTAX
        .iter()
        .chain(CONDITION_SYNTAX)
        .chain(MATCHER_SYNTAX)
        .chain(PROPERTY_SYNTAX)
    {
        for topic in spelling.topics {
            assert!(
                ids.contains(topic),
                "registry references unknown topic {topic}"
            );
        }
    }

    let mut aliases = HashMap::new();
    for topic in topics() {
        for alias in topic.aliases {
            let alias = normalize(alias);
            if let Some(previous) = aliases.insert(alias.clone(), topic.id) {
                assert_eq!(previous, topic.id, "duplicate exact alias {alias}");
            }
        }
    }
}

#[test]
fn catalog_covers_every_action_family_and_parser_surface() {
    let documented_actions = topics()
        .filter_map(|topic| topic.id.strip_prefix("action."))
        .collect::<HashSet<_>>();
    assert_eq!(
        documented_actions,
        Action::FAMILIES
            .iter()
            .map(|family| family.as_str())
            .collect::<HashSet<_>>()
    );

    for condition in [
        "method",
        "host",
        "url",
        "header",
        "res.header",
        "body",
        "client-ip",
        "server-ip",
        "status",
        "chance",
        "env",
        "any",
        "all",
        "not",
    ] {
        assert!(topics().any(|topic| topic.id == format!("condition.{condition}")));
    }
    for matcher in ["glob", "exact", "regex", "port", "not"] {
        assert!(topics().any(|topic| topic.id == format!("matcher.{matcher}")));
    }
    for property in ["important", "disabled", "tag"] {
        assert!(topics().any(|topic| topic.id == format!("property.{property}")));
    }
}

#[test]
fn action_help_exposes_parser_authoritative_resolution_and_phase_metadata() {
    let request = topics().find(|topic| topic.id == "action.host").unwrap();
    assert_eq!(
        topic_json(request)["phases"],
        serde_json::json!(["request"])
    );
    assert_eq!(
        topic_json(request)["resolution"],
        "single: first applicable action wins"
    );

    let response = topics()
        .find(|topic| topic.id == "action.res.header")
        .unwrap();
    assert_eq!(
        topic_json(response)["phases"],
        serde_json::json!(["response"])
    );
    assert_eq!(topic_json(response)["resolution"], "stackable");

    let both = topics().find(|topic| topic.id == "action.delete").unwrap();
    assert_eq!(
        topic_json(both)["phases"],
        serde_json::json!(["request", "response"])
    );
}

#[test]
fn every_dsl_help_example_parses_and_action_examples_name_their_family() {
    for topic in topics() {
        for example in topic.examples {
            let rules = RuleSet::parse("help", example).unwrap_or_else(|errors| {
                panic!("{} example failed: {example:?}: {errors:?}", topic.id)
            });
            if let Some(family) = topic.id.strip_prefix("action.") {
                assert!(
                    rules
                        .rules()
                        .iter()
                        .flat_map(|rule| &rule.actions)
                        .any(|action| action.family().as_str() == family),
                    "{} example does not contain family {family}: {example}",
                    topic.id
                );
            }
        }
    }
}

#[test]
fn exact_shorthand_ambiguity_category_and_search_are_deterministic() {
    let (_, _, topics) = select_topics(RulesHelpArgs {
        topic: Some("req.header".to_string()),
        search: None,
    })
    .unwrap();
    assert_eq!(topics[0].id, "action.req.header");

    let (_, _, topics) = select_topics(RulesHelpArgs {
        topic: Some("mockRaw".to_string()),
        search: None,
    })
    .unwrap();
    assert_eq!(topics[0].id, "action.mock");

    let (_, _, topics) = select_topics(RulesHelpArgs {
        topic: Some("client_ip".to_string()),
        search: None,
    })
    .unwrap();
    assert_eq!(topics[0].id, "condition.client-ip");

    let error = select_topics(RulesHelpArgs {
        topic: Some("status".to_string()),
        search: None,
    })
    .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("action.status"));
    assert!(message.contains("condition.status"));

    let (kind, _, actions) = select_topics(RulesHelpArgs {
        topic: Some("actions".to_string()),
        search: None,
    })
    .unwrap();
    assert_eq!(kind, "category");
    assert_eq!(actions.len(), Action::FAMILIES.len());

    let (kind, _, found) = select_topics(RulesHelpArgs {
        topic: None,
        search: Some("response header".to_string()),
    })
    .unwrap();
    assert_eq!(kind, "search");
    assert!(found.iter().any(|topic| topic.id == "action.res.header"));
    assert!(found.iter().any(|topic| topic.id == "condition.res.header"));

    let (_, _, found) = select_topics(RulesHelpArgs {
        topic: None,
        search: Some("script hook program transform".to_string()),
    })
    .unwrap();
    assert_eq!(found[0].id, "concept.scripting");
}

fn singular(category: &str) -> &str {
    match category {
        "concepts" => "concept",
        "matchers" => "matcher",
        "conditions" => "condition",
        "actions" => "action",
        "properties" => "property",
        _ => unreachable!(),
    }
}
