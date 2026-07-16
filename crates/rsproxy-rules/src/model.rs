use crate::{Action, TemplateMetadata};
use aho_corasick::AhoCorasick;
use fancy_regex::Regex as FancyRegex;
use regex::Regex as LinearRegex;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
/// An immutable, validated rules snapshot with an internal candidate index.
///
/// Construct snapshots through [`RuleSet::parse`] or [`RuleSet::parse_groups`]
/// so matcher compilation and ordering invariants are established together.
pub struct RuleSet {
    /// Unix-millisecond publication identifier assigned when the snapshot is built.
    pub version: u64,
    /// Parsed rules in group order and then source-line order.
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
/// Observable shape of a [`RuleSet`]'s compiled candidate index.
///
/// These counts diagnose indexing effectiveness; they do not change resolution
/// order or semantics.
pub struct RuleSetStats {
    /// Total parsed rules, including disabled rules.
    pub rules: usize,
    /// Rules retained for inspection but excluded from resolution.
    pub disabled: usize,
    /// Distinct exact-host buckets in the domain index.
    pub domain_exact_entries: usize,
    /// Distinct suffix-host buckets in the domain index.
    pub domain_suffix_entries: usize,
    /// Rules reachable through an exact-host or suffix-host bucket.
    pub indexed_rules: usize,
    /// Rules evaluated from the global bucket because they cannot use a host bucket.
    pub global_rules: usize,
    /// Required literals compiled into the Aho-Corasick regex prefilter.
    pub prefilter_literals: usize,
    /// Regex rules guarded by at least one required-literal prefilter entry.
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
/// One source rule after DSL parsing and model validation.
pub struct Rule {
    /// Caller-supplied group name used in diagnostics and cross-group ordering.
    pub group: String,
    /// One-based source line within the group.
    pub line: usize,
    /// Comment-free rule text retained for explain and trace output.
    pub raw: String,
    /// URL matcher that admits the rule and produces captures.
    pub matcher: Matcher,
    /// Validated actions in their source order.
    pub actions: Vec<Action>,
    /// Conditions ANDed after the matcher succeeds.
    pub conditions: Vec<Condition>,
    /// Whether this rule is ordered before non-important rules across the snapshot.
    pub important: bool,
    /// Whether the rule is retained but never considered during resolution.
    pub disabled: bool,
    /// Source metadata from `@tag:` modifiers; separate from runtime [`Action::Tag`].
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A compiled URL-selection strategy for one rule.
pub enum Matcher {
    /// Matches scheme, host, effective port, and path exactly; an omitted query is unconstrained.
    ExactUrl(String),
    /// Matches structured scheme/host/port/path/query glob components.
    Glob(GlobMatcher),
    /// Matches only the URL's explicit or scheme-default effective port.
    Port(u16),
    /// Matches the complete raw URL and exposes numbered and named captures.
    Regex(RegexMatcher),
    /// Matches when the nested matcher does not, without producing nested captures.
    Not(Box<Matcher>),
}

#[derive(Clone, Debug)]
/// A validated regex matcher and the engine selected for it.
///
/// Linear regexes use the Rust `regex` engine. Patterns requiring lookaround or
/// backreferences fall back to `fancy-regex` with a hard backtrack limit;
/// exceeding that limit is treated as no match.
pub struct RegexMatcher {
    /// Pattern text without the DSL's slash delimiters.
    pub pattern: String,
    /// Whether matching uses Unicode-aware case-insensitive mode.
    pub case_insensitive: bool,
    /// Engine selected when the matcher was compiled.
    pub engine: RegexEngine,
    pub(crate) compiled: Arc<CompiledRegex>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Regex implementation selected for a [`RegexMatcher`].
pub enum RegexEngine {
    /// Rust `regex`, providing bounded linear-time matching.
    Linear,
    /// `fancy-regex`, used for advanced constructs under a backtrack limit.
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
/// Structured components of a domain, glob, or path-prefix matcher.
pub struct GlobMatcher {
    /// Optional lowercased scheme requirement.
    pub scheme: Option<String>,
    /// Normalized host pattern, including `*` or `**` label wildcards when present.
    pub host: String,
    /// Optional effective-port glob matched after scheme defaults are applied.
    pub port: Option<String>,
    /// Optional path prefix or glob; `*` stays within a segment and `**` crosses segments.
    pub path: Option<String>,
    /// Optional query glob matched against the query without the leading `?`.
    pub query: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A validated predicate evaluated after a rule's URL matcher succeeds.
///
/// Multiple values inside one variant are ORed; separate values in a rule's
/// [`Rule::conditions`] list are ANDed.
pub enum Condition {
    /// Case-insensitively matches any listed HTTP method.
    Method(Vec<String>),
    /// Matches the request host using the DSL's host-glob semantics.
    Host(String),
    /// Matches the complete raw request URL with a glob or regex.
    Url(UrlCondition),
    /// Matches the normalized client IP against any exact or simple-glob pattern.
    ClientIp(Vec<String>),
    /// Matches the normalized target IP against any exact or simple-glob pattern.
    ServerIp(Vec<String>),
    /// Requires a request header to be present, using a case-insensitive field name.
    HeaderPresent(String),
    /// Requires a request header value to contain a case-insensitive substring.
    HeaderContains {
        /// Case-insensitive request-header name.
        name: String,
        /// Non-empty substring tested case-insensitively.
        value: String,
    },
    /// Requires a response header; it never matches without response metadata.
    ResHeaderPresent(String),
    /// Requires a response header value to contain a case-insensitive substring.
    ResHeaderContains {
        /// Case-insensitive response-header name.
        name: String,
        /// Non-empty substring tested case-insensitively.
        value: String,
    },
    /// Case-insensitively searches the lossy UTF-8 request body for a substring.
    BodyContains(String),
    /// Matches a regex against the lossy UTF-8 request body.
    BodyRegex(RegexMatcher),
    /// Matches any listed response status; it never matches without response metadata.
    Status(Vec<u16>),
    /// Deterministically samples the request and rule line in thousandths.
    ChancePermille(u16),
    /// Requires the named process environment variable to exist.
    EnvPresent(String),
    /// Requires an exact value for a process environment variable.
    EnvEquals {
        /// Environment variable name.
        name: String,
        /// Case-sensitive value required from the process environment.
        value: String,
    },
    /// Matches when at least one nested condition matches.
    Any(Vec<Condition>),
    /// Matches only when every nested condition matches.
    All(Vec<Condition>),
    /// Inverts a nested condition, except absent response metadata stays non-matching.
    Not(Box<Condition>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Full-URL predicate used by [`Condition::Url`].
pub enum UrlCondition {
    /// Glob matched against the complete raw URL.
    Glob(String),
    /// Regex matched against the complete raw URL.
    Regex(RegexMatcher),
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Immutable request snapshot used for matching and template rendering.
pub struct RequestMeta {
    /// HTTP method; method conditions compare it case-insensitively.
    pub method: String,
    /// Absolute raw URL used by URL matchers and `${url}` templates.
    pub url: String,
    /// Request fields in wire order; duplicate names are preserved.
    pub headers: Vec<(String, String)>,
    /// Request body bytes; body conditions interpret invalid UTF-8 lossily.
    pub body: Vec<u8>,
    /// Optional peer address or IP exposed to client-IP conditions and templates.
    pub client_ip: Option<String>,
    /// Optional target address or IP exposed to server-IP conditions and templates.
    pub server_ip: Option<String>,
    /// Per-request stable values for id, time, random, and UUID templates.
    pub template: TemplateMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Immutable upstream-response snapshot for response-phase conditions and templates.
pub struct ResponseMeta {
    /// HTTP response status code.
    pub status: u16,
    /// Response fields in wire order; duplicate names are preserved.
    pub headers: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Ordered output of one request- or response-phase resolution.
pub struct ResolveResult {
    /// Applicable actions after first-match and `skip` family rules are enforced.
    pub actions: Vec<ResolvedAction>,
    /// Source rules that contributed at least one returned action, without duplicates.
    pub matched_rules: Vec<MatchedRule>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// One applicable action paired with source provenance and matcher captures.
pub struct ResolvedAction {
    /// Typed operation to execute.
    pub action: Action,
    /// Group, line, and source text that produced the operation.
    pub rule: MatchedRule,
    /// Captures produced by this rule's matcher and used when rendering values.
    pub captures: Captures,
    pub(crate) response: Option<Arc<ResponseMeta>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Compact source provenance for a matched action.
pub struct MatchedRule {
    /// Caller-supplied rules group.
    pub group: String,
    /// One-based line number within the group.
    pub line: usize,
    /// Comment-free DSL source retained for diagnostics and trace explanations.
    pub raw: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
/// Matcher captures available to `$0`-`$9`, `${name}`, and action templates.
///
/// Glob wildcards populate numbered captures. Regex matchers additionally retain
/// the complete match and named captures; unmatched numbered groups render empty.
pub struct Captures {
    pub(crate) whole: Option<String>,
    pub(crate) indexed: Vec<String>,
    pub(crate) named: BTreeMap<String, String>,
}
