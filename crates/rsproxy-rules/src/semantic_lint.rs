use super::*;
use crate::language::status_forbids_body;
use std::collections::{BTreeMap, BTreeSet};

mod finding;
mod sets;
use finding::{finding, semantic_finding_bytes};
use sets::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Stable category of a conservative semantic lint finding.
pub enum SemanticLintKind {
    /// A later action in the same rule can never win its single-action family.
    DuplicateSingleFamily,
    /// Flat/`all(...)` constraints have a provably empty intersection.
    UnsatisfiableConditions,
    /// A request-only action is guarded by a condition that requires a response.
    RequestActionRequiresResponse,
    /// An action is suppressed by an earlier `skip(...)` in the same rule.
    ActionAfterSkip,
    /// More than one mutually exclusive local-response action is present.
    ConflictingTerminalActions,
    /// Response-only actions cannot run after a local response short-circuits upstream I/O.
    ResponseActionWithLocalResponse,
    /// `direct` makes an `upstream(...)` action in the same rule ineffective.
    UpstreamOverriddenByDirect,
    /// A body mutation cannot affect a response status that forbids content.
    BodyActionWithBodylessStatus,
}

impl SemanticLintKind {
    /// Returns the stable machine-readable finding identifier.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DuplicateSingleFamily => "duplicate-single-family",
            Self::UnsatisfiableConditions => "unsatisfiable-conditions",
            Self::RequestActionRequiresResponse => "request-action-requires-response",
            Self::ActionAfterSkip => "action-after-skip",
            Self::ConflictingTerminalActions => "conflicting-terminal-actions",
            Self::ResponseActionWithLocalResponse => "response-action-with-local-response",
            Self::UpstreamOverriddenByDirect => "upstream-overridden-by-direct",
            Self::BodyActionWithBodylessStatus => "body-action-with-bodyless-status",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A conservative same-rule semantic issue reported by [`RuleSet::semantic_lint`].
pub struct SemanticLintFinding {
    /// Stable finding category.
    pub kind: SemanticLintKind,
    /// Source group containing the rule.
    pub group: Arc<str>,
    /// One-based source line.
    pub line: usize,
    /// Comment-free source text.
    pub raw: Arc<str>,
    /// Human-readable explanation that is not intended as a machine key.
    pub message: String,
    /// Relevant action families, empty for condition-only findings.
    pub families: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Bounded same-rule semantic-lint result.
pub struct SemanticLintReport {
    /// Findings retained in deterministic rule/kind order.
    pub findings: Vec<SemanticLintFinding>,
    /// False when the finding or report-byte budget was reached.
    pub complete: bool,
}

impl RuleSet {
    /// Reports provable same-rule conflicts without guessing about regex
    /// overlap, ambiguous disjunctions, environment contents, or runtime data.
    pub fn semantic_lint(&self) -> Vec<SemanticLintFinding> {
        self.semantic_lint_report().findings
    }

