use rsproxy_rules::{Action, RequestMeta, ResponseMeta, RuleSet};
use serde::Deserialize;
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct CorpusFile {
    #[serde(default)]
    required_action_families: Vec<String>,
    case: Vec<CorpusCase>,
}

#[derive(Deserialize)]
struct CorpusCase {
    id: String,
    #[serde(default)]
    spec: bool,
    rules: Option<String>,
    group: Option<String>,
    #[serde(default)]
    groups: Vec<CorpusGroup>,
    request: Option<CorpusRequest>,
    response: Option<CorpusResponse>,
    #[serde(default)]
    expect_actions: Vec<String>,
    #[serde(default)]
    expect_rules: Vec<String>,
    #[serde(default)]
    expect_explain: Vec<String>,
    #[serde(default)]
    error_codes: Vec<String>,
    #[serde(default)]
    error_groups: Vec<String>,
    #[serde(default)]
    error_lines: Vec<usize>,
}

#[derive(Deserialize)]
struct CorpusGroup {
    name: String,
    text: String,
}

#[derive(Default, Deserialize)]
struct CorpusRequest {
    #[serde(default = "default_method")]
    method: String,
    url: String,
    #[serde(default)]
    headers: Vec<[String; 2]>,
    #[serde(default)]
    body: String,
    client_ip: Option<String>,
    server_ip: Option<String>,
}

#[derive(Deserialize)]
struct CorpusResponse {
    status: u16,
    #[serde(default)]
    headers: Vec<[String; 2]>,
}

#[test]
fn rules_corpus_matches_the_public_contract_and_spec_anchors() {
    let files = corpus_files();
    assert!(!files.is_empty(), "rules corpus directory is empty");
    let mut seen = HashSet::new();
    let mut spec_cases = BTreeSet::new();

    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        let corpus: CorpusFile = toml::from_str(&text)
            .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
        assert!(!corpus.case.is_empty(), "{} has no cases", path.display());
        validate_action_coverage(&path, &corpus);
        for case in corpus.case {
            assert!(
                seen.insert(case.id.clone()),
                "duplicate case id {}",
                case.id
            );
            if case.spec {
                spec_cases.insert(case.id.clone());
            }
            run_case(case);
        }
    }

    let spec = fs::read_to_string(spec_path()).unwrap();
    let anchors = spec_anchors(&spec);
    assert_eq!(anchors, spec_cases, "DSL spec/corpus anchors differ");
}

fn validate_action_coverage(path: &Path, corpus: &CorpusFile) {
    if corpus.required_action_families.is_empty() {
        return;
    }
    let required = corpus
        .required_action_families
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        required.len(),
        corpus.required_action_families.len(),
        "{} repeats a required action family",
        path.display()
    );
    let implemented = Action::FAMILIES.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(
        implemented.len(),
        Action::FAMILIES.len(),
        "public action-family contract contains duplicates"
    );
    assert_eq!(
        required,
        implemented,
        "{} action-family declaration differs from the public contract",
        path.display()
    );
    let observed = corpus
        .case
        .iter()
        .flat_map(|case| case.expect_actions.iter().map(String::as_str))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        observed,
        implemented,
        "{} does not exercise every action family",
        path.display()
    );
}

fn run_case(case: CorpusCase) {
    let groups = case_groups(&case);
    let parsed = RuleSet::parse_groups(
        groups
            .iter()
            .map(|group| (group.name.as_str(), group.text.as_str())),
    );
    if !case.error_codes.is_empty() {
        let errors = parsed.unwrap_err();
        assert_eq!(
            errors
                .iter()
                .map(|error| error.code.as_str())
                .collect::<Vec<_>>(),
            case.error_codes,
            "{} error codes",
            case.id
        );
        if !case.error_groups.is_empty() {
            assert_eq!(
                errors
                    .iter()
                    .map(|error| error.group.as_str())
                    .collect::<Vec<_>>(),
                case.error_groups,
                "{} error groups",
                case.id
            );
        }
        if !case.error_lines.is_empty() {
            assert_eq!(
                errors.iter().map(|error| error.line).collect::<Vec<_>>(),
                case.error_lines,
                "{} error lines",
                case.id
            );
        }
        return;
    }

    let rules = parsed.unwrap_or_else(|errors| panic!("{}: {errors:?}", case.id));
    let request = case
        .request
        .as_ref()
        .unwrap_or_else(|| panic!("{} has no request", case.id))
        .to_meta();
    let result = match &case.response {
        Some(response) => rules.resolve_response(&request, &response.to_meta()),
        None => rules.resolve(&request),
    };
    assert_eq!(
        result
            .actions
            .iter()
            .map(|action| action.action.family().to_string())
            .collect::<Vec<_>>(),
        case.expect_actions,
        "{} actions",
        case.id
    );
    assert_eq!(
        result
            .matched_rules
            .iter()
            .map(|rule| format!("{}:{}", rule.group, rule.line))
            .collect::<Vec<_>>(),
        case.expect_rules,
        "{} matched rules",
        case.id
    );
    if !case.expect_explain.is_empty() {
        let explain = match &case.response {
            Some(response) => rules.explain_response(&request, &response.to_meta()),
            None => rules.explain(&request),
        };
        assert_eq!(
            explain.lines().collect::<Vec<_>>(),
            case.expect_explain,
            "{} explain",
            case.id
        );
    }
}

fn case_groups(case: &CorpusCase) -> Vec<CorpusGroup> {
    if !case.groups.is_empty() {
        assert!(case.rules.is_none(), "{} mixes rules and groups", case.id);
        return case
            .groups
            .iter()
            .map(|group| CorpusGroup {
                name: group.name.clone(),
                text: group.text.clone(),
            })
            .collect();
    }
    vec![CorpusGroup {
        name: case.group.clone().unwrap_or_else(|| "default".to_string()),
        text: case
            .rules
            .clone()
            .unwrap_or_else(|| panic!("{} has no rules", case.id)),
    }]
}

impl CorpusRequest {
    fn to_meta(&self) -> RequestMeta {
        RequestMeta {
            method: self.method.clone(),
            url: self.url.clone(),
            headers: pairs(&self.headers),
            body: self.body.as_bytes().to_vec(),
            client_ip: self.client_ip.clone(),
            server_ip: self.server_ip.clone(),
            template: Default::default(),
        }
    }
}

impl CorpusResponse {
    fn to_meta(&self) -> ResponseMeta {
        ResponseMeta {
            status: self.status,
            headers: pairs(&self.headers),
        }
    }
}

fn pairs(values: &[[String; 2]]) -> Vec<(String, String)> {
    values
        .iter()
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect()
}

fn corpus_files() -> Vec<PathBuf> {
    let mut files = fs::read_dir(corpus_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("toml"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

fn spec_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/rules-dsl-spec.md")
}

fn spec_anchors(spec: &str) -> BTreeSet<String> {
    spec.split("<!-- corpus:")
        .skip(1)
        .filter_map(|tail| tail.split_once(" -->").map(|(id, _)| id.trim().to_string()))
        .collect()
}

fn default_method() -> String {
    "GET".to_string()
}
