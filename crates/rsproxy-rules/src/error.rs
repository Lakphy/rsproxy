use std::num::{ParseFloatError, ParseIntError};
use thiserror::Error;

/// A structured error produced while constructing or parsing rule model values.
///
/// [`RuleSet::parse`](crate::RuleSet::parse) continues to expose DSL diagnostics as
/// [`RuleError`] values. The parser preserves the category and location of those
/// diagnostics and uses this error's [`Display`](std::fmt::Display) output as the
/// human-readable message.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RuleModelError {
    /// A required value was present syntactically but contained no content.
    #[error("{message}")]
    EmptyInput {
        /// Model component that rejected the value.
        context: &'static str,
        /// Stable human-facing explanation suitable for a [`RuleError`] message.
        message: String,
    },

    /// A required argument or paired option was omitted.
    #[error("{message}")]
    MissingArgument {
        /// Model component that required the argument.
        context: &'static str,
        /// Human-facing explanation of the missing input.
        message: String,
    },

    /// Input could not be tokenized or interpreted according to its local grammar.
    #[error("{message}")]
    InvalidSyntax {
        /// Grammar component that rejected the input.
        context: &'static str,
        /// Human-facing syntax diagnostic.
        message: String,
    },

    /// Input had the correct shape but an invalid value.
    #[error("{message}")]
    InvalidInput {
        /// Model component that validated the value.
        context: &'static str,
        /// Human-facing value diagnostic.
        message: String,
    },

    /// Syntactically valid input requested a feature outside the supported DSL contract.
    #[error("{message}")]
    UnsupportedInput {
        /// Model component whose supported set was exceeded.
        context: &'static str,
        /// Human-facing unsupported-feature diagnostic.
        message: String,
    },

    /// Individually valid values violated a cross-field invariant.
    #[error("{message}")]
    ConstraintViolation {
        /// Model component enforcing the invariant.
        context: &'static str,
        /// Human-facing explanation of the violated invariant.
        message: String,
    },

    /// Input exceeded a documented size or complexity bound.
    #[error("{message}")]
    LimitExceeded {
        /// Bounded model component.
        context: &'static str,
        /// Human-facing limit diagnostic.
        message: String,
    },

    /// A numeric token could not be parsed as the required integer type.
    #[error("{message}")]
    InvalidInteger {
        /// Numeric option or field being parsed.
        context: &'static str,
        /// Original numeric token.
        input: String,
        /// Human-facing parse diagnostic.
        message: String,
        /// Integer parser error retained for diagnostic chaining.
        #[source]
        source: ParseIntError,
    },

    /// A numeric token could not be parsed as the required floating-point type.
    #[error("{message}")]
    InvalidFloat {
        /// Numeric option or field being parsed.
        context: &'static str,
        /// Original numeric token.
        input: String,
        /// Human-facing parse diagnostic.
        message: String,
        /// Floating-point parser error retained for diagnostic chaining.
        #[source]
        source: ParseFloatError,
    },

    /// A Rust `regex` pattern failed compilation.
    #[error("{context}: {source}")]
    InvalidRegex {
        /// Matcher or action slot that owns the pattern.
        context: &'static str,
        /// Regex compiler error retained for diagnostic chaining.
        #[source]
        source: Box<regex::Error>,
    },

    /// A matcher pattern was rejected by both linear and fancy regex engines.
    #[error("invalid regex matcher: regex={linear}; fancy-regex={fancy}")]
    InvalidRegexMatcher {
        /// Failure from the preferred linear engine.
        linear: Box<regex::Error>,
        /// Failure from the advanced fallback engine.
        #[source]
        fancy: Box<fancy_regex::Error>,
    },

    /// An exact-URL matcher failed strict URL validation.
    #[error("invalid exact URL matcher: {source}")]
    InvalidExactUrlMatcher {
        /// Underlying structured-URL diagnostic.
        #[source]
        source: Box<RuleModelError>,
    },

    /// A template or template transform was malformed or failed validation.
    #[error("{message}")]
    InvalidTemplate {
        /// Action slot containing the template.
        context: &'static str,
        /// Human-facing template diagnostic.
        message: String,
    },
}

impl RuleModelError {
    pub(crate) fn empty(context: &'static str, message: impl Into<String>) -> Self {
        Self::EmptyInput {
            context,
            message: message.into(),
        }
    }

    pub(crate) fn missing(context: &'static str, message: impl Into<String>) -> Self {
        Self::MissingArgument {
            context,
            message: message.into(),
        }
    }

    pub(crate) fn syntax(context: &'static str, message: impl Into<String>) -> Self {
        Self::InvalidSyntax {
            context,
            message: message.into(),
        }
    }

    pub(crate) fn invalid(context: &'static str, message: impl Into<String>) -> Self {
        Self::InvalidInput {
            context,
            message: message.into(),
        }
    }

    pub(crate) fn unsupported(context: &'static str, message: impl Into<String>) -> Self {
        Self::UnsupportedInput {
            context,
            message: message.into(),
        }
    }

    pub(crate) fn constraint(context: &'static str, message: impl Into<String>) -> Self {
        Self::ConstraintViolation {
            context,
            message: message.into(),
        }
    }

    pub(crate) fn limit(context: &'static str, message: impl Into<String>) -> Self {
        Self::LimitExceeded {
            context,
            message: message.into(),
        }
    }

    pub(crate) fn integer(
        context: &'static str,
        input: impl Into<String>,
        message: impl Into<String>,
        source: ParseIntError,
    ) -> Self {
        Self::InvalidInteger {
            context,
            input: input.into(),
            message: message.into(),
            source,
        }
    }

    pub(crate) fn float(
        context: &'static str,
        input: impl Into<String>,
        message: impl Into<String>,
        source: ParseFloatError,
    ) -> Self {
        Self::InvalidFloat {
            context,
            input: input.into(),
            message: message.into(),
            source,
        }
    }

    pub(crate) fn template(context: &'static str, message: impl Into<String>) -> Self {
        Self::InvalidTemplate {
            context,
            message: message.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Stable machine-readable category for a DSL parse diagnostic.
///
/// Callers may persist or branch on these values; [`RuleError::message`] remains
/// human-facing and is not a stable parsing contract.
pub enum RuleErrorCode {
    /// Tokenization or overall rule-shape failure.
    Syntax,
    /// Invalid URL matcher or matcher pattern.
    Matcher,
    /// Unknown or malformed action.
    Action,
    /// Unknown or malformed `when` condition.
    Condition,
    /// Unknown or malformed property target, such as a typed deletion path.
    Property,
}

impl RuleErrorCode {
    /// Returns the stable lowercase code used by CLI and control JSON contracts.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Matcher => "matcher",
            Self::Action => "action",
            Self::Condition => "condition",
            Self::Property => "property",
        }
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("{group}:{line}: {message}")]
/// A source-located DSL diagnostic returned by [`RuleSet::parse`](crate::RuleSet::parse).
pub struct RuleError {
    /// Stable diagnostic category; consumers must not infer it from `message`.
    pub code: RuleErrorCode,
    /// Caller-provided rules group containing the error.
    pub group: String,
    /// One-based line number within the group.
    pub line: usize,
    /// Human-facing detail whose wording may evolve.
    pub message: String,
}
