/// True when `family` equals `prefix` or is nested under it (`prefix.child`).
pub(crate) fn family_within(family: &str, prefix: &str) -> bool {
    family
        .strip_prefix(prefix)
        .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with('.'))
}

pub(super) fn normalize_skip_family(family: &str) -> String {
    family
        .trim()
        .trim_matches(['"', '\''])
        .to_ascii_lowercase()
        .replace(['_', '-'], ".")
}

/// Tracks which action families earlier `skip(...)` actions suppress.
///
/// Families are re-normalized on observation so programmatically built
/// `Action::Skip` values behave like parser output.
#[derive(Default)]
pub(crate) struct SkipState {
    families: crate::ActionFamilySet,
}

impl SkipState {
    pub(crate) fn suppresses(&self, family: crate::ActionFamily) -> bool {
        self.families.contains(family)
    }

    pub(crate) fn observe(&mut self, action: &crate::Action) {
        let crate::Action::Skip(families) = action else {
            return;
        };
        if families.is_empty() {
            self.families = crate::ActionFamilySet::ALL;
        } else {
            self.families.union(*families);
        }
    }
}

/// Returns same-rule actions that survive earlier `skip(...)` actions.
pub(crate) fn effective_actions(rule: &crate::Rule) -> Vec<&crate::Action> {
    let mut skip = SkipState::default();
    let mut actions = Vec::new();
    for action in &rule.actions {
        if skip.suppresses(action.family()) {
            continue;
        }
        actions.push(action);
        skip.observe(action);
    }
    actions
}
