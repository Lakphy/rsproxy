use super::*;
use crate::model::GlobId;

const MAX_GLOB_PATTERN_BYTES: usize = MAX_RULE_SOURCE_LINE_BYTES;
const GLOB_REGEX_SIZE_LIMIT: usize = 4 * 1024 * 1024;

/// Glob programs compiled once with an immutable [`RuleSet`] snapshot.
#[derive(Clone, Debug, Default)]
pub(crate) struct CompiledGlobSet {
    ids: BTreeMap<char, BTreeMap<String, GlobId>>,
    programs: Vec<LinearRegex>,
}

impl CompiledGlobSet {
    pub(crate) fn build(rules: &[Rule]) -> Self {
        let mut compiled = Self::default();
        for rule in rules {
            compiled.register_matcher(&rule.matcher);
            for condition in &rule.conditions {
                compiled.register_condition(condition);
            }
        }
        compiled
    }

    pub(crate) fn len(&self) -> usize {
        self.programs.len()
    }

    fn register(&mut self, pattern: &str, separator: char) -> GlobId {
        if let Some(id) = self.id(pattern, separator) {
            return id;
        }
        let regex = compile_glob_regex(pattern, separator)
            .expect("parser-validated glob must compile into a snapshot");
        let id = GlobId(self.programs.len());
        self.programs.push(regex);
        self.ids
            .entry(separator)
            .or_default()
            .insert(pattern.to_string(), id);
        id
    }

    fn register_matcher(&mut self, matcher: &Matcher) {
        match matcher {
            Matcher::Glob(glob) => {
                if host_pattern_uses_regex(&glob.host) {
                    self.register(&glob.host, '.');
                }
                if let Some(port) = &glob.port {
                    self.register(port, '.');
                }
                if let Some(path) = &glob.path
                    && glob_syntax_is_active(path)
                {
                    self.register(path, '/');
                }
                if let Some(query) = &glob.query {
                    self.register(query, '&');
                }
            }
            Matcher::Not(inner) => self.register_matcher(inner),
            Matcher::ExactUrl(_) | Matcher::Port(_) | Matcher::Regex(_) => {}
        }
    }

    fn register_condition(&mut self, condition: &Condition) {
        condition.for_each_node(&mut |condition| match condition {
            Condition::Host(pattern) if host_pattern_uses_regex(pattern) => {
                self.register(&pattern.to_ascii_lowercase(), '.');
            }
            Condition::Url(UrlCondition::Glob(pattern)) if glob_syntax_is_active(pattern) => {
                self.register(pattern, '\0');
            }
            Condition::ClientIp(patterns) | Condition::ServerIp(patterns) => {
                for pattern in patterns.iter().filter(|pattern| {
                    glob_syntax_is_active(pattern) && *pattern != "*" && *pattern != "**"
                }) {
                    self.register(&normalize_ip_value(pattern), '\0');
                }
            }
            _ => {}
        });
    }

    #[cfg(test)]
    pub(crate) fn glob_match(&self, pattern: &str, text: &str, separator: char) -> bool {
        self.id(pattern, separator)
            .is_some_and(|id| self.glob_match_id(id, text))
    }

    #[cfg(test)]
    pub(crate) fn glob_match_with_captures(
        &self,
        pattern: &str,
        text: &str,
        separator: char,
        captures: &mut Captures,
    ) -> bool {
        self.id(pattern, separator)
            .is_some_and(|id| self.glob_match_with_captures_id(id, text, captures))
    }

    pub(crate) fn id(&self, pattern: &str, separator: char) -> Option<GlobId> {
        self.ids
            .get(&separator)
            .and_then(|patterns| patterns.get(pattern))
            .copied()
    }

    pub(crate) fn glob_match_id(&self, id: GlobId, text: &str) -> bool {
        self.programs
            .get(id.0)
            .is_some_and(|regex| regex.is_match(text))
    }

    pub(crate) fn glob_match_with_captures_id(
        &self,
        id: GlobId,
        text: &str,
        captures: &mut Captures,
    ) -> bool {
        self.programs
            .get(id.0)
            .is_some_and(|regex| append_glob_captures(regex, text, captures))
    }

    pub(crate) fn host_matches(&self, pattern: &str, host: &str) -> bool {
        let normalized = pattern.trim_matches(['[', ']']).to_ascii_lowercase();
        self.host_matches_id(pattern, host, self.id(&normalized, '.'))
    }

    pub(crate) fn host_matches_id(&self, pattern: &str, host: &str, id: Option<GlobId>) -> bool {
        let pattern = pattern.trim_matches(['[', ']']).to_ascii_lowercase();
        let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
        if pattern == "**" || pattern == "*" {
            return true;
        }
        if let Some(base) = pattern.strip_prefix("**.") {
            return host == base || dotted_suffix_prefix(&host, base).is_some();
        }
        if let Some(base) = pattern.strip_prefix("*.") {
            return dotted_suffix_prefix(&host, base)
                .is_some_and(|prefix| !prefix.is_empty() && !prefix.contains('.'));
        }
        if glob_syntax_is_active(&pattern) {
            id.is_some_and(|id| self.glob_match_id(id, &host))
        } else {
            pattern == host
        }
    }

    /// Matches a load-time-normalized pattern against an already-normalized IP.
    #[cfg(test)]
    pub(crate) fn ip_matches(&self, pattern: &str, actual: &str) -> bool {
        let pattern = normalize_ip_value(pattern);
        self.ip_matches_id(&pattern, actual, self.id(&pattern, '\0'))
    }

