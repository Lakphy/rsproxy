use super::*;
use crate::model::{CompiledMatcherResources, GlobId};

impl Matcher {
    #[cfg(test)]
    pub(crate) fn matches(&self, url: &UrlParts, raw_url: &str) -> Option<Captures> {
        let globs = CompiledGlobSet::for_matcher(self);
        let resources = bind_matcher_resources(self, &globs);
        self.matches_compiled(url, raw_url, &globs, &resources)
    }

    pub(crate) fn matches_compiled(
        &self,
        url: &UrlParts,
        raw_url: &str,
        globs: &CompiledGlobSet,
        resources: &CompiledMatcherResources,
    ) -> Option<Captures> {
        match self {
            Matcher::ExactUrl(expected) => exact_url_matches(expected, url).then(Captures::default),
            Matcher::Glob(glob) => glob.matches_compiled(url, globs, resources),
            Matcher::Port(port) => (url.effective_port() == Some(*port)).then(Captures::default),
            Matcher::Regex(regex) => regex.matches(raw_url),
            Matcher::Not(inner) => {
                let CompiledMatcherResources::Not(inner_resources) = resources else {
                    return None;
                };
                inner
                    .matches_compiled(url, raw_url, globs, inner_resources)
                    .is_none()
                    .then(Captures::default)
            }
        }
    }
}

impl GlobMatcher {
    fn matches_compiled(
        &self,
        url: &UrlParts,
        globs: &CompiledGlobSet,
        resources: &CompiledMatcherResources,
    ) -> Option<Captures> {
        let CompiledMatcherResources::Glob {
            host,
            port: port_id,
            path: path_id,
            query: query_id,
        } = resources
        else {
            return None;
        };
        if let Some(scheme) = &self.scheme
            && !scheme.eq_ignore_ascii_case(&url.scheme)
        {
            return None;
        }
        if !globs.host_matches_id(&self.host, &url.host, *host) {
            return None;
        }
        if self.port.is_some() {
            let port = url.effective_port()?.to_string();
            if !resources_match(*port_id, &port, globs) {
                return None;
            }
        }

        let mut captures = Captures::default();
        if let Some(path_pat) = &self.path {
            if glob_syntax_is_active(path_pat) {
                if !resources_match_with_captures(*path_id, &url.path, globs, &mut captures) {
                    return None;
                }
            } else if !path_prefix_matches(path_pat, &url.path) {
                return None;
            }
        }
        if self.query.is_some() {
            let query = url.query.as_deref().unwrap_or("");
            if !resources_match_with_captures(*query_id, query, globs, &mut captures) {
                return None;
            }
        }
        Some(captures)
    }
}

fn resources_match(id: Option<GlobId>, text: &str, globs: &CompiledGlobSet) -> bool {
    id.is_some_and(|id| globs.glob_match_id(id, text))
}

fn resources_match_with_captures(
    id: Option<GlobId>,
    text: &str,
    globs: &CompiledGlobSet,
    captures: &mut Captures,
) -> bool {
    id.is_some_and(|id| globs.glob_match_with_captures_id(id, text, captures))
}

impl RegexMatcher {
    pub(super) fn matches(&self, raw_url: &str) -> Option<Captures> {
        match self.compiled.as_ref() {
            CompiledRegex::Linear(regex) => self.matches_linear(regex, raw_url),
            CompiledRegex::Fancy(regex) => self.matches_fancy(regex, raw_url),
        }
    }

    fn matches_linear(&self, regex: &LinearRegex, raw_url: &str) -> Option<Captures> {
        let captures = regex.captures(raw_url)?;
        Some(collect_captures(
            captures.len(),
            |idx| captures.get(idx).map(|matched| matched.as_str()),
            regex.capture_names().flatten(),
            |name| captures.name(name).map(|matched| matched.as_str()),
        ))
    }

    fn matches_fancy(&self, regex: &FancyRegex, raw_url: &str) -> Option<Captures> {
        let captures = match regex.captures(raw_url) {
            Ok(Some(captures)) => captures,
            Ok(None) => return None,
            Err(FancyError::RuntimeError(RuntimeError::BacktrackLimitExceeded)) => return None,
            Err(_) => return None,
        };
        Some(collect_captures(
            captures.len(),
            |idx| captures.get(idx).map(|matched| matched.as_str()),
            regex.capture_names().flatten(),
            |name| captures.name(name).map(|matched| matched.as_str()),
        ))
    }
}

/// Builds [`Captures`] from any regex engine's whole/indexed/named accessors,
/// keeping the public `$1`-`$9` numbered-capture limit.
fn collect_captures<'t>(
    count: usize,
    get_index: impl Fn(usize) -> Option<&'t str>,
    names: impl Iterator<Item = &'t str>,
    get_name: impl Fn(&str) -> Option<&'t str>,
) -> Captures {
    let mut out = Captures {
        whole: get_index(0).map(Arc::from),
        ..Captures::default()
    };
    for idx in 1..count.min(10) {
        out.insert_index(get_index(idx).map(str::to_string).unwrap_or_default());
    }
    for name in names {
        if let Some(value) = get_name(name) {
            Arc::make_mut(&mut out.named).insert(name.to_string(), Arc::from(value));
        }
    }
    out
}
