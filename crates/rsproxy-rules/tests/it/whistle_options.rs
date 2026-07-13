use crate::whistle_fixture;
use rsproxy_rules::{RequestMeta, ResponseMeta, RuleSet};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct OptionMatrix {
    classification: Vec<OptionClassification>,
}

#[derive(Deserialize)]
struct OptionClassification {
    protocol: String,
    options: Vec<String>,
    status: String,
    #[serde(default)]
    rsproxy: Option<String>,
    #[serde(default)]
    expect_actions: Vec<String>,
    #[serde(default)]
    configuration: Vec<String>,
    reason: String,
}

#[test]
fn whistle_option_matrix_classifies_documented_options_and_executes_supported_recipes() {
    whistle_fixture::assert_pinned();
    let root = workspace_root();
    let matrix: OptionMatrix = toml::from_str(&fs::read_to_string(matrix_path()).unwrap()).unwrap();
    let documented = BTreeMap::from([
        ("enable", documented_bullet_options("enable")),
        ("disable", documented_bullet_options("disable")),
        ("delete", documented_delete_options()),
    ]);
    let mut classified = BTreeMap::<String, BTreeSet<String>>::new();

    for item in &matrix.classification {
        assert!(
            documented.contains_key(item.protocol.as_str()),
            "unknown option protocol {}",
            item.protocol
        );
        assert!(
            !item.options.is_empty(),
            "empty {} classification",
            item.protocol
        );
        assert!(
            !item.reason.trim().is_empty(),
            "missing reason for {}",
            item.protocol
        );
        assert!(
            matches!(
                item.status.as_str(),
                "implemented" | "native-default" | "process-config" | "deferred-v2" | "removed-v1"
            ),
            "invalid status {}",
            item.status
        );
        let protocol_options = classified.entry(item.protocol.clone()).or_default();
        for option in &item.options {
            assert!(
                protocol_options.insert(option.clone()),
                "duplicate {} option {option}",
                item.protocol
            );
        }

        match item.status.as_str() {
            "implemented" => {
                assert!(item.configuration.is_empty());
                execute_recipe(item);
            }
            "process-config" => execute_configuration(&root, item),
            _ => {
                assert!(item.configuration.is_empty());
                assert!(
                    item.rsproxy.is_none(),
                    "{} non-implemented recipe",
                    item.protocol
                );
                assert!(
                    item.expect_actions.is_empty(),
                    "{} non-implemented actions",
                    item.protocol
                );
            }
        }
    }

    for (protocol, expected) in documented {
        let actual = classified.remove(protocol).unwrap_or_default();
        assert_eq!(actual, expected, "{protocol} option classification drift");
    }
    assert!(
        classified.is_empty(),
        "unexpected protocols: {classified:?}"
    );
}

fn execute_configuration(root: &Path, item: &OptionClassification) {
    assert!(item.rsproxy.is_none(), "{} process recipe", item.protocol);
    assert!(
        item.expect_actions.is_empty(),
        "{} process actions",
        item.protocol
    );
    assert!(
        !item.configuration.is_empty(),
        "{} process configuration is empty",
        item.protocol
    );
    let help = ["command.rs", "daemon.rs"]
        .into_iter()
        .map(|file| fs::read_to_string(root.join("crates/rsproxy-cli/src/cli").join(file)).unwrap())
        .collect::<String>();
    let mut seen = BTreeSet::new();
    for option in &item.configuration {
        assert!(
            option.starts_with("--") && seen.insert(option),
            "invalid or duplicate process option {option}"
        );
        assert!(help.contains(option), "missing process option {option}");
    }
}

fn execute_recipe(item: &OptionClassification) {
    let recipe = item
        .rsproxy
        .as_deref()
        .unwrap_or_else(|| panic!("missing implemented recipe for {}", item.protocol));
    assert!(!item.expect_actions.is_empty());
    let rules = RuleSet::parse("whistle-options", recipe)
        .unwrap_or_else(|errors| panic!("{} {recipe}: {errors:?}", item.protocol));
    let response = ResponseMeta {
        status: 301,
        headers: Vec::new(),
    };
    let actual = rules
        .resolve_response(&request(), &response)
        .actions
        .iter()
        .map(|item| item.action.family().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        actual, item.expect_actions,
        "{} recipe {recipe}",
        item.protocol
    );
}

fn documented_bullet_options(protocol: &str) -> BTreeSet<String> {
    let source = whistle_fixture::read(&format!("docs/en/docs/rules/{protocol}.md"));
    let mut options = BTreeSet::new();
    for line in source.lines() {
        let Some(bullet) = line.strip_prefix("- ") else {
            continue;
        };
        let prefix = bullet
            .split_once(':')
            .map(|(value, _)| value)
            .unwrap_or(bullet);
        for option in inline_code(prefix) {
            options.insert(option.to_string());
        }
    }
    options
}

fn documented_delete_options() -> BTreeSet<String> {
    let source = whistle_fixture::read("docs/en/docs/rules/delete.md");
    let row = source
        .lines()
        .find(|line| line.starts_with("| value |"))
        .expect("delete option table row");
    inline_code(row)
        .filter_map(|value| match value {
            "pathname" | "pathname.index" | "urlParams" | "reqBody" | "resBody" | "reqType"
            | "resType" | "reqCharset" | "resCharset" => Some(value),
            "urlParams.xxx" => Some("urlParams.*"),
            "reqHeaders.xxx" => Some("reqHeaders.*"),
            "resHeaders.xxx" => Some("resHeaders.*"),
            "reqBody.xxx.yyy" => Some("reqBody.*"),
            "resBody.xxx.yyy" => Some("resBody.*"),
            "reqCookies.xxx" => Some("reqCookies.*"),
            "resCookies.xxx" => Some("resCookies.*"),
            _ => None,
        })
        .map(str::to_string)
        .collect()
}

fn inline_code(input: &str) -> impl Iterator<Item = &str> {
    input.split('`').skip(1).step_by(2)
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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/contracts/whistle_options.toml")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}
