use super::*;

impl RuleIndex {
    pub(super) fn build(rules: &[Rule]) -> Self {
        let mut index = Self::default();
        let mut prefilter_literal_rules: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (idx, rule) in rules.iter().enumerate() {
            if let Matcher::Regex(regex) = &rule.matcher
                && let Some(literal) = required_regex_literal(regex)
            {
                prefilter_literal_rules
                    .entry(literal)
                    .or_default()
                    .push(idx);
                index.prefilter_rule_ids.insert(idx);
                continue;
            }
            match matcher_index_key(&rule.matcher) {
                MatcherIndexKey::Exact(host) => {
                    index.domain_exact.entry(host).or_default().push(idx);
                }
                MatcherIndexKey::Suffix(host) => {
                    index.domain_suffix.entry(host).or_default().push(idx);
                }
                MatcherIndexKey::Global => index.global.push(idx),
            }
        }
        for (literal, rules) in prefilter_literal_rules {
            index.prefilter_literals.push(literal);
            index.prefilter_literal_rules.push(rules);
        }
        index.prefilter = if index.prefilter_literals.is_empty() {
            None
        } else {
            AhoCorasick::new(&index.prefilter_literals).ok()
        };
        if index.prefilter.is_none() {
            for rules in index.prefilter_literal_rules.drain(..) {
                index.global.extend(rules);
            }
            index.prefilter_literals.clear();
            index.prefilter_rule_ids.clear();
        }
        index
    }

    pub(super) fn prefilter_matches(&self, raw_url: &str) -> Vec<usize> {
        let Some(prefilter) = &self.prefilter else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for mat in prefilter.find_iter(raw_url) {
            if let Some(rules) = self.prefilter_literal_rules.get(mat.pattern().as_usize()) {
                extend_unique(&mut out, &mut seen, rules);
            }
        }
        out
    }

    pub(super) fn stats(&self, rules: &[Rule]) -> RuleSetStats {
        RuleSetStats {
            rules: rules.len(),
            disabled: rules.iter().filter(|rule| rule.disabled).count(),
            domain_exact_entries: self.domain_exact.len(),
            domain_suffix_entries: self.domain_suffix.len(),
            indexed_rules: self
                .domain_exact
                .values()
                .chain(self.domain_suffix.values())
                .map(Vec::len)
                .sum(),
            global_rules: self.global.len(),
            prefilter_literals: self.prefilter_literals.len(),
            prefilter_rules: self.prefilter_rule_ids.len(),
        }
    }
}

enum MatcherIndexKey {
    Exact(String),
    Suffix(String),
    Global,
}

fn matcher_index_key(matcher: &Matcher) -> MatcherIndexKey {
    match matcher {
        Matcher::ExactUrl(expected) => UrlParts::parse(expected)
            .ok()
            .map(|url| {
                MatcherIndexKey::Exact(url.host.trim_matches(['[', ']']).to_ascii_lowercase())
            })
            .unwrap_or(MatcherIndexKey::Global),
        Matcher::Glob(glob) => glob_index_key(glob),
        Matcher::Port(_) | Matcher::Regex(_) | Matcher::Not(_) => MatcherIndexKey::Global,
    }
}

fn glob_index_key(glob: &GlobMatcher) -> MatcherIndexKey {
    let host = glob.host.trim_matches(['[', ']']).to_ascii_lowercase();
    if host.is_empty() || host == "*" || host.contains('?') {
        return MatcherIndexKey::Global;
    }
    if let Some(base) = host
        .strip_prefix("**.")
        .or_else(|| host.strip_prefix("*."))
        .or_else(|| host.strip_prefix('.'))
    {
        if is_plain_domain(base) {
            return MatcherIndexKey::Suffix(base.to_string());
        }
        return MatcherIndexKey::Global;
    }
    if host.contains('*') {
        return MatcherIndexKey::Global;
    }
    if is_plain_domain(&host) {
        MatcherIndexKey::Exact(host)
    } else {
        MatcherIndexKey::Global
    }
}

fn is_plain_domain(host: &str) -> bool {
    !host.is_empty()
        && host
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | ':' | '_'))
}

pub(super) fn host_suffixes(host: &str) -> Vec<&str> {
    let mut suffixes = Vec::new();
    let mut start = 0usize;
    loop {
        suffixes.push(&host[start..]);
        let Some(next_dot) = host[start..].find('.') else {
            break;
        };
        start += next_dot + 1;
        if start >= host.len() {
            break;
        }
    }
    suffixes
}

pub(super) fn extend_unique(out: &mut Vec<usize>, seen: &mut HashSet<usize>, values: &[usize]) {
    for value in values {
        if seen.insert(*value) {
            out.push(*value);
        }
    }
}

fn required_regex_literal(regex: &RegexMatcher) -> Option<String> {
    use regex_syntax::hir::{Hir, HirKind};

    fn longest_required(hir: &Hir) -> Option<Vec<u8>> {
        match hir.kind() {
            HirKind::Literal(literal) => Some(literal.0.to_vec()),
            HirKind::Capture(capture) => longest_required(&capture.sub),
            HirKind::Concat(parts) => parts
                .iter()
                .filter_map(longest_required)
                .max_by_key(Vec::len),
            HirKind::Repetition(repetition) if repetition.min > 0 => {
                longest_required(&repetition.sub)
            }
            HirKind::Empty
            | HirKind::Class(_)
            | HirKind::Look(_)
            | HirKind::Repetition(_)
            | HirKind::Alternation(_) => None,
        }
    }

    if regex.case_insensitive {
        return None;
    }
    let hir = regex_syntax::parse(&regex.pattern).ok()?;
    let literal = String::from_utf8(longest_required(&hir)?).ok()?;
    (literal.len() >= 3).then_some(literal)
}
