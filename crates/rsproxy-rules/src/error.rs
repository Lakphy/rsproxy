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
    #[error("{message}")]
    EmptyInput {
        context: &'static str,
        message: String,
    },

    #[error("{message}")]
    MissingArgument {
        context: &'static str,
        message: String,
    },

    #[error("{message}")]
    InvalidSyntax {
        context: &'static str,
        message: String,
    },

    #[error("{message}")]
    InvalidInput {
        context: &'static str,
        message: String,
    },

    #[error("{message}")]
    UnsupportedInput {
        context: &'static str,
        message: String,
    },

    #[error("{message}")]
    ConstraintViolation {
        context: &'static str,
        message: String,
    },

    #[error("{message}")]
    LimitExceeded {
        context: &'static str,
        message: String,
    },

    #[error("{message}")]
    InvalidInteger {
        context: &'static str,
        input: String,
        message: String,
        #[source]
        source: ParseIntError,
    },

    #[error("{message}")]
    InvalidFloat {
        context: &'static str,
        input: String,
        message: String,
        #[source]
        source: ParseFloatError,
    },

    #[error("{context}: {source}")]
    InvalidRegex {
        context: &'static str,
        #[source]
        source: Box<regex::Error>,
    },

    #[error("invalid regex matcher: regex={linear}; fancy-regex={fancy}")]
    InvalidRegexMatcher {
        linear: Box<regex::Error>,
        #[source]
        fancy: Box<fancy_regex::Error>,
    },

    #[error("invalid exact URL matcher: {source}")]
    InvalidExactUrlMatcher {
        #[source]
        source: Box<RuleModelError>,
    },

    #[error("{message}")]
    InvalidTemplate {
        context: &'static str,
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
pub enum RuleErrorCode {
    Syntax,
    Matcher,
    Action,
    Condition,
    Property,
}

impl RuleErrorCode {
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
pub struct RuleError {
    pub code: RuleErrorCode,
    pub group: String,
    pub line: usize,
    pub message: String,
}
