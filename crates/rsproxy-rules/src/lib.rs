//! Parses the rsproxy rules DSL and resolves matching actions for proxy requests.
//!
//! A [`RuleSet`] is an immutable, indexed snapshot. Parsing validates matcher,
//! condition, action, template, and value-source syntax before the snapshot is
//! published, so request-time resolution does not reinterpret DSL text. Use
//! [`RuleSet::resolve`] for request-phase matching and
//! [`RuleSet::resolve_response`] when response-dependent conditions are available.
//!
//! Resolution preserves group and source order, except that `@important` rules
//! precede ordinary rules. Single-action families take their first match, while
//! stackable families retain every applicable action. The returned
//! [`ResolvedAction`] carries its source rule and matcher captures so values can
//! be rendered against the same request metadata that was matched.
//!
//! This crate owns rule semantics only. It does not read value files, mutate HTTP
//! messages, open network connections, or execute delays and throttles; callers
//! perform those effects after resolving and rendering the typed action model.
use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use fancy_regex::{
    Error as FancyError, Regex as FancyRegex, RegexBuilder as FancyRegexBuilder, RuntimeError,
};
use regex::{Regex as LinearRegex, RegexBuilder as LinearRegexBuilder};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;

const DEFAULT_FANCY_BACKTRACK_LIMIT: usize = 100_000;

mod action;
mod bounded_replace;
mod error;
mod explain;
mod family;
mod index;
mod language;
mod lint;
mod matcher;
mod matching;
mod migration;
mod model;
mod parser;
mod planning;
mod redact;
mod resolution_api;
mod resolve;
mod semantic_lint;
mod snapshot_support;
mod template;

pub use action::{
    Action, BodyOp, CacheDirective, CacheOp, CookieAttr, CookieOp, CorsOp, DeleteBodyPath,
    DeleteBodyPathSegment, DeleteOp, DeletePathSegment, HeaderOp, HostPool, InjectMode, InjectOp,
    InjectTarget, MockInlineOp, Phase, QueryOp, RegexReplacePattern, TlsCipherSuite, TlsMinVersion,
    TlsOp, UrlRewritePattern, Value, valid_value_key,
};
pub use error::{RuleError, RuleErrorCode, RuleModelError, RuleSourceSpan};
pub use language::{
    ACTION_SYNTAX, CONDITION_SYNTAX, MATCHER_SYNTAX, MAX_HTTP_STATUS, MAX_RULE_ACTIONS_PER_RULE,
    MAX_RULE_ACTIONS_PER_SNAPSHOT, MAX_RULE_BODY_CONDITIONS_PER_SNAPSHOT, MAX_RULE_CALL_ARGUMENTS,
    MAX_RULE_CONDITION_NODES_PER_RULE, MAX_RULE_CONDITION_NODES_PER_SNAPSHOT, MAX_RULE_DIAGNOSTICS,
    MAX_RULE_EXPLAIN_BYTES, MAX_RULE_EXPLAIN_VALUE_BYTES, MAX_RULE_EXTERNAL_PATH_BYTES,
    MAX_RULE_EXTERNAL_VALUE_BYTES, MAX_RULE_GLOB_CAPTURES, MAX_RULE_GROUP_NAME_BYTES,
    MAX_RULE_GROUPS_PER_SNAPSHOT, MAX_RULE_LINT_COMPARISON_BYTES, MAX_RULE_LINT_COMPARISONS,
    MAX_RULE_LINT_FINDINGS, MAX_RULE_LINT_REPORT_BYTES, MAX_RULE_MOCK_FILE_CANDIDATES,
    MAX_RULE_PARSE_NESTING, MAX_RULE_PROPERTIES_PER_RULE, MAX_RULE_RENDERED_TAG_BYTES,
    MAX_RULE_RENDERED_VALUE_BYTES, MAX_RULE_SNAPSHOT_SOURCE_BYTES, MAX_RULE_SOURCE_LINE_BYTES,
    MAX_RULE_TAGS_PER_REQUEST, MAX_RULE_TLS_PEM_BYTES, MAX_RULE_UPSTREAM_HOPS,
    MAX_RULES_PER_SNAPSHOT, MIN_FINAL_HTTP_STATUS, MIN_HTTP_STATUS, PROPERTY_SYNTAX,
    REDIRECT_STATUSES, RULE_LANGUAGE_HEADER, RULE_LANGUAGE_VERSION, RuleSyntaxSpelling,
    canonical_action_name, canonical_condition_name, canonical_property_name, is_http_token_byte,
};
pub use lint::{LintFinding, LintReport};
pub use matcher::{
    ActionFamily, ActionFamilySet, ResolutionPolicy, UrlParts, validate_redirect_location,
};
pub use migration::migrate_rule_source_v3;
pub use model::{
    Captures, Condition, GlobMatcher, MatchedRule, Matcher, RegexEngine, RegexMatcher, RequestMeta,
    ResolveResult, ResolvedAction, ResponseMeta, Rule, RuleSet, RuleSetStats, UrlCondition,
};
pub use redact::redact_secrets;
pub use semantic_lint::{SemanticLintFinding, SemanticLintKind, SemanticLintReport};
pub use template::TemplateMetadata;

pub(crate) use model::{CompiledRegex, RuleIndex};

use explain::*;
use family::*;
use index::*;
use matching::*;
use parser::*;
use resolution_api::action_for_body_availability;
use snapshot_support::*;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
