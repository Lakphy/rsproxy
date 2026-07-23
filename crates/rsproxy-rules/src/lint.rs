use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
/// A rule that can never win one or more of its single-action families because
/// an earlier, unconditional, broader rule always claims them first.
///
/// Within one snapshot rules resolve in group order, then line order, with
/// `@important` rules moved ahead; single-action families keep the first
/// matching action. A later, more specific rule is therefore silently ignored
/// when a broader earlier rule covers every URL it matches.
pub struct LintFinding {
    /// Group of the rule that never takes effect.
    pub group: Arc<str>,
    /// One-based source line of the shadowed rule.
    pub line: usize,
    /// Comment-free source of the shadowed rule.
    pub raw: Arc<str>,
    /// Group of the earlier rule that wins first.
    pub shadowed_by_group: Arc<str>,
    /// One-based source line of the earlier rule.
    pub shadowed_by_line: usize,
    /// Comment-free source of the earlier rule.
    pub shadowed_by_raw: Arc<str>,
    /// Single-action families the later rule can never win.
    pub families: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Bounded conservative shadow-lint result.
pub struct LintReport {
    /// Findings retained in deterministic rule/family order.
    pub findings: Vec<LintFinding>,
    /// False when the comparison, finding, or report-byte budget was reached.
    pub complete: bool,
    /// Pairwise rule comparisons performed.
    pub comparisons: usize,
    /// Matcher-source bytes charged for the comparisons performed.
    pub comparison_bytes: usize,
}

impl RuleSet {
    /// Reports later single-action-family rules that are unreachable because an
    /// earlier, condition-free rule with a broader matcher always wins first.
    ///
    /// The subsumption check is conservative: only glob/exact matchers with a
    /// provable coverage relationship produce findings, so an empty result does
    /// not guarantee the absence of ordering mistakes.
    pub fn lint(&self) -> Vec<LintFinding> {
        self.lint_report().findings
    }

    /// Runs shadow lint with explicit completeness and resource-budget metadata.
    pub fn lint_report(&self) -> LintReport {
        let mut ordered: Vec<usize> = (0..self.rules.len())
            .filter(|idx| !self.rules[*idx].disabled)
            .collect();
        ordered.sort_by_key(|idx| {
            let rule = &self.rules[*idx];
            (!rule.important, *idx)
        });

        let ordered_families = ordered
            .iter()
            .map(|index| single_families(&self.rules[*index]))
            .collect::<Vec<_>>();
        let matcher_bytes = ordered
            .iter()
            .map(|index| matcher_source_bytes(&self.rules[*index].matcher))
            .collect::<Vec<_>>();
        let mut findings = Vec::new();
        let mut report_bytes = 0usize;
        let mut comparisons = 0usize;
        let mut comparison_bytes = 0usize;
        let mut complete = true;
        'later: for (pos, &later_idx) in ordered.iter().enumerate() {
            let later = &self.rules[later_idx];
            let later_families = ordered_families[pos].clone();
            if later_families.is_empty() {
                continue;
            }
            let mut remaining = later_families;
            for (earlier_pos, &earlier_idx) in ordered[..pos].iter().enumerate() {
                if comparisons == MAX_RULE_LINT_COMPARISONS {
                    complete = false;
                    break 'later;
                }
                let comparison_size = matcher_bytes[pos].saturating_add(matcher_bytes[earlier_pos]);
                if comparison_bytes
                    .checked_add(comparison_size)
                    .is_none_or(|bytes| bytes > MAX_RULE_LINT_COMPARISON_BYTES)
                {
                    complete = false;
                    break 'later;
                }
                comparisons += 1;
                comparison_bytes += comparison_size;
                let earlier = &self.rules[earlier_idx];
                if !earlier.conditions.is_empty() {
                    continue;
                }
                let shared: Vec<String> = ordered_families[earlier_pos]
                    .iter()
                    .filter(|family| remaining.contains(family))
                    .cloned()
                    .collect();
                if shared.is_empty() {
                    continue;
                }
                if !matcher_subsumes(&earlier.matcher, &later.matcher, &self.index.compiled_globs) {
                    continue;
                }
                remaining.retain(|family| !shared.contains(family));
                let finding = LintFinding {
                    group: later.group.clone(),
                    line: later.line,
                    raw: later.raw.clone(),
                    shadowed_by_group: earlier.group.clone(),
                    shadowed_by_line: earlier.line,
                    shadowed_by_raw: earlier.raw.clone(),
                    families: shared,
                };
                let size = lint_finding_bytes(&finding);
                if findings.len() == MAX_RULE_LINT_FINDINGS
                    || report_bytes
                        .checked_add(size)
                        .is_none_or(|bytes| bytes > MAX_RULE_LINT_REPORT_BYTES)
                {
                    complete = false;
                    break 'later;
                }
                report_bytes += size;
                findings.push(finding);
                if remaining.is_empty() {
                    break;
                }
            }
        }
        LintReport {
            findings,
            complete,
            comparisons,
            comparison_bytes,
        }
    }
}

