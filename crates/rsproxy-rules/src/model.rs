use crate::{Action, TemplateMetadata};
use aho_corasick::AhoCorasick;
use fancy_regex::Regex as FancyRegex;
use regex::Regex as LinearRegex;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleSet {
    pub version: u64,
    pub rules: Vec<Rule>,
    pub(crate) index: RuleIndex,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RuleIndex {
    pub(crate) domain_exact: BTreeMap<String, Vec<usize>>,
    pub(crate) domain_suffix: BTreeMap<String, Vec<usize>>,
    pub(crate) global: Vec<usize>,
    pub(crate) prefilter_literals: Vec<String>,
    pub(crate) prefilter_literal_rules: Vec<Vec<usize>>,
    pub(crate) prefilter_rule_ids: HashSet<usize>,
    pub(crate) prefilter: Option<AhoCorasick>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleSetStats {
    pub rules: usize,
    pub disabled: usize,
    pub domain_exact_entries: usize,
    pub domain_suffix_entries: usize,
    pub indexed_rules: usize,
    pub global_rules: usize,
    pub prefilter_literals: usize,
    pub prefilter_rules: usize,
}

impl PartialEq for RuleIndex {
    fn eq(&self, other: &Self) -> bool {
        self.domain_exact == other.domain_exact
            && self.domain_suffix == other.domain_suffix
            && self.global == other.global
            && self.prefilter_literals == other.prefilter_literals
            && self.prefilter_literal_rules == other.prefilter_literal_rules
            && self.prefilter_rule_ids == other.prefilter_rule_ids
    }
}

impl Eq for RuleIndex {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub group: String,
    pub line: usize,
    pub raw: String,
    pub matcher: Matcher,
    pub actions: Vec<Action>,
    pub conditions: Vec<Condition>,
    pub important: bool,
    pub disabled: bool,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Matcher {
    ExactUrl(String),
    Glob(GlobMatcher),
    Port(u16),
    Regex(RegexMatcher),
    Not(Box<Matcher>),
}

#[derive(Clone, Debug)]
pub struct RegexMatcher {
    pub pattern: String,
    pub case_insensitive: bool,
    pub engine: RegexEngine,
    pub(crate) compiled: Arc<CompiledRegex>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegexEngine {
    Linear,
    Fancy,
}

#[derive(Debug)]
pub(crate) enum CompiledRegex {
    Linear(LinearRegex),
    Fancy(FancyRegex),
}

impl PartialEq for RegexMatcher {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern
            && self.case_insensitive == other.case_insensitive
            && self.engine == other.engine
    }
}

impl Eq for RegexMatcher {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobMatcher {
    pub scheme: Option<String>,
    pub host: String,
    pub port: Option<String>,
    pub path: Option<String>,
    pub query: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Condition {
    Method(Vec<String>),
    Host(String),
    Url(UrlCondition),
    ClientIp(Vec<String>),
    ServerIp(Vec<String>),
    HeaderPresent(String),
    HeaderContains { name: String, value: String },
    ResHeaderPresent(String),
    ResHeaderContains { name: String, value: String },
    BodyContains(String),
    BodyRegex(RegexMatcher),
    Status(Vec<u16>),
    ChancePermille(u16),
    EnvPresent(String),
    EnvEquals { name: String, value: String },
    Any(Vec<Condition>),
    Not(Box<Condition>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UrlCondition {
    Glob(String),
    Regex(RegexMatcher),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestMeta {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub client_ip: Option<String>,
    pub server_ip: Option<String>,
    pub template: TemplateMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResponseMeta {
    pub status: u16,
    pub headers: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveResult {
    pub actions: Vec<ResolvedAction>,
    pub matched_rules: Vec<MatchedRule>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedAction {
    pub action: Action,
    pub rule: MatchedRule,
    pub captures: Captures,
    pub(crate) response: Option<Arc<ResponseMeta>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchedRule {
    pub group: String,
    pub line: usize,
    pub raw: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Captures {
    pub(crate) whole: Option<String>,
    pub(crate) indexed: Vec<String>,
    pub(crate) named: BTreeMap<String, String>,
}
