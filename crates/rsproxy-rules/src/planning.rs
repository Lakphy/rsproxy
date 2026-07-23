use super::*;
use crate::model::CompiledConditionResources;

impl Condition {
    /// Calls `visit` for every node in this condition tree, combinators included.
    pub(crate) fn for_each_node(&self, visit: &mut impl FnMut(&Condition)) {
        visit(self);
        match self {
            Condition::Any(conditions) | Condition::All(conditions) => {
                for condition in conditions {
                    condition.for_each_node(visit);
                }
            }
            Condition::Not(inner) => inner.for_each_node(visit),
            _ => {}
        }
    }

    pub(super) fn depends_on_request_body(&self) -> bool {
        match self {
            Condition::BodyContains(_) | Condition::BodyRegex(_) => true,
            Condition::Any(conditions) | Condition::All(conditions) => {
                conditions.iter().any(Self::depends_on_request_body)
            }
            Condition::Not(inner) => inner.depends_on_request_body(),
            _ => false,
        }
    }

    pub(super) fn may_match_before_request_body(
        &self,
        resources: &CompiledConditionResources,
        context: &matcher::ConditionMatchContext<'_, '_>,
    ) -> bool {
        match self {
            Condition::BodyContains(_)
            | Condition::BodyRegex(_)
            | Condition::ResHeaderPresent(_)
            | Condition::ResHeaderContains { .. }
            | Condition::Status(_) => true,
            Condition::Any(conditions) => {
                let CompiledConditionResources::Children(children) = resources else {
                    return false;
                };
                conditions
                    .iter()
                    .zip(children)
                    .any(|(condition, resources)| {
                        condition.may_match_before_request_body(resources, context)
                    })
            }
            Condition::All(conditions) => {
                let CompiledConditionResources::Children(children) = resources else {
                    return false;
                };
                conditions
                    .iter()
                    .zip(children)
                    .all(|(condition, resources)| {
                        condition.may_match_before_request_body(resources, context)
                    })
            }
            Condition::Not(inner)
                if inner.depends_on_request_body() || inner.depends_on_response() =>
            {
                true
            }
            _ => self.matches_with_compiled(resources, context),
        }
    }

    pub(crate) fn depends_on_response(&self) -> bool {
        match self {
            Condition::ResHeaderPresent(_)
            | Condition::ResHeaderContains { .. }
            | Condition::Status(_) => true,
            Condition::Any(conditions) | Condition::All(conditions) => {
                conditions.iter().any(Self::depends_on_response)
            }
            Condition::Not(inner) => inner.depends_on_response(),
            _ => false,
        }
    }
}
