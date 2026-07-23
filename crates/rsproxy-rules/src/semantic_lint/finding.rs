use super::*;

pub(super) fn finding(
    rule: &Rule,
    kind: SemanticLintKind,
    message: String,
    families: Vec<String>,
) -> SemanticLintFinding {
    SemanticLintFinding {
        kind,
        group: rule.group.clone(),
        line: rule.line,
        raw: rule.raw.clone(),
        message,
        families,
    }
}

pub(super) fn semantic_finding_bytes(finding: &SemanticLintFinding) -> usize {
    finding
        .group
        .len()
        .saturating_add(finding.raw.len())
        .saturating_add(finding.message.len())
        .saturating_add(finding.families.iter().map(String::len).sum::<usize>())
}