    pub(crate) fn ip_matches_id(&self, pattern: &str, actual: &str, id: Option<GlobId>) -> bool {
        let pattern = normalize_ip_value(pattern);
        if pattern == "*" || pattern == "**" {
            true
        } else if glob_syntax_is_active(&pattern) {
            id.is_some_and(|id| self.glob_match_id(id, actual))
        } else {
            pattern.eq_ignore_ascii_case(actual)
        }
    }

    /// Builds a set covering one matcher's glob patterns for focused tests.
    #[cfg(test)]
    pub(crate) fn for_matcher(matcher: &Matcher) -> Self {
        let mut compiled = Self::default();
        compiled.register_matcher(matcher);
        compiled
    }

    /// Builds a set covering one condition's glob patterns for focused tests.
    #[cfg(test)]
    pub(crate) fn for_condition(condition: &Condition) -> Self {
        let mut compiled = Self::default();
        compiled.register_condition(condition);
        compiled
    }

    /// Builds a set from explicit `(pattern, separator)` pairs for focused tests.
    #[cfg(test)]
    pub(crate) fn of(patterns: &[(&str, char)]) -> Self {
        let mut compiled = Self::default();
        for (pattern, separator) in patterns {
            compiled.register(pattern, *separator);
        }
        compiled
    }
}

pub(crate) fn host_pattern_uses_regex(pattern: &str) -> bool {
    let pattern = pattern.trim_matches(['[', ']']);
    glob_syntax_is_active(pattern)
        && pattern != "*"
        && pattern != "**"
        && !pattern.starts_with("*.")
        && !pattern.starts_with("**.")
}

pub(crate) fn glob_syntax_is_active(pattern: &str) -> bool {
    pattern.contains(['*', '\\'])
}

pub(super) fn exact_url_matches(expected: &str, url: &UrlParts) -> bool {
    let Ok(expected) = UrlParts::parse(expected) else {
        return false;
    };
    expected.scheme == url.scheme
        && expected.host == url.host
        && expected.effective_port() == url.effective_port()
        && expected.path == url.path
        && (expected.query.is_none() || expected.query == url.query)
}

/// Returns the label prefix of `host` when it ends with `.{base}`.
pub(super) fn dotted_suffix_prefix<'a>(host: &'a str, base: &str) -> Option<&'a str> {
    host.strip_suffix(base)?.strip_suffix('.')
}

pub(super) fn normalize_ip_value(value: &str) -> String {
    value
        .parse::<SocketAddr>()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| value.trim().trim_matches(['[', ']']).to_string())
}

pub(super) fn path_prefix_matches(pattern: &str, path: &str) -> bool {
    if pattern == "/" {
        return true;
    }
    path == pattern
        || (path.starts_with(pattern)
            && (pattern.ends_with('/') || path.as_bytes().get(pattern.len()) == Some(&b'/')))
}

fn append_glob_captures(regex: &LinearRegex, text: &str, captures: &mut Captures) -> bool {
    regex
        .captures(text)
        .is_some_and(|matched| append_matched_glob_captures(&matched, captures))
}

fn append_matched_glob_captures(matched: &regex::Captures<'_>, captures: &mut Captures) -> bool {
    let remaining = MAX_RULE_GLOB_CAPTURES.saturating_sub(captures.indexed.len());
    for capture in matched.iter().skip(1).take(remaining) {
        captures.insert_index(
            capture
                .map(|value| value.as_str().to_string())
                .unwrap_or_default(),
        );
    }
    true
}

fn compile_glob_regex(pattern: &str, sep: char) -> Option<LinearRegex> {
    fn push_literal(source: &mut String, character: char) {
        if regex_syntax::is_meta_character(character) {
            source.push('\\');
        }
        source.push(character);
    }

    let mut source = String::with_capacity(pattern.len().saturating_mul(2) + 8);
    source.push_str("(?s)\\A");
    let mut characters = pattern.chars().peekable();
    let mut captures = 0usize;
    while let Some(character) = characters.next() {
        if character == '\\' {
            push_literal(&mut source, characters.next()?);
            continue;
        }
        if character != '*' {
            push_literal(&mut source, character);
            continue;
        }

        let double = characters.peek() == Some(&'*');
        if double {
            characters.next();
        }
        if captures < MAX_RULE_GLOB_CAPTURES {
            source.push('(');
            captures += 1;
        } else {
            source.push_str("(?:");
        }
        if double {
            source.push_str(".*?");
        } else {
            source.push_str("[^");
            push_literal(&mut source, sep);
            source.push_str("]*?");
        }
        source.push(')');
    }
    source.push_str("\\z");
    LinearRegexBuilder::new(&source)
        .size_limit(GLOB_REGEX_SIZE_LIMIT)
        .build()
        .ok()
}

pub(crate) fn validate_glob_pattern(
    pattern: &str,
    separator: char,
    context: &'static str,
) -> Result<(), RuleModelError> {
    if pattern.len() > MAX_GLOB_PATTERN_BYTES {
        return Err(RuleModelError::limit(
            context,
            format!("glob pattern exceeds {MAX_GLOB_PATTERN_BYTES} bytes"),
        ));
    }
    compile_glob_regex(pattern, separator)
        .map(drop)
        .ok_or_else(|| RuleModelError::invalid(context, "invalid or incomplete glob pattern"))
}

pub(super) fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

pub(super) fn chance(req: &RequestMeta, line: usize, permille: u16) -> bool {
    if permille >= 1000 {
        return true;
    }
    if permille == 0 {
        return false;
    }
    let mut hash = 1469598103934665603u64;
    for byte in req.url.as_bytes().iter().chain(req.method.as_bytes()) {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash ^= line as u64;
    (hash % 1000) < permille as u64
}
