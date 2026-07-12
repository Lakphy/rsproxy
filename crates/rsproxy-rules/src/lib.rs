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
mod matcher;
mod matching;
mod model;
mod parser;
mod planning;
mod redact;
mod resolve;
mod template;

pub use action::*;
pub use error::{RuleError, RuleErrorCode};
pub use matcher::UrlParts;
pub use model::*;
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
