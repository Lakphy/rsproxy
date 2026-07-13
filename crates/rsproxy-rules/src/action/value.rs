#[derive(Clone, Debug, PartialEq, Eq)]
/// A deferred action-value source.
///
/// Text-valued action slots require loaded bytes to be UTF-8, while body,
/// injection, and mock slots may preserve binary bytes. File paths are a
/// trusted-rule capability and are not confined to the storage directory.
pub enum Value {
    /// Inline DSL text, with templates and captures rendered by the caller.
    Inline(String),
    /// File path attempted relative to storage before being used as written.
    File(String),
    /// Key loaded from `<storage>/values/<key>`.
    Reference(String),
}

impl Value {
    /// Wraps programmatic text as an inline value without parsing DSL markers.
    pub fn inline(value: impl Into<String>) -> Self {
        Self::Inline(value.into())
    }

    /// Borrows inline text and returns `None` for values requiring external I/O.
    pub fn as_inline(&self) -> Option<&str> {
        match self {
            Self::Inline(value) => Some(value),
            Self::File(_) | Self::Reference(_) => None,
        }
    }

    /// Returns the inline text, file path, or reference key stored by this value.
    pub fn source(&self) -> &str {
        match self {
            Self::Inline(value) | Self::File(value) | Self::Reference(value) => value,
        }
    }
}

/// Checks the stable `@key` contract: 1–128 ASCII alphanumerics, `.`, `_`, or `-`.
pub fn valid_value_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
