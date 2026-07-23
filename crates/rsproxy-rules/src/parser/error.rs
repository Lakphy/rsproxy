use super::*;

pub(crate) struct ParseRuleError {
    pub(crate) code: RuleErrorCode,
    pub(crate) source: RuleModelError,
    pub(crate) span: Option<RuleSourceSpan>,
}

pub(super) fn validate_status_range(
    code: u16,
    range: std::ops::RangeInclusive<u16>,
    context: &'static str,
    expected: &str,
) -> Result<(), RuleModelError> {
    if range.contains(&code) {
        Ok(())
    } else {
        Err(RuleModelError::constraint(
            context,
            format!("{context} must be {expected}; got {code}"),
        ))
    }
}

pub(super) fn parse_error(code: RuleErrorCode, source: RuleModelError) -> ParseRuleError {
    ParseRuleError {
        code,
        source,
        span: None,
    }
}

impl ParseRuleError {
    pub(super) fn at_token(self, token: &RuleToken) -> Self {
        self.with_span(token.start, token.end)
    }

    pub(super) fn with_span(mut self, start: usize, end: usize) -> Self {
        self.span = Some(RuleSourceSpan { start, end });
        self
    }
}
