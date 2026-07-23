use super::*;
use crate::model::{
    BodyLiteralId, CompiledBodyContainsSet, CompiledConditionResources, CompiledMatcherResources,
    CompiledRuleResources,
};

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
        index.compiled_globs = CompiledGlobSet::build(rules);
        index.compiled_body_literals = CompiledBodyContainsSet::build(rules);
        index.compiled_resources = rules
            .iter()
            .map(|rule| CompiledRuleResources {
                matcher: bind_matcher_resources(&rule.matcher, &index.compiled_globs),
                conditions: rule
                    .conditions
                    .iter()
                    .map(|condition| {
                        bind_condition_resources(
                            condition,
                            &index.compiled_globs,
                            &index.compiled_body_literals,
                        )
                    })
                    .collect(),
            })
            .collect();
        index
    }

    pub(super) fn prefilter_matches(&self, raw_url: &str) -> Vec<usize> {
        let Some(prefilter) = &self.prefilter else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        // A candidate prefilter must be a semantic superset of full regex
        // matching. Required literals can overlap (for example, a host literal
        // and the same host plus `/`), so skipping overlaps would create false
        // negatives for every rule attached to the longer literal.
        for mat in prefilter.find_overlapping_iter(raw_url) {
            if let Some(rules) = self.prefilter_literal_rules.get(mat.pattern().as_usize()) {
                extend_unique(&mut out, &mut seen, rules);
                if seen.len() == self.prefilter_rule_ids.len() {
                    break;
                }
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
            compiled_globs: self.compiled_globs.len(),
            compiled_body_literals: self.compiled_body_literals.len(),
        }
    }
}

impl CompiledBodyContainsSet {
    fn build(rules: &[Rule]) -> Self {
        let mut source_literals = BTreeSet::<String>::new();
        for rule in rules {
            for condition in &rule.conditions {
                collect_body_literals(condition, &mut source_literals);
            }
        }
        Self::from_literals(source_literals)
    }

    /// Builds a set covering one condition's body literals for focused tests.
    #[cfg(test)]
    pub(crate) fn for_condition(condition: &Condition) -> Self {
        let mut source_literals = BTreeSet::new();
        collect_body_literals(condition, &mut source_literals);
        Self::from_literals(source_literals)
    }

    fn from_literals(source_literals: BTreeSet<String>) -> Self {
        let literals = source_literals
            .iter()
            .map(|literal| literal.to_ascii_lowercase())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let normalized_ids = literals
            .iter()
            .enumerate()
            .map(|(id, literal)| (literal.clone(), id))
            .collect::<BTreeMap<_, _>>();
        let literal_ids = source_literals
            .into_iter()
            .map(|literal| {
                let id = normalized_ids[&literal.to_ascii_lowercase()];
                (literal, id)
            })
            .collect();
        let matcher = if literals.is_empty() {
            None
        } else {
            Some(
                AhoCorasickBuilder::new()
                    .ascii_case_insensitive(true)
                    .match_kind(MatchKind::Standard)
                    .build(&literals)
                    .expect("parser-bounded body literals must compile into a snapshot"),
            )
        };
        Self {
            literals,
            literal_ids,
            matcher,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.literals.len()
    }

    pub(crate) fn id(&self, literal: &str) -> Option<BodyLiteralId> {
        self.literal_ids.get(literal).copied().map(BodyLiteralId)
    }

    pub(crate) fn matches_id(&self, id: BodyLiteralId, matched: &[bool]) -> bool {
        matched.get(id.0).copied().unwrap_or(false)
    }

    pub(crate) fn scan(&self, text: &str) -> Vec<bool> {
        let mut matched = vec![false; self.literals.len()];
        let Some(matcher) = &self.matcher else {
            return matched;
        };
        let mut remaining = matched.len();
        for found in matcher.find_overlapping_iter(text) {
            let seen = &mut matched[found.pattern().as_usize()];
            if !*seen {
                *seen = true;
                remaining -= 1;
                if remaining == 0 {
                    break;
                }
            }
        }
        matched
    }
}

pub(crate) fn bind_matcher_resources(
    matcher: &Matcher,
    globs: &CompiledGlobSet,
) -> CompiledMatcherResources {
    match matcher {
        Matcher::Glob(glob) => CompiledMatcherResources::Glob {
            host: host_pattern_uses_regex(&glob.host)
                .then(|| globs.id(&glob.host, '.'))
                .flatten(),
            port: glob
                .port
                .as_ref()
                .and_then(|pattern| globs.id(pattern, '.')),
            path: glob.path.as_ref().and_then(|pattern| {
                glob_syntax_is_active(pattern)
                    .then(|| globs.id(pattern, '/'))
                    .flatten()
            }),
            query: glob
                .query
                .as_ref()
                .and_then(|pattern| globs.id(pattern, '&')),
        },
        Matcher::Not(inner) => {
            CompiledMatcherResources::Not(Box::new(bind_matcher_resources(inner, globs)))
        }
        Matcher::ExactUrl(_) | Matcher::Port(_) | Matcher::Regex(_) => {
            CompiledMatcherResources::None
        }
    }
}

pub(crate) fn bind_condition_resources(
    condition: &Condition,
    globs: &CompiledGlobSet,
    body_literals: &CompiledBodyContainsSet,
) -> CompiledConditionResources {
    match condition {
        Condition::Host(pattern) => CompiledConditionResources::Host(
            host_pattern_uses_regex(pattern)
                .then(|| globs.id(&pattern.to_ascii_lowercase(), '.'))
                .flatten(),
        ),
        Condition::Url(UrlCondition::Glob(pattern)) => CompiledConditionResources::UrlGlob(
            glob_syntax_is_active(pattern)
                .then(|| globs.id(pattern, '\0'))
                .flatten(),
        ),
        Condition::ClientIp(patterns) => CompiledConditionResources::ClientIp(
            patterns
                .iter()
                .map(|pattern| globs.id(&normalize_ip_value(pattern), '\0'))
                .collect(),
        ),
        Condition::ServerIp(patterns) => CompiledConditionResources::ServerIp(
            patterns
                .iter()
                .map(|pattern| globs.id(&normalize_ip_value(pattern), '\0'))
                .collect(),
        ),
        Condition::BodyContains(literal) => CompiledConditionResources::BodyContains(
            body_literals
                .id(literal)
                .expect("snapshot body literal must have a compiled ID"),
        ),
        Condition::Any(children) | Condition::All(children) => {
            CompiledConditionResources::Children(
                children
                    .iter()
                    .map(|child| bind_condition_resources(child, globs, body_literals))
                    .collect(),
            )
        }
        Condition::Not(inner) => CompiledConditionResources::Not(Box::new(
            bind_condition_resources(inner, globs, body_literals),
        )),
        _ => CompiledConditionResources::None,
    }
}

fn collect_body_literals(condition: &Condition, literals: &mut BTreeSet<String>) {
    condition.for_each_node(&mut |node| {
        if let Condition::BodyContains(literal) = node {
            literals.insert(literal.clone());
        }
    });
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
