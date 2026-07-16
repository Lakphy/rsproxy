use super::*;

impl UrlCondition {
    pub(super) fn matches(&self, raw_url: &str) -> bool {
        match self {
            UrlCondition::Glob(pattern) => {
                if pattern.contains('*') {
                    glob_match(pattern, raw_url, '\0')
                } else {
                    pattern == raw_url
                }
            }
            UrlCondition::Regex(regex) => regex.matches(raw_url).is_some(),
        }
    }
}

impl Condition {
    pub(crate) fn matches(
        &self,
        req: &RequestMeta,
        url: Option<&UrlParts>,
        res: Option<&ResponseMeta>,
        line: usize,
    ) -> bool {
        match self {
            Condition::Method(methods) => {
                methods.iter().any(|m| m.eq_ignore_ascii_case(&req.method))
            }
            Condition::Host(pattern) => url.is_some_and(|u| host_matches(pattern, &u.host)),
            Condition::Url(condition) => condition.matches(&req.url),
            Condition::ClientIp(patterns) => req
                .client_ip
                .as_deref()
                .map(normalize_ip_value)
                .is_some_and(|actual| patterns.iter().any(|pattern| ip_matches(pattern, &actual))),
            Condition::ServerIp(patterns) => req
                .server_ip
                .as_deref()
                .map(normalize_ip_value)
                .is_some_and(|actual| patterns.iter().any(|pattern| ip_matches(pattern, &actual))),
            Condition::HeaderPresent(name) => header(req.headers.as_slice(), name).is_some(),
            Condition::HeaderContains { name, value } => header(req.headers.as_slice(), name)
                .is_some_and(|actual| {
                    actual
                        .to_ascii_lowercase()
                        .contains(&value.to_ascii_lowercase())
                }),
            Condition::ResHeaderPresent(name) => match res {
                Some(res) => header(res.headers.as_slice(), name).is_some(),
                None => false,
            },
            Condition::ResHeaderContains { name, value } => match res {
                Some(res) => header(res.headers.as_slice(), name).is_some_and(|actual| {
                    actual
                        .to_ascii_lowercase()
                        .contains(&value.to_ascii_lowercase())
                }),
                None => false,
            },
            Condition::BodyContains(expected) => {
                let body = String::from_utf8_lossy(&req.body).to_ascii_lowercase();
                body.contains(&expected.to_ascii_lowercase())
            }
            Condition::BodyRegex(regex) => {
                let body = String::from_utf8_lossy(&req.body);
                regex.matches(&body).is_some()
            }
            Condition::Status(statuses) => match res {
                Some(res) => statuses.contains(&res.status),
                None => false,
            },
            Condition::ChancePermille(permille) => chance(req, line, *permille),
            Condition::EnvPresent(name) => std::env::var(name).is_ok(),
            Condition::EnvEquals { name, value } => {
                std::env::var(name).is_ok_and(|actual| actual == *value)
            }
            Condition::Any(conditions) => conditions
                .iter()
                .any(|condition| condition.matches(req, url, res, line)),
            Condition::All(conditions) => conditions
                .iter()
                .all(|condition| condition.matches(req, url, res, line)),
            Condition::Not(inner) if res.is_none() && inner.depends_on_response() => false,
            Condition::Not(inner) => !inner.matches(req, url, res, line),
        }
    }
}
