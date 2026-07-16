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
    pub group: String,
    /// One-based source line of the shadowed rule.
    pub line: usize,
    /// Comment-free source of the shadowed rule.
    pub raw: String,
    /// Group of the earlier rule that wins first.
    pub shadowed_by_group: String,
    /// One-based source line of the earlier rule.
    pub shadowed_by_line: usize,
    /// Comment-free source of the earlier rule.
    pub shadowed_by_raw: String,
    /// Single-action families the later rule can never win.
    pub families: Vec<String>,
}

impl RuleSet {
    /// Reports later single-action-family rules that are unreachable because an
    /// earlier, condition-free rule with a broader matcher always wins first.
    ///
    /// The subsumption check is conservative: only glob/exact matchers with a
    /// provable coverage relationship produce findings, so an empty result does
    /// not guarantee the absence of ordering mistakes.
    pub fn lint(&self) -> Vec<LintFinding> {
        let mut ordered: Vec<usize> = (0..self.rules.len())
            .filter(|idx| !self.rules[*idx].disabled)
            .collect();
        ordered.sort_by_key(|idx| {
            let rule = &self.rules[*idx];
            (!rule.important, *idx)
        });

        let mut findings = Vec::new();
        for (pos, &later_idx) in ordered.iter().enumerate() {
            let later = &self.rules[later_idx];
            let later_families = single_families(later);
            if later_families.is_empty() {
                continue;
            }
            let mut remaining = later_families;
            for &earlier_idx in &ordered[..pos] {
                let earlier = &self.rules[earlier_idx];
                if !earlier.conditions.is_empty() {
                    continue;
                }
                let shared: Vec<String> = single_families(earlier)
                    .into_iter()
                    .filter(|family| remaining.contains(family))
                    .collect();
                if shared.is_empty() {
                    continue;
                }
                if !matcher_subsumes(&earlier.matcher, &later.matcher) {
                    continue;
                }
                remaining.retain(|family| !shared.contains(family));
                findings.push(LintFinding {
                    group: later.group.clone(),
                    line: later.line,
                    raw: later.raw.clone(),
                    shadowed_by_group: earlier.group.clone(),
                    shadowed_by_line: earlier.line,
                    shadowed_by_raw: earlier.raw.clone(),
                    families: shared,
                });
                if remaining.is_empty() {
                    break;
                }
            }
        }
        findings
    }
}

fn single_families(rule: &Rule) -> Vec<String> {
    let mut families = Vec::new();
    for action in &rule.actions {
        if action.is_single() {
            let family = action.family().to_string();
            if !families.contains(&family) {
                families.push(family);
            }
        }
    }
    families
}

/// Reports whether `a` provably matches every URL that `b` matches.
fn matcher_subsumes(a: &Matcher, b: &Matcher) -> bool {
    match (a, b) {
        (Matcher::Glob(a), Matcher::Glob(b)) => glob_subsumes(a, b),
        (Matcher::Glob(_), Matcher::ExactUrl(url)) => UrlParts::parse(url)
            .ok()
            .is_some_and(|parts| a.matches(&parts, url).is_some()),
        (Matcher::ExactUrl(a), Matcher::ExactUrl(b)) => a == b,
        _ => false,
    }
}

fn glob_subsumes(a: &GlobMatcher, b: &GlobMatcher) -> bool {
    let scheme_ok = a.scheme.is_none() || a.scheme == b.scheme;
    let host_ok = host_pattern_subsumes(&a.host, &b.host);
    let port_ok = a.port.is_none() || a.port == b.port;
    let path_ok = a.path.is_none() || a.path == b.path;
    let query_ok = a.query.is_none() || a.query == b.query;
    scheme_ok && host_ok && port_ok && path_ok && query_ok
}

/// Reports whether host pattern `a` covers every host that pattern `b` covers.
fn host_pattern_subsumes(a: &str, b: &str) -> bool {
    let a = a.to_ascii_lowercase();
    let b = b.to_ascii_lowercase();
    if a == b || a == "*" || a == "**" {
        return true;
    }
    // A literal host on the later rule reduces subsumption to plain matching.
    if !b.contains('*') {
        return host_matches(&a, &b);
    }
    if let Some(base) = a.strip_prefix("**.")
        && let Some(later_base) = b.strip_prefix("**.").or_else(|| b.strip_prefix("*."))
    {
        return !later_base.contains('*')
            && (later_base == base || later_base.ends_with(&format!(".{base}")));
    }
    false
}