fn matcher_source_bytes(matcher: &Matcher) -> usize {
    match matcher {
        Matcher::ExactUrl(url) => url.len(),
        Matcher::Glob(glob) => glob
            .scheme
            .as_ref()
            .map_or(0, String::len)
            .saturating_add(glob.host.len())
            .saturating_add(glob.port.as_ref().map_or(0, String::len))
            .saturating_add(glob.path.as_ref().map_or(0, String::len))
            .saturating_add(glob.query.as_ref().map_or(0, String::len)),
        Matcher::Port(_) => size_of::<u16>(),
        Matcher::Regex(regex) => regex.pattern.len(),
        Matcher::Not(inner) => matcher_source_bytes(inner),
    }
}

fn lint_finding_bytes(finding: &LintFinding) -> usize {
    finding
        .group
        .len()
        .saturating_add(finding.raw.len())
        .saturating_add(finding.shadowed_by_group.len())
        .saturating_add(finding.shadowed_by_raw.len())
        .saturating_add(finding.families.iter().map(String::len).sum::<usize>())
}

fn single_families(rule: &Rule) -> Vec<String> {
    let mut families = Vec::new();
    for action in effective_actions(rule) {
        if action.is_single() {
            let family = action.family().as_str().to_string();
            if !families.contains(&family) {
                families.push(family);
            }
        }
    }
    families
}

/// Reports whether `a` provably matches every URL that `b` matches.
fn matcher_subsumes(a: &Matcher, b: &Matcher, globs: &CompiledGlobSet) -> bool {
    match (a, b) {
        (Matcher::Glob(a), Matcher::Glob(b)) => glob_subsumes(a, b, globs),
        (Matcher::Glob(_), Matcher::ExactUrl(url)) => {
            let resources = bind_matcher_resources(a, globs);
            UrlParts::parse(url)
                .ok()
                .is_some_and(|parts| a.matches_compiled(&parts, url, globs, &resources).is_some())
        }
        (Matcher::ExactUrl(a), Matcher::ExactUrl(b)) => a == b,
        _ => false,
    }
}

fn glob_subsumes(a: &GlobMatcher, b: &GlobMatcher, globs: &CompiledGlobSet) -> bool {
    let scheme_ok = a.scheme.is_none() || a.scheme == b.scheme;
    let host_ok = host_pattern_subsumes(&a.host, &b.host, globs);
    let port_ok = a.port.is_none() || a.port == b.port;
    let path_ok = a.path.is_none() || a.path == b.path;
    let query_ok = a.query.is_none() || a.query == b.query;
    scheme_ok && host_ok && port_ok && path_ok && query_ok
}

/// Reports whether host pattern `a` covers every host that pattern `b` covers.
fn host_pattern_subsumes(a: &str, b: &str, globs: &CompiledGlobSet) -> bool {
    if a == b || a == "*" || a == "**" {
        return true;
    }
    // A literal host on the later rule reduces subsumption to plain matching.
    if !glob_syntax_is_active(b) {
        return globs.host_matches(a, b);
    }
    if let Some(base) = a.strip_prefix("**.")
        && let Some(later_base) = b.strip_prefix("**.").or_else(|| b.strip_prefix("*."))
    {
        return !glob_syntax_is_active(later_base)
            && (later_base == base || dotted_suffix_prefix(later_base, base).is_some());
    }
    false
}
