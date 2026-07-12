#[path = "support/whistle_fixture.rs"]
mod whistle_fixture;

use rsproxy_rules::{Action, RequestMeta, RuleSet};
use serde::Deserialize;
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct MigrationMatrix {
    mapping: Vec<MigrationMapping>,
    #[serde(default)]
    syntax_mapping: Vec<SyntaxMapping>,
    unsupported: Vec<UnsupportedCapability>,
}

#[derive(Deserialize)]
struct MigrationMapping {
    id: String,
    whistle: Vec<String>,
    sources: Vec<String>,
    rsproxy: String,
    expect_actions: Vec<String>,
}

#[derive(Deserialize)]
struct UnsupportedCapability {
    capability: String,
    #[serde(default)]
    aliases: Vec<String>,
    status: String,
    source: String,
    reason: String,
}

#[derive(Deserialize)]
struct SyntaxMapping {
    id: String,
    whistle: Vec<String>,
    sources: Vec<String>,
    rsproxy: String,
    expect_actions: Vec<String>,
}

#[test]
fn whistle_migration_matrix_is_snapshot_backed_parseable_and_action_complete() {
    whistle_fixture::assert_pinned();
    let text = fs::read_to_string(matrix_path()).unwrap();
    let matrix: MigrationMatrix = toml::from_str(&text).unwrap();
    let implemented = Action::FAMILIES.iter().copied().collect::<BTreeSet<_>>();
    let mut observed = BTreeSet::new();
    let mut ids = HashSet::new();
    let mut classified_whistle = BTreeSet::new();

    for mapping in &matrix.mapping {
        assert!(
            ids.insert(mapping.id.as_str()),
            "duplicate mapping {}",
            mapping.id
        );
        assert!(
            !mapping.whistle.is_empty(),
            "{} has no whistle capability",
            mapping.id
        );
        assert!(
            !mapping.sources.is_empty(),
            "{} has no source evidence",
            mapping.id
        );
        let evidence = mapping
            .sources
            .iter()
            .map(|source| whistle_fixture::read(source))
            .collect::<Vec<_>>()
            .join("\n")
            .to_ascii_lowercase();
        for capability in &mapping.whistle {
            assert!(
                evidence.contains(&capability.to_ascii_lowercase()),
                "{} source evidence does not mention `{capability}`",
                mapping.id
            );
            classified_whistle.insert(capability.clone());
        }

        let rules = RuleSet::parse("migration", &mapping.rsproxy)
            .unwrap_or_else(|errors| panic!("{} rsproxy rule: {errors:?}", mapping.id));
        let actual = rules
            .resolve(&request())
            .actions
            .iter()
            .map(|item| item.action.family().to_string())
            .collect::<Vec<_>>();
        assert_eq!(actual, mapping.expect_actions, "{} actions", mapping.id);
        observed.extend(mapping.expect_actions.iter().map(String::as_str));
    }
    assert_eq!(observed, implemented, "migration matrix action coverage");

    for mapping in &matrix.syntax_mapping {
        assert!(
            ids.insert(mapping.id.as_str()),
            "duplicate mapping {}",
            mapping.id
        );
        assert_mapping_evidence(&mapping.id, &mapping.whistle, &mapping.sources);
        classified_whistle.extend(mapping.whistle.iter().cloned());
        let rules = RuleSet::parse("migration", &mapping.rsproxy)
            .unwrap_or_else(|errors| panic!("{} rsproxy rule: {errors:?}", mapping.id));
        let actual = rules
            .resolve(&request())
            .actions
            .iter()
            .map(|item| item.action.family().to_string())
            .collect::<Vec<_>>();
        assert_eq!(actual, mapping.expect_actions, "{} actions", mapping.id);
    }

    let mut unsupported = HashSet::new();
    for item in &matrix.unsupported {
        assert!(
            unsupported.insert(item.capability.as_str()),
            "duplicate unsupported capability {}",
            item.capability
        );
        classified_whistle.insert(item.capability.clone());
        for alias in &item.aliases {
            assert!(
                unsupported.insert(alias.as_str()),
                "duplicate unsupported capability {alias}"
            );
            classified_whistle.insert(alias.clone());
        }
        assert!(matches!(item.status.as_str(), "deferred-v2" | "removed-v1"));
        assert!(
            !item.reason.trim().is_empty(),
            "{} has no reason",
            item.capability
        );
        let evidence = whistle_fixture::read(&item.source).to_ascii_lowercase();
        assert!(
            evidence.contains(&item.capability.to_ascii_lowercase()),
            "unsupported source does not mention {}",
            item.capability
        );
        for alias in &item.aliases {
            assert!(
                evidence.contains(&alias.to_ascii_lowercase()),
                "unsupported source does not mention alias {alias}"
            );
        }
    }
    assert!(
        !unsupported.is_empty(),
        "unsupported scope must stay explicit"
    );
    let source_registry = whistle_source_registry();
    let missing = source_registry
        .difference(&classified_whistle)
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "unclassified Whistle registry names: {missing:?}"
    );
}

fn assert_mapping_evidence(id: &str, whistle: &[String], sources: &[String]) {
    assert!(!whistle.is_empty(), "{id} has no whistle capability");
    assert!(!sources.is_empty(), "{id} has no source evidence");
    let evidence = sources
        .iter()
        .map(|source| whistle_fixture::read(source))
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    for capability in whistle {
        assert!(
            evidence.contains(&capability.to_ascii_lowercase()),
            "{id} source evidence does not mention `{capability}`"
        );
    }
}

fn whistle_source_registry() -> BTreeSet<String> {
    let source = whistle_fixture::read("lib/rules/protocols.js");
    let protocols = source_block(&source, "var protocols = [", "];")
        .lines()
        .filter_map(parse_js_string_entry);
    let aliases = source_block(&source, "var aliasProtocols = {", "};")
        .lines()
        .filter_map(|line| line.trim().trim_end_matches(',').split_once(':'))
        .filter_map(|(name, _)| parse_js_string(name.trim()));
    protocols.chain(aliases).map(str::to_string).collect()
}

fn source_block<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    source
        .split_once(start)
        .and_then(|(_, rest)| rest.split_once(end))
        .map(|(block, _)| block)
        .unwrap_or_else(|| panic!("missing source block {start}"))
}

fn parse_js_string_entry(line: &str) -> Option<&str> {
    parse_js_string(line.trim().trim_end_matches(','))
}

fn parse_js_string(input: &str) -> Option<&str> {
    input
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .or_else(|| {
            input
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
        })
        .or_else(|| {
            (!input.is_empty()
                && input
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
            .then_some(input)
        })
}

fn request() -> RequestMeta {
    RequestMeta {
        method: "GET".to_string(),
        url: "http://example.test/items/42".to_string(),
        headers: Vec::new(),
        body: b"item-42".to_vec(),
        client_ip: Some("192.0.2.10".to_string()),
        server_ip: Some("198.51.100.20".to_string()),
        template: Default::default(),
    }
}

fn matrix_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/contracts/whistle_migration.toml")
}
