use super::*;

impl RuleSet {
    /// Resolves request-phase actions with request-body-dependent rules enabled.
    ///
    /// Response-only conditions do not match because no response snapshot exists.
    pub fn resolve(&self, req: &RequestMeta) -> ResolveResult {
        self.resolve_inner(req, None, true)
    }

    /// Resolves with an immutable response snapshot available to conditions and templates.
    pub fn resolve_response(&self, req: &RequestMeta, res: &ResponseMeta) -> ResolveResult {
        self.resolve_inner(req, Some(res), true)
    }

    /// Resolves request-phase actions while omitting body-dependent conditions and actions.
    ///
    /// Non-body operations in a mixed [`DeleteOp`] action are retained.
    pub fn resolve_without_request_body(&self, req: &RequestMeta) -> ResolveResult {
        self.resolve_inner(req, None, false)
    }

    /// Resolves response-phase actions without evaluating or returning request-body work.
    pub fn resolve_response_without_request_body(
        &self,
        req: &RequestMeta,
        res: &ResponseMeta,
    ) -> ResolveResult {
        self.resolve_inner(req, Some(res), false)
    }

    /// Reports whether any still-viable rule needs the request body to decide or execute.
    ///
    /// Matcher and body-independent conditions are evaluated first, avoiding body
    /// buffering when every candidate has already been ruled out.
    pub fn request_body_required(&self, req: &RequestMeta) -> bool {
        let parts = UrlParts::parse(&req.url);
        let condition_cache = matcher::ConditionCache::new(req);
        let condition_context = matcher::ConditionMatchContext::compiled(
            parts.as_ref().ok(),
            None,
            0,
            &self.index.compiled_globs,
            &self.index.compiled_body_literals,
            &condition_cache,
        );
        self.candidate_rule_indices(parts.as_ref().ok(), &req.url)
            .into_iter()
            .filter(|idx| !self.rules[*idx].disabled)
            .filter(|idx| {
                let rule = &self.rules[*idx];
                let resources = &self.index.compiled_resources[*idx];
                parts
                    .as_ref()
                    .ok()
                    .and_then(|url| {
                        rule.matcher.matches_compiled(
                            url,
                            &req.url,
                            &self.index.compiled_globs,
                            &resources.matcher,
                        )
                    })
                    .is_some()
            })
            .filter(|idx| {
                let rule = &self.rules[*idx];
                let resources = &self.index.compiled_resources[*idx];
                let context = condition_context.with_line(rule.line);
                rule.conditions
                    .iter()
                    .zip(&resources.conditions)
                    .all(|(condition, resources)| {
                        condition.may_match_before_request_body(resources, &context)
                    })
            })
            .any(|idx| {
                let rule = &self.rules[idx];
                rule.conditions
                    .iter()
                    .any(Condition::depends_on_request_body)
                    || rule.actions.iter().any(action_requires_request_body)
            })
    }
}

fn action_requires_request_body(action: &Action) -> bool {
    matches!(action, Action::ReqBody(_))
        || matches!(
            action,
            Action::Delete(operations)
                if operations.iter().any(|operation| {
                    matches!(operation, DeleteOp::ReqBody | DeleteOp::ReqBodyPath(_))
                })
        )
}

pub(super) fn action_for_body_availability(
    action: &Action,
    request_body_available: bool,
) -> Option<Action> {
    if request_body_available {
        return Some(action.clone());
    }
    match action {
        Action::ReqBody(_) => None,
        Action::Delete(operations) => {
            let operations = operations
                .iter()
                .filter(|operation| {
                    !matches!(operation, DeleteOp::ReqBody | DeleteOp::ReqBodyPath(_))
                })
                .cloned()
                .collect::<Vec<_>>();
            (!operations.is_empty()).then_some(Action::Delete(operations))
        }
        _ => Some(action.clone()),
    }
}
