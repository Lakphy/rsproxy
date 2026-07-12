#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Inline(String),
    File(String),
    Reference(String),
}

impl Value {
    pub fn inline(value: impl Into<String>) -> Self {
        Self::Inline(value.into())
    }

    pub fn as_inline(&self) -> Option<&str> {
        match self {
            Self::Inline(value) => Some(value),
            Self::File(_) | Self::Reference(_) => None,
        }
    }

    pub fn source(&self) -> &str {
        match self {
            Self::Inline(value) | Self::File(value) | Self::Reference(value) => value,
        }
    }
}

pub fn valid_value_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
