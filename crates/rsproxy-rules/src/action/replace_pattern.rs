use regex::{Regex, RegexBuilder};
use std::fmt;
use std::sync::Arc;

#[derive(Clone)]
/// A Rust regular expression compiled once for repeated replacement operations.
pub struct RegexReplacePattern {
    pattern: String,
    case_insensitive: bool,
    compiled: Arc<Regex>,
}

impl RegexReplacePattern {
    /// Compiles a pattern with the requested case-sensitivity, returning regex syntax errors.
    pub fn new(pattern: String, case_insensitive: bool) -> Result<Self, regex::Error> {
        let compiled = RegexBuilder::new(&pattern)
            .case_insensitive(case_insensitive)
            .build()?;
        Ok(Self {
            pattern,
            case_insensitive,
            compiled: Arc::new(compiled),
        })
    }

    /// Replaces every non-overlapping match using Rust-regex capture expansion.
    pub fn replace_all(&self, input: &str, replacement: &str) -> String {
        self.compiled.replace_all(input, replacement).into_owned()
    }

    /// Replaces all matches only when capture expansion fits `limit` UTF-8 bytes.
    pub fn replace_all_bounded(
        &self,
        input: &str,
        replacement: &str,
        limit: usize,
    ) -> Result<String, crate::RuleModelError> {
        crate::bounded_replace::regex_replace_all(
            &self.compiled,
            input,
            replacement,
            limit,
            "regex replacement",
        )
    }

    /// Returns pattern text without DSL slash delimiters or flags.
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Reports whether the compiled matcher ignores case.
    pub fn is_case_insensitive(&self) -> bool {
        self.case_insensitive
    }

    pub(crate) fn display(&self) -> String {
        if self.case_insensitive {
            format!("/{}/i", self.pattern)
        } else {
            format!("/{}/", self.pattern)
        }
    }
}

impl fmt::Debug for RegexReplacePattern {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RegexReplacePattern")
            .field("pattern", &self.pattern)
            .field("case_insensitive", &self.case_insensitive)
            .finish()
    }
}

impl PartialEq for RegexReplacePattern {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern && self.case_insensitive == other.case_insensitive
    }
}

impl Eq for RegexReplacePattern {}
