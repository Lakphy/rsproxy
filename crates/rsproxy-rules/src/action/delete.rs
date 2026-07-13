use crate::RuleModelError;

#[derive(Clone, Debug, PartialEq, Eq)]
/// A non-empty, validated path into a JSON, form, or JSONP body.
///
/// The DSL limits paths to 16 KiB and 128 segments. Object keys retain escaped
/// separators, while a trailing array selector becomes an index segment.
pub struct DeleteBodyPath {
    segments: Vec<DeleteBodyPathSegment>,
}

impl DeleteBodyPath {
    /// Constructs a path, rejecting an empty segment list.
    pub fn new(segments: Vec<DeleteBodyPathSegment>) -> Result<Self, RuleModelError> {
        if segments.is_empty() {
            Err(RuleModelError::empty(
                "delete body path",
                "delete body path must contain at least one segment",
            ))
        } else {
            Ok(Self { segments })
        }
    }

    /// Returns path segments in traversal order without exposing mutable invariants.
    pub fn segments(&self) -> &[DeleteBodyPathSegment] {
        &self.segments
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// One traversal step in a nested body deletion path.
pub enum DeleteBodyPathSegment {
    /// Selects an object key or form field component.
    Key(String),
    /// Selects a zero-based JSON array element.
    Index(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// A URL pathname segment selector evaluated against the original pathname.
pub enum DeletePathSegment {
    /// Selects a zero-based segment; negative values count backward from the end.
    Index(i32),
    /// Removes the final segment while preserving an existing trailing slash.
    Last,
}