    /// Runs semantic lint with explicit completeness metadata.
    pub fn semantic_lint_report(&self) -> SemanticLintReport {
        let mut findings = Vec::new();
        let mut report_bytes = 0usize;
        for rule in self.rules.iter().filter(|rule| !rule.disabled) {
            let mut current = Vec::new();
            let effective = effective_actions(rule);
            let duplicate_families = duplicate_single_families(&effective);
            if !duplicate_families.is_empty() {
                current.push(finding(
                    rule,
                    SemanticLintKind::DuplicateSingleFamily,
                    format!(
                        "later actions can never win single-action families: {}",
                        duplicate_families.join(", ")
                    ),
                    duplicate_families,
                ));
            }
            if let Some(message) = unsatisfiable_conditions(&rule.conditions) {
                current.push(finding(
                    rule,
                    SemanticLintKind::UnsatisfiableConditions,
                    message,
                    Vec::new(),
                ));
            }
            let phase_families = request_actions_requiring_response(rule, &effective);
            if !phase_families.is_empty() {
                current.push(finding(
                    rule,
                    SemanticLintKind::RequestActionRequiresResponse,
                    format!(
                        "request-only actions cannot run after the required response metadata exists: {}",
                        phase_families.join(", ")
                    ),
                    phase_families,
                ));
            }
            let skipped_families = actions_after_skip(rule);
            if !skipped_families.is_empty() {
                current.push(finding(
                    rule,
                    SemanticLintKind::ActionAfterSkip,
                    format!(
                        "earlier skip actions suppress later families in this rule: {}",
                        skipped_families.join(", ")
                    ),
                    skipped_families,
                ));
            }
            let terminal_families = terminal_families(&effective);
            if terminal_families.len() > 1 {
                current.push(finding(
                    rule,
                    SemanticLintKind::ConflictingTerminalActions,
                    format!(
                        "only one local response can be sent; precedence is status, then redirect, then mock: {}",
                        terminal_families.join(", ")
                    ),
                    terminal_families.clone(),
                ));
            }
            let response_families =
                response_actions_with_local_response(&effective, &terminal_families);
            if !response_families.is_empty() {
                current.push(finding(
                    rule,
                    SemanticLintKind::ResponseActionWithLocalResponse,
                    format!(
                        "local response action {} bypasses response-phase actions: {}",
                        terminal_families.join(", "),
                        response_families.join(", ")
                    ),
                    response_families,
                ));
            }
            if has_effective_family(&effective, &ActionFamily::Direct)
                && has_effective_family(&effective, &ActionFamily::Upstream)
            {
                current.push(finding(
                    rule,
                    SemanticLintKind::UpstreamOverriddenByDirect,
                    "direct routing always overrides the upstream action in this rule".to_string(),
                    vec!["upstream".to_string(), "direct".to_string()],
                ));
            }
            let bodyless_families = body_actions_with_bodyless_status(&effective);
            if !bodyless_families.is_empty() {
                current.push(finding(
                    rule,
                    SemanticLintKind::BodyActionWithBodylessStatus,
                    format!(
                        "res.status(204/205/304) suppresses response content actions: {}",
                        bodyless_families[1..].join(", ")
                    ),
                    bodyless_families,
                ));
            }
            for finding in current {
                let size = semantic_finding_bytes(&finding);
                if findings.len() == MAX_RULE_LINT_FINDINGS
                    || report_bytes
                        .checked_add(size)
                        .is_none_or(|bytes| bytes > MAX_RULE_LINT_REPORT_BYTES)
                {
                    return SemanticLintReport {
                        findings,
                        complete: false,
                    };
                }
                report_bytes += size;
                findings.push(finding);
            }
        }
        SemanticLintReport {
            findings,
            complete: true,
        }
    }
}

fn duplicate_single_families(effective: &[&Action]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut duplicate = BTreeSet::new();
    for action in effective {
        if action.is_single() && !seen.insert(action.family()) {
            duplicate.insert(action.family().as_str().to_string());
        }
    }
    duplicate.into_iter().collect()
}

fn request_actions_requiring_response(rule: &Rule, effective: &[&Action]) -> Vec<String> {
    if !rule.conditions.iter().any(condition_requires_response) {
        return Vec::new();
    }
    effective
        .iter()
        .filter(|action| action.applies_in(Phase::Req) && !action.applies_in(Phase::Res))
        .map(|action| action.family())
        .map(|family| family.as_str().to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn condition_requires_response(condition: &Condition) -> bool {
    match condition {
        Condition::ResHeaderPresent(_)
        | Condition::ResHeaderContains { .. }
        | Condition::Status(_) => true,
        Condition::All(conditions) => conditions.iter().any(condition_requires_response),
        Condition::Any(conditions) => conditions.iter().all(condition_requires_response),
        Condition::Not(inner) => inner.depends_on_response(),
        _ => false,
    }
}

fn actions_after_skip(rule: &Rule) -> Vec<String> {
    let mut skip = SkipState::default();
    let mut ineffective = BTreeSet::new();
    for action in &rule.actions {
        if skip.suppresses(action.family()) {
            ineffective.insert(action.family().as_str().to_string());
            continue;
        }
        skip.observe(action);
    }
    ineffective.into_iter().collect()
}

fn terminal_families(effective: &[&Action]) -> Vec<String> {
    [
        ActionFamily::Status,
        ActionFamily::Redirect,
        ActionFamily::Mock,
    ]
    .into_iter()
    .filter(|family| has_effective_family(effective, family))
    .map(|family| family.as_str().to_string())
    .collect()
}

fn response_actions_with_local_response(
    effective: &[&Action],
    terminal_families: &[String],
) -> Vec<String> {
    if terminal_families.is_empty() {
        return Vec::new();
    }
    effective
        .iter()
        .filter(|action| action.applies_in(Phase::Res) && !action.applies_in(Phase::Req))
        .map(|action| action.family())
        .map(|family| family.as_str().to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn has_effective_family(effective: &[&Action], family: &ActionFamily) -> bool {
    effective.iter().any(|action| action.family() == *family)
}

fn body_actions_with_bodyless_status(effective: &[&Action]) -> Vec<String> {
    if !effective
        .iter()
        .any(|action| matches!(action, Action::ResStatus(status) if status_forbids_body(*status)))
    {
        return Vec::new();
    }
    let mut families = effective
        .iter()
        .map(|action| action.family())
        .filter(|family| {
            matches!(
                family,
                ActionFamily::ResBodySet
                    | ActionFamily::ResBodyPrepend
                    | ActionFamily::ResBodyAppend
                    | ActionFamily::ResBodyReplace
                    | ActionFamily::Inject
                    | ActionFamily::ResMerge
            )
        })
        .map(|family| family.as_str().to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if !families.is_empty() {
        families.insert(0, "res.status".to_string());
    }
    families
}

fn unsatisfiable_conditions(conditions: &[Condition]) -> Option<String> {
    if conditions
        .iter()
        .any(|condition| constant_truth(condition) == ConstantTruth::False)
    {
        return Some("a constant chance/boolean condition can never match".to_string());
    }

    let mut conjunction = Vec::new();
    for condition in conditions {
        flatten_conjunction(condition, &mut conjunction);
    }

    if method_constraints_are_empty(&conjunction) {
        return Some("method constraints have no value in common".to_string());
    }
    if status_constraints_are_empty(&conjunction) {
        return Some("response-status constraints have no value in common".to_string());
    }

    if let Some(message) = environment_contradiction(&conjunction) {
        return Some(message);
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConstantTruth {
    True,
    False,
    Unknown,
}

fn constant_truth(condition: &Condition) -> ConstantTruth {
    match condition {
        Condition::ChancePermille(0) => ConstantTruth::False,
        Condition::ChancePermille(1000) => ConstantTruth::True,
        Condition::Any(conditions) => {
            let mut unknown = false;
            for condition in conditions {
                match constant_truth(condition) {
                    ConstantTruth::True => return ConstantTruth::True,
                    ConstantTruth::Unknown => unknown = true,
                    ConstantTruth::False => {}
                }
            }
            if unknown {
                ConstantTruth::Unknown
            } else {
                ConstantTruth::False
            }
        }
        Condition::All(conditions) => {
            let mut unknown = false;
            for condition in conditions {
                match constant_truth(condition) {
                    ConstantTruth::False => return ConstantTruth::False,
                    ConstantTruth::Unknown => unknown = true,
                    ConstantTruth::True => {}
                }
            }
            if unknown {
                ConstantTruth::Unknown
            } else {
                ConstantTruth::True
            }
        }
        Condition::Not(inner) => match constant_truth(inner) {
            ConstantTruth::True => ConstantTruth::False,
            ConstantTruth::False => ConstantTruth::True,
            ConstantTruth::Unknown => ConstantTruth::Unknown,
        },
        _ => ConstantTruth::Unknown,
    }
}

fn method_constraints_are_empty(conjunction: &[&Condition]) -> bool {
    let positive =
        string_intersection(conjunction.iter().filter_map(|condition| match condition {
            Condition::Method(values) => Some(values.as_slice()),
            _ => None,
        }));
    let forbidden = conjunction
        .iter()
        .filter_map(|condition| match condition {
            Condition::Not(inner) => match inner.as_ref() {
                Condition::Method(values) => Some(values.as_slice()),
                _ => None,
            },
            _ => None,
        })
        .flatten()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    positive.is_some_and(|values| values.is_subset(&forbidden))
}

fn status_constraints_are_empty(conjunction: &[&Condition]) -> bool {
    let positive = u16_intersection(conjunction.iter().filter_map(|condition| match condition {
        Condition::Status(values) => Some(values.as_slice()),
        _ => None,
    }));
    let forbidden = conjunction
        .iter()
        .filter_map(|condition| match condition {
            Condition::Not(inner) => match inner.as_ref() {
                Condition::Status(values) => Some(values.as_slice()),
                _ => None,
            },
            _ => None,
        })
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    positive.is_some_and(|values| values.is_subset(&forbidden))
}

fn environment_contradiction(conjunction: &[&Condition]) -> Option<String> {
    let mut required = BTreeMap::new();
    let mut required_present = BTreeSet::new();
    let mut forbidden_present = BTreeSet::new();
    let mut forbidden_values = BTreeMap::<&str, BTreeSet<&str>>::new();

    for condition in conjunction {
        match condition {
            Condition::EnvPresent(name) => {
                required_present.insert(name.as_str());
            }
            Condition::EnvEquals { name, value } => {
                required_present.insert(name.as_str());
                if let Some(previous) = required.insert(name.as_str(), value.as_str())
                    && previous != value
                {
                    return Some(format!(
                        "environment variable `{name}` is required to equal both `{previous}` and `{value}`"
                    ));
                }
            }
            Condition::Not(inner) => match inner.as_ref() {
                Condition::EnvPresent(name) => {
                    forbidden_present.insert(name.as_str());
                }
                Condition::EnvEquals { name, value } => {
                    forbidden_values
                        .entry(name.as_str())
                        .or_default()
                        .insert(value.as_str());
                }
                _ => {}
            },
            _ => {}
        }
    }

    if let Some(name) = required_present.intersection(&forbidden_present).next() {
        return Some(format!(
            "environment variable `{name}` is required to be both present and absent"
        ));
    }
    for (name, value) in required {
        if forbidden_values
            .get(name)
            .is_some_and(|values| values.contains(value))
        {
            return Some(format!(
                "environment variable `{name}` is required to equal and not equal `{value}`"
            ));
        }
    }
    None
}
