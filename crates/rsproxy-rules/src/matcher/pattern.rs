use super::*;

impl Matcher {
    pub(crate) fn matches(&self, url: &UrlParts, raw_url: &str) -> Option<Captures> {
        match self {
            Matcher::ExactUrl(expected) => exact_url_matches(expected, url).then(Captures::default),
            Matcher::Glob(glob) => glob.matches(url),
            Matcher::Port(port) => (url.effective_port() == Some(*port)).then(Captures::default),
            Matcher::Regex(regex) => regex.matches(raw_url),
            Matcher::Not(inner) => inner
                .matches(url, raw_url)
                .is_none()
                .then(Captures::default),
        }
    }
}

impl GlobMatcher {
    pub(super) fn matches(&self, url: &UrlParts) -> Option<Captures> {
        if let Some(scheme) = &self.scheme
            && !scheme.eq_ignore_ascii_case(&url.scheme)
        {
            return None;
        }
        if !host_matches(&self.host, &url.host) {
            return None;
        }
        if let Some(port_pat) = &self.port {
            let port = url.effective_port()?.to_string();
            if !glob_match(port_pat, &port, '.') {
                return None;
            }
        }

        let mut captures = Captures::default();
        if let Some(path_pat) = &self.path {
            if path_pat.contains('*') {
                if !glob_match_with_captures(path_pat, &url.path, '/', &mut captures) {
                    return None;
                }
            } else if !path_prefix_matches(path_pat, &url.path) {
                return None;
            }
        }
        if let Some(query_pat) = &self.query {
            let query = url.query.as_deref().unwrap_or("");
            if !glob_match_with_captures(query_pat, query, '&', &mut captures) {
                return None;
            }
        }
        Some(captures)
    }
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
        let mut out = Captures {
            whole: captures.get(0).map(|matched| matched.as_str().to_string()),
            ..Captures::default()
        };
        for idx in 1..captures.len().min(10) {
            out.insert_index(
                captures
                    .get(idx)
                    .map(|matched| matched.as_str().to_string())
                    .unwrap_or_default(),
            );
        }
        for name in regex.capture_names().flatten() {
            if let Some(value) = captures.name(name) {
                out.named
                    .insert(name.to_string(), value.as_str().to_string());
            }
        }
        Some(out)
    }

    fn matches_fancy(&self, regex: &FancyRegex, raw_url: &str) -> Option<Captures> {
        let captures = match regex.captures(raw_url) {
            Ok(Some(captures)) => captures,
            Ok(None) => return None,
            Err(FancyError::RuntimeError(RuntimeError::BacktrackLimitExceeded)) => return None,
            Err(_) => return None,
        };
        let mut out = Captures {
            whole: captures.get(0).map(|matched| matched.as_str().to_string()),
            ..Captures::default()
        };
        for idx in 1..captures.len().min(10) {
            out.insert_index(
                captures
                    .get(idx)
                    .map(|matched| matched.as_str().to_string())
                    .unwrap_or_default(),
            );
        }
        for name in regex.capture_names().flatten() {
            if let Some(value) = captures.name(name) {
                out.named
                    .insert(name.to_string(), value.as_str().to_string());
            }
        }
        Some(out)
    }
}
