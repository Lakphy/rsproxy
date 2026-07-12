use super::*;

impl Condition {
    pub(super) fn depends_on_request_body(&self) -> bool {
        match self {
            Condition::BodyContains(_) | Condition::BodyRegex(_) => true,
            Condition::Any(conditions) => conditions.iter().any(Self::depends_on_request_body),
            Condition::Not(inner) => inner.depends_on_request_body(),
            _ => false,
        }
    }

    pub(super) fn may_match_before_request_body(
        &self,
        req: &RequestMeta,
        url: Option<&UrlParts>,
        line: usize,
    ) -> bool {
        match self {
            Condition::BodyContains(_)
            | Condition::BodyRegex(_)
            | Condition::ResHeaderPresent(_)
            | Condition::ResHeaderContains { .. }
            | Condition::Status(_) => true,
            Condition::Any(conditions) => conditions
                .iter()
                .any(|condition| condition.may_match_before_request_body(req, url, line)),
            Condition::Not(inner)
                if inner.depends_on_request_body() || inner.depends_on_response() =>
            {
                true
            }
            _ => self.matches(req, url, None, line),
        }
    }

    pub(crate) fn depends_on_response(&self) -> bool {
        match self {
            Condition::ResHeaderPresent(_)
            | Condition::ResHeaderContains { .. }
            | Condition::Status(_) => true,
            Condition::Any(conditions) => conditions.iter().any(Self::depends_on_response),
            Condition::Not(inner) => inner.depends_on_response(),
            _ => false,
        }
    }
}
