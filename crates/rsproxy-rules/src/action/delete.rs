#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteBodyPath {
    segments: Vec<DeleteBodyPathSegment>,
}

impl DeleteBodyPath {
    pub fn new(segments: Vec<DeleteBodyPathSegment>) -> Result<Self, &'static str> {
        if segments.is_empty() {
            Err("delete body path must contain at least one segment")
        } else {
            Ok(Self { segments })
        }
    }

    pub fn segments(&self) -> &[DeleteBodyPathSegment] {
        &self.segments
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleteBodyPathSegment {
    Key(String),
    Index(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeletePathSegment {
    Index(i32),
    Last,
}
