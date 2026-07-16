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
use aho_corasick::AhoCorasick;
use fancy_regex::{
    Error as FancyError, Regex as FancyRegex, RegexBuilder as FancyRegexBuilder, RuntimeError,
};
use regex::{Regex as LinearRegex, RegexBuilder as LinearRegexBuilder};
use std::collections::{BTreeMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;

const DEFAULT_FANCY_BACKTRACK_LIMIT: usize = 100_000;

mod action;
mod error;
mod explain;
mod index;
mod lint;
mod matcher;
mod matching;
mod model;
mod parser;
mod planning;
mod redact;
mod resolve;
mod template;

pub use action::{
    Action, BodyOp, CacheDirective, CacheOp, CookieAttr, CookieOp, CorsOp, DeleteBodyPath,
    DeleteBodyPathSegment, DeleteOp, DeletePathSegment, HeaderOp, HostPool, InjectMode, InjectOp,
    InjectTarget, MockInlineOp, Phase, QueryOp, RegexReplacePattern, TlsCipherSuite, TlsMinVersion,
    TlsOp, UrlRewritePattern, Value, valid_value_key,
};
pub use error::{RuleError, RuleErrorCode, RuleModelError};
pub use lint::LintFinding;
pub use matcher::UrlParts;
pub use model::{
    Captures, Condition, GlobMatcher, MatchedRule, Matcher, RegexEngine, RegexMatcher, RequestMeta,
    ResolveResult, ResolvedAction, ResponseMeta, Rule, RuleSet, RuleSetStats, UrlCondition,
};
pub use redact::redact_secrets;
pub use template::TemplateMetadata;

pub(crate) use model::{CompiledRegex, RuleIndex};

use explain::*;
use index::*;
use matching::*;
use parser::*;
use template::now_millis;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
