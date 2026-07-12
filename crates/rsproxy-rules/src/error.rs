use std::fmt;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleError {
    pub code: RuleErrorCode,
    pub group: String,
    pub line: usize,
    pub message: String,
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.group, self.line, self.message)
    }
}
