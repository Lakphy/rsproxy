use super::*;
use crate::model::{BodyLiteralId, CompiledBodyContainsSet, CompiledConditionResources};
use std::borrow::Cow;
use std::cell::OnceCell;

pub(crate) struct ConditionCache<'a> {
    req: &'a RequestMeta,
    body_text: OnceCell<Cow<'a, str>>,
    body_literal_matches: OnceCell<Vec<bool>>,
    client_ip: OnceCell<Option<String>>,
    server_ip: OnceCell<Option<String>>,
}

pub(crate) struct ConditionMatchContext<'req, 'ctx> {
    url: Option<&'ctx UrlParts>,
    res: Option<&'ctx ResponseMeta>,
    line: usize,
    globs: &'ctx CompiledGlobSet,
    body_literals: &'ctx CompiledBodyContainsSet,
    cache: &'ctx ConditionCache<'req>,
}

impl<'req, 'ctx> ConditionMatchContext<'req, 'ctx> {
    pub(crate) fn compiled(
        url: Option<&'ctx UrlParts>,
        res: Option<&'ctx ResponseMeta>,
        line: usize,
        globs: &'ctx CompiledGlobSet,
        body_literals: &'ctx CompiledBodyContainsSet,
        cache: &'ctx ConditionCache<'req>,
    ) -> Self {
        Self {
            url,
            res,
            line,
            globs,
            body_literals,
            cache,
        }
    }

    /// Rebinds the same shared caches to another rule's source line.
    pub(crate) fn with_line(&self, line: usize) -> Self {
        Self { line, ..*self }
    }
}

impl<'a> ConditionCache<'a> {
    pub(crate) fn new(req: &'a RequestMeta) -> Self {
        Self {
            req,
            body_text: OnceCell::new(),
            body_literal_matches: OnceCell::new(),
            client_ip: OnceCell::new(),
            server_ip: OnceCell::new(),
        }
    }

    fn body_text(&self) -> &str {
        self.body_text
            .get_or_init(|| String::from_utf8_lossy(&self.req.body))
            .as_ref()
    }

    fn client_ip(&self) -> Option<&str> {
        self.client_ip
            .get_or_init(|| self.req.client_ip.as_deref().map(normalize_ip_value))
            .as_deref()
    }

    fn server_ip(&self) -> Option<&str> {
        self.server_ip
            .get_or_init(|| self.req.server_ip.as_deref().map(normalize_ip_value))
            .as_deref()
    }

    fn body_contains(&self, compiled: &CompiledBodyContainsSet, id: BodyLiteralId) -> bool {
        let text = self.body_text();
        let matched = self
            .body_literal_matches
            .get_or_init(|| compiled.scan(text));
        compiled.matches_id(id, matched)
    }
}

impl UrlCondition {
    fn matches(
        &self,
        raw_url: &str,
        globs: &CompiledGlobSet,
        resources: &CompiledConditionResources,
    ) -> bool {
        match self {
            UrlCondition::Glob(pattern) => {
                if glob_syntax_is_active(pattern) {
                    let CompiledConditionResources::UrlGlob(id) = resources else {
                        return false;
                    };
                    id.is_some_and(|id| globs.glob_match_id(id, raw_url))
                } else {
                    pattern == raw_url
                }
            }
            UrlCondition::Regex(regex) => regex.matches(raw_url).is_some(),
        }
    }
}

impl Condition {
    #[cfg(test)]
    pub(crate) fn matches(
        &self,
        req: &RequestMeta,
        url: Option<&UrlParts>,
        res: Option<&ResponseMeta>,
        line: usize,
    ) -> bool {
        let globs = CompiledGlobSet::for_condition(self);
        let body_literals = CompiledBodyContainsSet::for_condition(self);
        let resources = bind_condition_resources(self, &globs, &body_literals);
        let cache = ConditionCache::new(req);
        self.matches_with_compiled(
            &resources,
            &ConditionMatchContext {
                url,
                res,
                line,
                globs: &globs,
                body_literals: &body_literals,
                cache: &cache,
            },
        )
    }

    pub(crate) fn matches_with_compiled(
        &self,
        resources: &CompiledConditionResources,
        context: &ConditionMatchContext<'_, '_>,
    ) -> bool {
        let ConditionMatchContext {
            url,
            res,
            line,
            globs,
            body_literals,
            cache,
        } = context;
        let req = cache.req;
        match self {
            Condition::Method(methods) => {
                methods.iter().any(|m| m.eq_ignore_ascii_case(&req.method))
            }
            Condition::Host(pattern) => {
                let CompiledConditionResources::Host(id) = resources else {
                    return false;
                };
                url.is_some_and(|url| globs.host_matches_id(pattern, &url.host, *id))
            }
            Condition::Url(condition) => condition.matches(&req.url, globs, resources),
            Condition::ClientIp(patterns) => cache.client_ip().is_some_and(|actual| {
                let CompiledConditionResources::ClientIp(ids) = resources else {
                    return false;
                };
                patterns
                    .iter()
                    .zip(ids)
                    .any(|(pattern, id)| globs.ip_matches_id(pattern, actual, *id))
            }),
            Condition::ServerIp(patterns) => cache.server_ip().is_some_and(|actual| {
                let CompiledConditionResources::ServerIp(ids) = resources else {
                    return false;
                };
                patterns
                    .iter()
                    .zip(ids)
                    .any(|(pattern, id)| globs.ip_matches_id(pattern, actual, *id))
            }),
            Condition::HeaderPresent(name) => header(req.headers.as_slice(), name).is_some(),
            Condition::HeaderContains { name, value } => header(req.headers.as_slice(), name)
                .is_some_and(|actual| contains_ascii_case_insensitive(actual, value)),
            Condition::ResHeaderPresent(name) => match res {
                Some(res) => header(res.headers.as_slice(), name).is_some(),
                None => false,
            },
            Condition::ResHeaderContains { name, value } => match res {
                Some(res) => header(res.headers.as_slice(), name)
                    .is_some_and(|actual| contains_ascii_case_insensitive(actual, value)),
                None => false,
            },
            Condition::BodyContains(_) => {
                let CompiledConditionResources::BodyContains(id) = resources else {
                    return false;
                };
                cache.body_contains(body_literals, *id)
            }
            Condition::BodyRegex(regex) => regex.matches(cache.body_text()).is_some(),
            Condition::Status(statuses) => match res {
                Some(res) => statuses.contains(&res.status),
                None => false,
            },
            Condition::ChancePermille(permille) => chance(req, *line, *permille),
            Condition::EnvPresent(name) => std::env::var(name).is_ok(),
            Condition::EnvEquals { name, value } => {
                std::env::var(name).is_ok_and(|actual| actual == *value)
            }
            Condition::Any(conditions) => {
                let CompiledConditionResources::Children(children) = resources else {
                    return false;
                };
                conditions
                    .iter()
                    .zip(children)
                    .any(|(condition, resources)| {
                        condition.matches_with_compiled(resources, context)
                    })
            }
            Condition::All(conditions) => {
                let CompiledConditionResources::Children(children) = resources else {
                    return false;
                };
                conditions
                    .iter()
                    .zip(children)
                    .all(|(condition, resources)| {
                        condition.matches_with_compiled(resources, context)
                    })
            }
            Condition::Not(inner) if res.is_none() && inner.depends_on_response() => false,
            Condition::Not(inner) => {
                let CompiledConditionResources::Not(inner_resources) = resources else {
                    return false;
                };
                !inner.matches_with_compiled(inner_resources, context)
            }
        }
    }
}

fn contains_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}
